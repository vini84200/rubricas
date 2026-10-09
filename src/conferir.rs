//! Como o executor julga o que saiu de cada checagem. Funções puras: quem roda
//! os contêineres está em `executor.rs`.

use crate::fila::Par;
use crate::pipeline::{Caso, ResultadoCaso, ResultadoChecagem, Situacao, TipoChecagem};

/// O que uma execução do programa do aluno produziu.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Execucao {
    pub saida: String,
    pub erro: String,
    /// `None` quando foi interrompido (tempo esgotado ou sinal).
    pub codigo: Option<i32>,
    pub tempo_esgotado: bool,
}

/// Saída igual à esperada, sem contar espaços no fim das linhas nem linhas em
/// branco no fim.
pub fn confere(esperado: &str, obtido: &str, ignorar_maiusculas: bool) -> bool {
    let limpar = |t: &str| {
        let linhas: Vec<&str> = t.lines().map(str::trim_end).collect();
        let fim = linhas.iter().rposition(|l| !l.is_empty()).map_or(0, |i| i + 1);
        let texto = linhas[..fim].join("\n");
        if ignorar_maiusculas { texto.to_lowercase() } else { texto }
    };
    limpar(esperado) == limpar(obtido)
}

/// Os últimos ~2 KB de um texto, começando numa linha inteira.
pub fn fim(texto: &str) -> String {
    const MAX: usize = 2048;
    if texto.len() <= MAX {
        return texto.to_string();
    }
    let mut corte = texto.len() - MAX;
    while !texto.is_char_boundary(corte) {
        corte += 1;
    }
    let resto = &texto[corte..];
    match resto.find('\n') {
        Some(i) if i + 1 < resto.len() => format!("…\n{}", &resto[i + 1..]),
        _ => format!("…{resto}"),
    }
}

/// Julga os casos de uma checagem `casos`.
pub fn julgar_casos(nome: &str, casos: &[Caso], execucoes: &[Execucao], ignorar_maiusculas: bool) -> ResultadoChecagem {
    let mut resultados = Vec::new();
    for (caso, ex) in casos.iter().zip(execucoes) {
        let motivo = if ex.tempo_esgotado {
            "tempo esgotado".to_string()
        } else {
            match ex.codigo {
                Some(0) if confere(&caso.saida, &ex.saida, ignorar_maiusculas) => String::new(),
                Some(0) => "saída diferente".into(),
                Some(c) => format!("terminou com erro (código {c})"),
                None => "interrompido".into(),
            }
        };
        resultados.push(ResultadoCaso {
            nome: caso.nome.clone(),
            passou: motivo.is_empty(),
            entrada: caso.entrada.clone(),
            esperado: caso.saida.clone(),
            obtido: fim(&ex.saida),
            motivo,
            erro: fim(&ex.erro),
        });
    }
    let passaram = resultados.iter().filter(|r| r.passou).count() as u32;
    let total = casos.len() as u32;
    ResultadoChecagem {
        nome: nome.into(),
        tipo: TipoChecagem::Casos,
        situacao: if passaram == total { Situacao::Passou } else { Situacao::Falhou },
        passaram,
        total,
        resumo: format!("{passaram} de {total} {}", if total == 1 { "caso" } else { "casos" }),
        casos: resultados,
        saida: String::new(),
    }
}

/// (passaram, total) a partir da saída do `pytest -q`. Erros de coleta (o
/// arquivo do aluno nem importa) contam como falha.
pub fn resumo_pytest(saida: &str) -> Option<(u32, u32)> {
    let linha = saida.lines().rev().map(|l| l.trim().trim_matches('=').trim()).find(|l| {
        l.contains(" in ") && (l.contains("passed") || l.contains("failed") || l.contains("error") || l.starts_with("no tests ran"))
    })?;
    let (mut passaram, mut falharam) = (0, 0);
    for parte in linha.split(" in ").next()?.split(", ") {
        let mut p = parte.split_whitespace();
        let (Some(n), Some(o_que)) = (p.next().and_then(|n| n.parse::<u32>().ok()), p.next()) else { continue };
        match o_que {
            "passed" | "xpassed" => passaram += n,
            "failed" | "error" | "errors" => falharam += n,
            _ => {}
        }
    }
    Some((passaram, passaram + falharam))
}

/// Quantos problemas o `ruff check --output-format=concise` encontrou.
pub fn problemas_ruff(saida: &str) -> Option<u32> {
    saida.lines().find_map(|l| {
        let l = l.trim();
        if l.starts_with("All checks passed") {
            return Some(0);
        }
        l.strip_prefix("Found ")?.split_whitespace().next()?.parse().ok()
    })
}

/// Qual arquivo da submissão é o programa: o de nome `desejado` ou, sem ele, o
/// único `.py`.
pub fn escolher_arquivo(arquivos: &[String], desejado: &str) -> Result<String, String> {
    let nome = |c: &String| c.rsplit('/').next().unwrap_or(c).to_lowercase();
    if !desejado.is_empty()
        && let Some(a) = arquivos.iter().find(|c| nome(c) == desejado.to_lowercase())
    {
        return Ok(a.clone());
    }
    let py: Vec<&String> = arquivos.iter().filter(|c| nome(c).ends_with(".py")).collect();
    match py.as_slice() {
        [um] => Ok((*um).clone()),
        [] => Err("a submissão não tem arquivo .py".into()),
        varios => Err(format!(
            "{} arquivos .py ({}){}",
            varios.len(),
            varios.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
            if desejado.is_empty() { String::new() } else { format!(" e nenhum se chama {desejado}") }
        )),
    }
}

/// Pares do `results.csv` do JPlag, com as submissões na raiz `alunos/` e as
/// referências em `referencias/`. Pares entre duas referências não interessam.
pub fn pares_jplag(csv: &str) -> Result<Vec<Par>, String> {
    let mut leitor = csv::Reader::from_reader(csv.as_bytes());
    let cab = leitor.headers().map_err(|e| e.to_string())?.clone();
    let col = |nome: &str| cab.iter().position(|c| c == nome).ok_or(format!("results.csv sem a coluna {nome}"));
    let (c1, c2, cs) = (col("submissionName1")?, col("submissionName2")?, col("averageSimilarity")?);
    // Com uma raiz só (sem referências), o JPlag escreve só o nome; com duas,
    // `alunos/<id>` e `referencias/<nome>`. Ids de aluno nunca têm `/`.
    let nome = |s: &str| match s.strip_prefix("alunos/") {
        Some(aluno) => (aluno.to_string(), false),
        None => (s.to_string(), s.starts_with("referencias/")),
    };
    let mut pares = Vec::new();
    for reg in leitor.records() {
        let reg = reg.map_err(|e| e.to_string())?;
        let (a, ref_a) = nome(reg.get(c1).unwrap_or(""));
        let (b, ref_b) = nome(reg.get(c2).unwrap_or(""));
        if ref_a && ref_b {
            continue;
        }
        let similaridade = reg.get(cs).and_then(|v| v.parse().ok()).ok_or("similaridade ilegível no results.csv")?;
        pares.push(Par { a, b, similaridade });
    }
    Ok(pares)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn saida_confere_ignorando_espacos_no_fim_e_opcionalmente_maiusculas() {
        assert!(confere("5\n", "5", false));
        assert!(confere("a\nb\n", "a  \r\nb\n\n\n", false));
        assert!(!confere("a\nb\n", "a\n\nb\n", false), "linha em branco no meio conta");
        assert!(!confere("  5", "5", false), "espaço no começo conta");
        assert!(!confere("Soma: 5", "soma: 5", false));
        assert!(confere("Soma: 5", "SOMA: 5", true));
    }

    fn caso(entrada: &str, saida: &str) -> Caso {
        Caso { nome: format!("com {entrada}"), entrada: entrada.into(), saida: saida.into(), argumentos: vec![] }
    }

    #[test]
    fn casos_que_nao_rodam_reprovam_e_mostram_o_fim_do_erro() {
        let casos = [caso("2 3", "5"), caso("1 1", "2"), caso("0 0", "0"), caso("9 9", "18")];
        let rodou = |saida: &str| Execucao { saida: saida.into(), codigo: Some(0), ..Default::default() };
        let longo = format!("{}\nZeroDivisionError: division by zero\n", "Traceback linha\n".repeat(200));
        let execucoes = [
            rodou("5\n"),
            rodou("3\n"),
            Execucao { erro: longo, codigo: Some(1), ..Default::default() },
            Execucao { tempo_esgotado: true, ..Default::default() },
        ];
        let r = julgar_casos("Exemplos", &casos, &execucoes, false);
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Falhou, 1, 4, "1 de 4 casos"));
        assert_eq!(r.tipo, TipoChecagem::Casos);
        assert_eq!(r.casos[1].motivo, "saída diferente");
        assert_eq!((r.casos[1].esperado.as_str(), r.casos[1].obtido.as_str()), ("2", "3\n"));
        assert_eq!(r.casos[2].motivo, "terminou com erro (código 1)");
        assert!(r.casos[2].erro.ends_with("ZeroDivisionError: division by zero\n"));
        assert!(r.casos[2].erro.len() <= 2100, "só o fim do stderr");
        assert_eq!(r.casos[3].motivo, "tempo esgotado");

        let tudo = julgar_casos("Exemplos", &casos[..1], &execucoes[..1], false);
        assert_eq!((tudo.situacao, tudo.resumo.as_str()), (Situacao::Passou, "1 de 1 caso"));
    }

    #[test]
    fn le_o_resumo_do_pytest() {
        assert_eq!(resumo_pytest("..F.\n=== short test summary info ===\nFAILED t.py::x\n1 failed, 3 passed in 0.12s\n"), Some((3, 4)));
        assert_eq!(resumo_pytest("5 passed, 1 warning in 0.03s"), Some((5, 5)));
        assert_eq!(resumo_pytest("1 failed, 1 passed, 2 errors in 0.2s"), Some((1, 4)));
        assert_eq!(resumo_pytest("ERROR testes.py - SyntaxError\n1 error in 0.05s\n"), Some((0, 1)));
        assert_eq!(resumo_pytest("2 passed, 1 skipped in 0.1s"), Some((2, 2)));
        assert_eq!(resumo_pytest("no tests ran in 0.01s"), Some((0, 0)));
        assert_eq!(resumo_pytest("python: can't open file"), None);
    }

    #[test]
    fn conta_os_problemas_do_ruff() {
        assert_eq!(problemas_ruff("All checks passed!\n"), Some(0));
        assert_eq!(problemas_ruff("main.py:1:8: F401 [*] `os` imported but unused\nFound 1 error.\n[*] 1 fixable"), Some(1));
        assert_eq!(problemas_ruff("a\nFound 12 errors.\n"), Some(12));
        assert_eq!(problemas_ruff("error: Failed to parse"), None);
    }

    #[test]
    fn escolhe_o_programa_do_aluno() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(escolher_arquivo(&v(&["Lista5_Ana.py"]), "main.py"), Ok("Lista5_Ana.py".into()));
        assert_eq!(escolher_arquivo(&v(&["leiame.txt", "main.py", "util.py"]), "main.py"), Ok("main.py".into()));
        assert_eq!(escolher_arquivo(&v(&["src/MAIN.py", "util.py"]), "main.py"), Ok("src/MAIN.py".into()));
        assert!(escolher_arquivo(&v(&["a.py", "b.py"]), "main.py").unwrap_err().contains("a.py"));
        assert!(escolher_arquivo(&v(&["a.py", "b.py"]), "").is_err());
        assert!(escolher_arquivo(&v(&["leiame.txt"]), "").unwrap_err().contains(".py"));
    }

    #[test]
    fn le_os_pares_do_jplag() {
        let csv = "submissionName1,submissionName2,averageSimilarity,maxSimilarity\n\
                   alunos/1,alunos/2,0.8,0.9\n\
                   referencias/prof,alunos/1,0.5,0.6\n\
                   referencias/prof,referencias/web,0.99,0.99\n\
                   \"alunos/a,b\",alunos/2,0.1,0.2\n";
        let pares = pares_jplag(csv).unwrap();
        assert_eq!(
            pares,
            [
                Par { a: "1".into(), b: "2".into(), similaridade: 0.8 },
                Par { a: "referencias/prof".into(), b: "1".into(), similaridade: 0.5 },
                Par { a: "a,b".into(), b: "2".into(), similaridade: 0.1 },
            ]
        );
        assert!(pares_jplag("x,y\n1,2\n").is_err());

        // sem referências (uma raiz só), o JPlag não põe o nome da pasta na frente
        let sem_refs = "submissionName1,submissionName2,averageSimilarity,maxSimilarity\nana,bruno,1.0,1.0\n";
        assert_eq!(pares_jplag(sem_refs).unwrap(), [Par { a: "ana".into(), b: "bruno".into(), similaridade: 1.0 }]);
    }
}
