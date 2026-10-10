//! Como o executor julga o que saiu de cada checagem. Funções puras: quem roda
//! os contêineres está em `executor.rs`.

use crate::fila::Par;
use crate::pipeline::{Checagem, ResultadoCaso, ResultadoChecagem, Situacao, TipoChecagem};
use serde::Deserialize;

/// O que uma execução produziu: o contêiner de uma checagem ou, nos casos, cada
/// execução do programa do aluno (o script dos casos as devolve neste formato).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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

/// Marca que o script dos casos imprime antes do JSON com as execuções.
pub const MARCA_CASOS: &str = "@@rubricas@@";

/// Linha que o script de estilo imprime quando o `ruff format --check` reprova.
pub const MARCA_FORMATO: &str = "FORMATO";

/// Julga o que a checagem produziu no contêiner.
pub fn julgar(ch: &Checagem, s: &Execucao) -> ResultadoChecagem {
    match ch.tipo {
        TipoChecagem::Casos => julgar_conteiner_dos_casos(ch, s),
        TipoChecagem::Pytest => julgar_pytest(ch, s),
        TipoChecagem::Estilo => julgar_estilo(ch, s),
        TipoChecagem::Comando => julgar_comando(ch, s),
    }
}

/// O que o script dos casos imprimiu depois da marca, se ele chegou ao fim.
pub fn ler_execucoes(saida: &str) -> Option<Vec<Execucao>> {
    let (_, json) = saida.rsplit_once(MARCA_CASOS)?;
    serde_json::from_str(json.trim()).ok()
}

fn julgar_conteiner_dos_casos(ch: &Checagem, s: &Execucao) -> ResultadoChecagem {
    match ler_execucoes(&s.saida) {
        Some(ex) => julgar_casos(ch, &ex),
        // O contêiner inteiro estourou (memória, tempo total): todos os casos falham.
        None if s.tempo_esgotado || s.codigo == Some(137) => {
            let motivo = if s.tempo_esgotado { "tempo esgotado" } else { "memória esgotada" };
            let ex = vec![Execucao { erro: motivo.into(), tempo_esgotado: s.tempo_esgotado, ..Default::default() }; ch.casos.len()];
            julgar_casos(ch, &ex)
        }
        None => ResultadoChecagem::erro(ch, &format!("o executor de casos não respondeu:\n{}{}", s.saida, s.erro)),
    }
}

/// Julga os casos de uma checagem `casos`, uma execução por caso.
pub fn julgar_casos(ch: &Checagem, execucoes: &[Execucao]) -> ResultadoChecagem {
    let mut resultados = Vec::new();
    for (caso, ex) in ch.casos.iter().zip(execucoes) {
        let motivo = if ex.tempo_esgotado {
            "tempo esgotado".to_string()
        } else {
            match ex.codigo {
                Some(0) if confere(&caso.saida, &ex.saida, ch.ignorar_maiusculas) => String::new(),
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
    let total = ch.casos.len() as u32;
    let resumo = format!("{passaram} de {total} {}", if total == 1 { "caso" } else { "casos" });
    ResultadoChecagem { casos: resultados, ..ResultadoChecagem::contagem(ch, passaram, total, resumo, String::new()) }
}

fn julgar_pytest(ch: &Checagem, s: &Execucao) -> ResultadoChecagem {
    let (passaram, total) = resumo_pytest(&s.saida).unwrap_or((0, 0));
    let resumo = if s.tempo_esgotado {
        "tempo esgotado".to_string()
    } else if total == 0 {
        "nenhum teste rodou".to_string()
    } else {
        format!("{passaram} de {total} {}", if total == 1 { "teste" } else { "testes" })
    };
    let r = ResultadoChecagem::contagem(ch, passaram, total, resumo, fim(&format!("{}{}", s.saida, s.erro)));
    // Estourou o tempo: falha, mesmo que o resumo impresso antes diga que tudo passou.
    if s.tempo_esgotado { ResultadoChecagem { situacao: Situacao::Falhou, ..r } } else { r }
}

fn julgar_estilo(ch: &Checagem, s: &Execucao) -> ResultadoChecagem {
    let Some(mut problemas) = problemas_ruff(&s.saida) else {
        return ResultadoChecagem::erro(ch, &format!("não entendi a saída do ruff:\n{}{}", s.saida, s.erro));
    };
    let formato = s.saida.lines().any(|l| l.trim() == MARCA_FORMATO);
    if formato {
        problemas += 1;
    }
    let resumo = match (problemas, formato) {
        (0, _) => "sem problemas de estilo".to_string(),
        (1, true) => "formatação fora do padrão".to_string(),
        (n, true) => format!("{} problemas de estilo e formatação fora do padrão", n - 1),
        (1, false) => "1 problema de estilo".to_string(),
        (n, false) => format!("{n} problemas de estilo"),
    };
    let saida = fim(&s.saida.replace(&format!("{MARCA_FORMATO}\n"), ""));
    ResultadoChecagem::contagem(ch, u32::from(problemas == 0), 1, resumo, saida)
}

fn julgar_comando(ch: &Checagem, s: &Execucao) -> ResultadoChecagem {
    let resumo = match (s.tempo_esgotado, s.codigo) {
        (true, _) => "tempo esgotado".to_string(),
        (_, Some(0)) => "passou".to_string(),
        (_, Some(c)) => format!("terminou com código {c}"),
        (_, None) => "interrompido".to_string(),
    };
    let ok = s.codigo == Some(0) && !s.tempo_esgotado;
    ResultadoChecagem::contagem(ch, u32::from(ok), 1, resumo, fim(&format!("{}{}", s.saida, s.erro)))
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
    use crate::pipeline::Caso;

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

    fn checagem(tipo: &str) -> Checagem {
        toml::from_str(&format!("tipo = \"{tipo}\"\nnome = \"{tipo}\"")).unwrap()
    }

    fn casos(casos: &[Caso]) -> Checagem {
        Checagem { casos: casos.to_vec(), ..checagem("casos") }
    }

    fn saiu(saida: &str, codigo: Option<i32>) -> Execucao {
        Execucao { saida: saida.into(), codigo, ..Default::default() }
    }

    #[test]
    fn casos_que_nao_rodam_reprovam_e_mostram_o_fim_do_erro() {
        let ch = casos(&[caso("2 3", "5"), caso("1 1", "2"), caso("0 0", "0"), caso("9 9", "18")]);
        let longo = format!("{}\nZeroDivisionError: division by zero\n", "Traceback linha\n".repeat(200));
        let execucoes = [
            saiu("5\n", Some(0)),
            saiu("3\n", Some(0)),
            Execucao { erro: longo, codigo: Some(1), ..Default::default() },
            Execucao { tempo_esgotado: true, ..Default::default() },
        ];
        let r = julgar_casos(&ch, &execucoes);
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Falhou, 1, 4, "1 de 4 casos"));
        assert_eq!(r.tipo, TipoChecagem::Casos);
        assert_eq!(r.casos[1].motivo, "saída diferente");
        assert_eq!((r.casos[1].esperado.as_str(), r.casos[1].obtido.as_str()), ("2", "3\n"));
        assert_eq!(r.casos[2].motivo, "terminou com erro (código 1)");
        assert!(r.casos[2].erro.ends_with("ZeroDivisionError: division by zero\n"));
        assert!(r.casos[2].erro.len() <= 2100, "só o fim do stderr");
        assert_eq!(r.casos[3].motivo, "tempo esgotado");

        let tudo = julgar_casos(&casos(&ch.casos[..1]), &execucoes[..1]);
        assert_eq!((tudo.situacao, tudo.resumo.as_str()), (Situacao::Passou, "1 de 1 caso"));
    }

    #[test]
    fn le_os_casos_que_o_conteiner_devolveu() {
        let ch = casos(&[caso("1", "1"), caso("2", "2")]);
        let json = r#"[{"saida":"1\n","erro":"","codigo":0,"tempo_esgotado":false},{"saida":"9\n","erro":"","codigo":0,"tempo_esgotado":false}]"#;
        let r = julgar(&ch, &saiu(&format!("lixo do aluno\n{MARCA_CASOS}{json}\n"), Some(0)));
        assert_eq!((r.situacao, r.passaram, r.total), (Situacao::Falhou, 1, 2));

        // o contêiner estourou a memória ou o tempo antes do fim: todos os casos falham
        let r = julgar(&ch, &saiu("", Some(137)));
        assert_eq!((r.situacao, r.passaram, r.total), (Situacao::Falhou, 0, 2));
        assert_eq!(r.casos[0].erro, "memória esgotada");
        let r = julgar(&ch, &Execucao { tempo_esgotado: true, ..Default::default() });
        assert_eq!(r.casos[1].motivo, "tempo esgotado");

        // terminou sem a marca: falha do executor, não do aluno
        let r = julgar(&ch, &saiu("Traceback: casos.py quebrou", Some(1)));
        assert_eq!((r.situacao, r.total), (Situacao::Erro, 0));
        assert_eq!(r.resumo, "o executor de casos não respondeu:");
        assert!(r.saida.contains("casos.py quebrou"));
    }

    #[test]
    fn julga_o_pytest() {
        let ch = checagem("pytest");
        let r = julgar(&ch, &saiu("..F.\n1 failed, 3 passed in 0.12s\n", Some(1)));
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Falhou, 3, 4, "3 de 4 testes"));
        let r = julgar(&ch, &saiu("1 passed in 0.01s\n", Some(0)));
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Passou, 1, 1, "1 de 1 teste"));
        // nenhum teste rodou (o arquivo do aluno nem importa): uma falha
        let r = julgar(&ch, &Execucao { erro: "ImportError".into(), codigo: Some(4), ..Default::default() });
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Falhou, 0, 1, "nenhum teste rodou"));
        assert_eq!(r.saida, "ImportError");
        // estourou o tempo: não passa, nem com o resumo todo verde
        let r = julgar(&ch, &Execucao { saida: "2 passed in 9s".into(), tempo_esgotado: true, ..Default::default() });
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Falhou, 2, 2, "tempo esgotado"));
    }

    #[test]
    fn julga_o_estilo() {
        let ch = checagem("estilo");
        let r = julgar(&ch, &saiu("All checks passed!\n", Some(0)));
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Passou, 1, 1, "sem problemas de estilo"));
        let r = julgar(&ch, &saiu("a.py:1:1: F401\nb.py:2:1: E501\nFound 2 errors.\n", Some(1)));
        assert_eq!((r.situacao, r.passaram, r.resumo.as_str()), (Situacao::Falhou, 0, "2 problemas de estilo"));
        assert_eq!(julgar(&ch, &saiu("x\nFound 1 error.\n", Some(1))).resumo, "1 problema de estilo");

        // a marca do `ruff format --check` conta como mais um problema e some da saída
        let formato = format!("All checks passed!\n{MARCA_FORMATO}\n");
        let r = julgar(&ch, &saiu(&formato, Some(0)));
        assert_eq!((r.situacao, r.resumo.as_str()), (Situacao::Falhou, "formatação fora do padrão"));
        assert_eq!(r.saida, "All checks passed!\n");
        let r = julgar(&ch, &saiu(&format!("x\nFound 3 errors.\n{MARCA_FORMATO}\n"), Some(1)));
        assert_eq!(r.resumo, "3 problemas de estilo e formatação fora do padrão");

        // saída que não é do ruff: falha do executor
        let r = julgar(&ch, &Execucao { erro: "sh: ruff: not found".into(), codigo: Some(127), ..Default::default() });
        assert_eq!((r.situacao, r.resumo.as_str()), (Situacao::Erro, "não entendi a saída do ruff:"));
    }

    #[test]
    fn julga_o_comando() {
        let ch = checagem("comando");
        let r = julgar(&ch, &saiu("ok\n", Some(0)));
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Passou, 1, 1, "passou"));
        let r = julgar(&ch, &Execucao { saida: "a".into(), erro: "b".into(), codigo: Some(2), ..Default::default() });
        assert_eq!((r.situacao, r.passaram, r.resumo.as_str(), r.saida.as_str()), (Situacao::Falhou, 0, "terminou com código 2", "ab"));
        assert_eq!(julgar(&ch, &saiu("", None)).resumo, "interrompido");
        let r = julgar(&ch, &Execucao { codigo: Some(0), tempo_esgotado: true, ..Default::default() });
        assert_eq!((r.situacao, r.resumo.as_str()), (Situacao::Falhou, "tempo esgotado"));
    }

    #[test]
    fn codigo_que_nao_rodou_conta_cada_caso_ou_uma_falha() {
        let r = ResultadoChecagem::nao_rodou(&casos(&[caso("1", "1"), caso("2", "2")]), "sem .py");
        assert_eq!((r.situacao, r.passaram, r.total, r.resumo.as_str()), (Situacao::Falhou, 0, 2, "não rodou: sem .py"));
        let r = ResultadoChecagem::nao_rodou(&checagem("pytest"), "sem .py");
        assert_eq!((r.situacao, r.total), (Situacao::Falhou, 1));
        let r = ResultadoChecagem::erro(&checagem("estilo"), "podman: sem imagem\ndetalhes");
        assert_eq!((r.situacao, r.total, r.resumo.as_str()), (Situacao::Erro, 0, "podman: sem imagem"));
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
