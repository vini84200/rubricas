//! Pipeline de checagens de uma avaliação (`dados/pipelines/<avaliacao>/pipeline.toml`)
//! e o relatório que o executor devolve por aluno. O servidor nunca executa
//! código de aluno: ele só guarda o pipeline, entrega o trabalho ao executor e
//! transforma os resultados em sugestões de marcação.
//!
//! ```toml
//! arquivo = "main.py"              # nome que o arquivo do aluno recebe
//! tempo = 10                       # segundos por execução (padrão 10)
//! plagio = { base = "esqueleto.py", referencias = "referencias/", limiar = 0.6 }
//!
//! [[checagens]]
//! tipo = "casos"
//! nome = "Exemplos do enunciado"
//! criterio = "funciona"
//! sugestao = [{ minimo = 1.0, nivel = "atende" }, { minimo = 0.5, nivel = "parcial" }]
//! [[checagens.casos]]
//! entrada = "2\n3\n"
//! saida = "5\n"
//! ```

use crate::modelo::{Avaliacao, Criterio, Escala, Marcacao, slug};
use serde::{Deserialize, Serialize};

fn dez() -> f64 {
    10.0
}
fn um() -> f64 {
    1.0
}
fn imagem_padrao() -> String {
    "python:3.12-slim".into()
}
fn memoria_padrao() -> String {
    "256m".into()
}
fn limiar_padrao() -> f64 {
    0.6
}
fn falso(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    /// Imagem do contêiner onde o código do aluno roda.
    #[serde(default = "imagem_padrao")]
    pub imagem: String,
    /// Segundos por execução (por caso, no tipo `casos`).
    #[serde(default = "dez")]
    pub tempo: f64,
    #[serde(default = "memoria_padrao")]
    pub memoria: String,
    #[serde(default = "um")]
    pub cpus: f64,
    /// Nome que o arquivo do aluno recebe antes de rodar. Vazio: o nome original.
    #[serde(default)]
    pub arquivo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plagio: Option<Plagio>,
    #[serde(default)]
    pub checagens: Vec<Checagem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plagio {
    /// Código fornecido a todos (não conta como plágio): arquivo ou pasta do pipeline.
    #[serde(default)]
    pub base: String,
    /// Pasta do pipeline com soluções de referência (do professor, da internet).
    #[serde(default)]
    pub referencias: String,
    /// Similaridade a partir da qual o cartão de avaliar mostra o alerta.
    #[serde(default = "limiar_padrao")]
    pub limiar: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoChecagem {
    /// stdin → stdout esperado.
    Casos,
    /// Arquivo de testes do professor que importa o arquivo do aluno.
    Pytest,
    /// `ruff check` (e, opcionalmente, `ruff format --check`).
    Estilo,
    /// Qualquer comando: passa se termina com código 0.
    Comando,
}

/// Um item de `[[checagens]]`. Campos que não valem para o tipo são recusados.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checagem {
    pub tipo: TipoChecagem,
    #[serde(default)]
    pub nome: String,
    /// Id do critério que esta checagem sugere marcar.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub criterio: String,
    /// Origem da rubrica, quando o mesmo id de critério existe em mais de uma.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rubrica: String,
    /// Regras de sugestão, da maior fração para a menor. Vazio: a regra padrão da escala.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sugestao: Vec<Regra>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub casos: Vec<Caso>,
    #[serde(default, skip_serializing_if = "falso")]
    pub ignorar_maiusculas: bool,
    /// casos: compara também os prompts do `input()`. Sem isto, eles não entram
    /// na saída, porque cada aluno escreve o seu.
    #[serde(default, skip_serializing_if = "falso")]
    pub considerar_prompts: bool,

    /// pytest: arquivo de testes no pipeline.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub testes: String,

    /// estilo: regras do ruff (`select`); vazio usa as padrão.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regras: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignorar: Vec<String>,
    /// estilo: também exige `ruff format --check`.
    #[serde(default, skip_serializing_if = "falso")]
    pub formato: bool,

    /// comando: linha de comando rodada com `sh -c` na pasta de trabalho.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comando: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imagem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tempo: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memoria: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caso {
    #[serde(default)]
    pub nome: String,
    #[serde(default)]
    pub entrada: String,
    pub saida: String,
    /// Argumentos de linha de comando para o programa.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argumentos: Vec<String>,
}

/// "A partir desta fração de acertos, sugira esta marcação". Só um de `nivel`,
/// `atende` ou `nota`, conforme a escala do critério.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regra {
    pub minimo: f64,
    /// Id ou rótulo do nível (escala `niveis`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nivel: Option<String>,
    /// Escala `binaria`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atende: Option<bool>,
    /// Escalas `livre` e `ponto_unico`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nota: Option<f64>,
}

// ---------- Relatório do executor ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Situacao {
    Passou,
    /// O código do aluno não atendeu (inclusive se não rodou: sintaxe, exceção, tempo).
    Falhou,
    /// Falha do próprio executor (imagem ausente, podman fora do ar): não reprova
    /// e o aluno continua pendente.
    Erro,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultadoCaso {
    pub nome: String,
    pub passou: bool,
    #[serde(default)]
    pub entrada: String,
    #[serde(default)]
    pub esperado: String,
    #[serde(default)]
    pub obtido: String,
    /// "saída diferente", "tempo esgotado", "terminou com erro (código 1)"…
    #[serde(default)]
    pub motivo: String,
    /// Fim do stderr.
    #[serde(default)]
    pub erro: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultadoChecagem {
    pub nome: String,
    pub tipo: TipoChecagem,
    pub situacao: Situacao,
    pub passaram: u32,
    pub total: u32,
    /// Uma linha: "7 de 8 casos", "3 problemas de estilo".
    #[serde(default)]
    pub resumo: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub casos: Vec<ResultadoCaso>,
    /// Fim da saída (pytest, ruff, comando, erro de execução).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub saida: String,
}

impl ResultadoChecagem {
    /// `passaram` de `total`: passa só com todos. Sem nada contado (nenhum teste
    /// rodou, o arquivo do aluno nem importa), conta como uma falha.
    pub fn contagem(ch: &Checagem, passaram: u32, total: u32, resumo: String, saida: String) -> Self {
        let total = total.max(1);
        let situacao = if passaram == total { Situacao::Passou } else { Situacao::Falhou };
        ResultadoChecagem { nome: ch.nome.clone(), tipo: ch.tipo, situacao, passaram, total, resumo, casos: vec![], saida }
    }

    /// O código do aluno não chegou a rodar: tudo falha. Os casos contam um a um;
    /// as outras checagens, que não têm casos, contam como uma.
    pub fn nao_rodou(ch: &Checagem, motivo: &str) -> Self {
        Self::contagem(ch, 0, ch.casos.len() as u32, format!("não rodou: {motivo}"), String::new())
    }

    /// Falha do próprio executor: não reprova, e o aluno continua pendente.
    pub fn erro(ch: &Checagem, msg: &str) -> Self {
        ResultadoChecagem {
            nome: ch.nome.clone(),
            tipo: ch.tipo,
            situacao: Situacao::Erro,
            passaram: 0,
            total: 0,
            resumo: msg.lines().next().unwrap_or("").chars().take(200).collect(),
            casos: vec![],
            saida: crate::conferir::fim(msg),
        }
    }
}

/// Sugestão de marcação para um critério. Quem decide é sempre o avaliador.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sugestao {
    pub rubrica: String,
    pub criterio: String,
    pub marcacao: Marcacao,
    /// "8/8 casos".
    pub motivo: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// O que o executor manda de um aluno. As sugestões não ficam aqui: saem das
/// checagens e da rubrica atual a cada leitura (`fila::Panorama::sugestoes`).
pub struct Relatorio {
    /// Versão do trabalho (pipeline + submissão) a que o relatório responde.
    pub versao: String,
    /// Versão da regra de julgar do executor que gerou o relatório.
    #[serde(default)]
    pub motor: String,
    #[serde(default)]
    pub gerado_em: u64,
    pub checagens: Vec<ResultadoChecagem>,
}

/// Um caminho relativo dentro da pasta do pipeline ou do trabalho.
pub fn caminho_seguro(caminho: &str) -> bool {
    let c = caminho.trim_end_matches('/');
    !c.is_empty() && c.split('/').all(crate::modelo::id_valido)
}

impl TipoChecagem {
    pub fn nome(self) -> &'static str {
        match self {
            TipoChecagem::Casos => "casos",
            TipoChecagem::Pytest => "pytest",
            TipoChecagem::Estilo => "estilo",
            TipoChecagem::Comando => "comando",
        }
    }
}

/// O critério que uma checagem aponta: (origem da rubrica, critério).
fn resolver<'a>(av: &'a Avaliacao, rubrica: &str, criterio: &str) -> Result<(&'a str, &'a Criterio), String> {
    let achados: Vec<_> = av
        .rubricas
        .iter()
        .filter(|ra| rubrica.is_empty() || ra.origem == rubrica)
        .filter_map(|ra| ra.rubrica.criterio(criterio).map(|c| (ra.origem.as_str(), c)))
        .collect();
    match achados.as_slice() {
        [] => Err(format!("o critério “{criterio}” não existe na avaliação")),
        [(_, c)] if c.roteiro() => Err(format!("“{criterio}” é um passo de roteiro, não recebe marcação")),
        [um] => Ok(*um),
        _ => Err(format!("o critério “{criterio}” existe em mais de uma rubrica: diga qual com rubrica = \"…\"")),
    }
}

/// Confere a regra contra a escala e devolve a marcação que ela sugere.
fn marcacao_da_regra(c: &Criterio, r: &Regra) -> Result<Marcacao, String> {
    let campos = [r.nivel.is_some(), r.atende.is_some(), r.nota.is_some()].iter().filter(|x| **x).count();
    if campos != 1 {
        return Err("cada regra de sugestão tem um, e só um, de nivel, atende ou nota".into());
    }
    let m = match (c.escala.as_ref(), r) {
        (Some(Escala::Niveis { niveis }), Regra { nivel: Some(n), .. }) => {
            let alvo = slug(n);
            let nivel = niveis
                .iter()
                .find(|x| x.id == *n || slug(&x.rotulo) == alvo)
                .ok_or_else(|| format!("o nível “{n}” não existe em “{}”", c.nome))?;
            Marcacao::Nivel { nivel: nivel.id.clone() }
        }
        (Some(Escala::Binaria { .. }), Regra { atende: Some(a), .. }) => Marcacao::Binaria { atende: *a },
        (Some(Escala::Livre { .. }), Regra { nota: Some(n), .. }) => Marcacao::Nota { nota: *n },
        (Some(Escala::PontoUnico { max, .. }), Regra { nota: Some(n), .. }) => ponto_unico(*n, *max),
        _ => return Err(format!("a regra não combina com a escala de “{}”", c.nome)),
    };
    c.pontos(&m)?;
    Ok(m)
}

/// Sem regras no pipeline: binária só atende com tudo certo; níveis só nos
/// extremos; nota proporcional aos acertos, no passo da escala.
fn marcacao_padrao(c: &Criterio, tudo: bool, nada: bool, fracao: f64) -> Option<Marcacao> {
    let arredondar = |v: f64, passo: f64| ((v / passo).round() * passo * 1000.0).round() / 1000.0;
    match c.escala.as_ref()? {
        Escala::Binaria { .. } => Some(Marcacao::Binaria { atende: tudo }),
        Escala::Niveis { niveis } => {
            let por_pontos = |maior: bool| {
                niveis.iter().reduce(|a, b| if (b.pontos > a.pontos) == maior { b } else { a })
            };
            let n = if tudo { por_pontos(true) } else if nada { por_pontos(false) } else { None }?;
            Some(Marcacao::Nivel { nivel: n.id.clone() })
        }
        Escala::Livre { max, passo } => {
            Some(Marcacao::Nota { nota: arredondar(max * fracao, passo.unwrap_or(passo_padrao(*max))).min(*max) })
        }
        Escala::PontoUnico { max, .. } => Some(ponto_unico(arredondar(max * fracao, passo_padrao(*max)).min(*max), *max)),
    }
}

/// O passo da nota quando a escala não diz. Espelha `passoDe` em
/// web/js/nota.js: a sugestão cai num valor que a tela oferece.
fn passo_padrao(max: f64) -> f64 {
    if max <= 2.0 {
        0.1
    } else if max <= 5.0 {
        0.25
    } else {
        0.5
    }
}

fn ponto_unico(nota: f64, max: f64) -> Marcacao {
    use crate::modelo::Desempenho;
    let desempenho = if nota >= max - 1e-9 { Desempenho::Atende } else { Desempenho::Abaixo };
    Marcacao::PontoUnico { desempenho, nota }
}

/// Lê e valida um pipeline contra a avaliação a que ele pertence.
pub fn ler(texto: &str, av: &Avaliacao) -> Result<Pipeline, String> {
    let mut p: Pipeline = toml::from_str(texto).map_err(|e| crate::dados::erro_toml(texto, &e))?;
    if !(p.tempo.is_finite() && p.tempo > 0.0 && p.tempo <= 600.0) {
        return Err("tempo: use de 0 a 600 segundos".into());
    }
    if !(p.cpus.is_finite() && p.cpus > 0.0) {
        return Err("cpus precisa ser maior que zero".into());
    }
    if !p.arquivo.is_empty() && !(crate::modelo::id_valido(&p.arquivo) && !p.arquivo.contains('/')) {
        return Err(format!("arquivo: nome inválido {:?}", p.arquivo));
    }
    if let Some(pl) = &p.plagio {
        if !(pl.limiar > 0.0 && pl.limiar <= 1.0) {
            return Err("plagio.limiar: use uma fração entre 0 e 1 (0.6 = 60%)".into());
        }
        for (campo, valor) in [("base", &pl.base), ("referencias", &pl.referencias)] {
            if !valor.is_empty() && !caminho_seguro(valor) {
                return Err(format!("plagio.{campo}: caminho inválido {valor:?}"));
            }
        }
    }
    for (i, ch) in p.checagens.iter_mut().enumerate() {
        if ch.nome.trim().is_empty() {
            ch.nome = ch.tipo.nome().into();
        }
        let onde = format!("Checagem {} “{}”", i + 1, ch.nome);
        let erro = |m: String| format!("{onde}: {m}");
        let tipo = ch.tipo;
        let sobra = [
            ("casos", !ch.casos.is_empty(), TipoChecagem::Casos),
            ("ignorar_maiusculas", ch.ignorar_maiusculas, TipoChecagem::Casos),
            ("considerar_prompts", ch.considerar_prompts, TipoChecagem::Casos),
            ("testes", !ch.testes.is_empty(), TipoChecagem::Pytest),
            ("regras", !ch.regras.is_empty(), TipoChecagem::Estilo),
            ("ignorar", !ch.ignorar.is_empty(), TipoChecagem::Estilo),
            ("formato", ch.formato, TipoChecagem::Estilo),
            ("comando", !ch.comando.is_empty(), TipoChecagem::Comando),
        ]
        .into_iter()
        .find(|(_, usado, dono)| *usado && *dono != tipo);
        if let Some((campo, _, _)) = sobra {
            return Err(erro(format!("o campo {campo} não vale para o tipo {}", tipo.nome())));
        }
        match tipo {
            TipoChecagem::Casos if ch.casos.is_empty() => return Err(erro("defina pelo menos um [[checagens.casos]]".into())),
            TipoChecagem::Pytest if !caminho_seguro(&ch.testes) => {
                return Err(erro("diga o arquivo de testes, por exemplo testes = \"testes.py\"".into()));
            }
            TipoChecagem::Comando if ch.comando.trim().is_empty() => return Err(erro("falta o comando".into())),
            _ => {}
        }
        for (n, caso) in ch.casos.iter_mut().enumerate() {
            if caso.nome.trim().is_empty() {
                caso.nome = format!("caso {}", n + 1);
            }
        }
        if let Some(t) = ch.tempo
            && !(t.is_finite() && t > 0.0 && t <= 600.0)
        {
            return Err(erro("tempo: use de 0 a 600 segundos".into()));
        }
        if ch.criterio.is_empty() {
            if !ch.sugestao.is_empty() || !ch.rubrica.is_empty() {
                return Err(erro("regras de sugestão precisam de criterio = \"…\"".into()));
            }
            continue;
        }
        let (_, c) = resolver(av, &ch.rubrica, &ch.criterio).map_err(erro)?;
        for r in &mut ch.sugestao {
            if !(0.0..=1.0).contains(&r.minimo) {
                return Err(erro("minimo é uma fração de acertos, de 0 a 1".into()));
            }
            // Grava o id do nível, mesmo que a regra use o rótulo.
            if let Marcacao::Nivel { nivel } = marcacao_da_regra(c, r).map_err(erro)? {
                r.nivel = Some(nivel);
            }
        }
        ch.sugestao.sort_by(|a, b| b.minimo.total_cmp(&a.minimo));
    }
    Ok(p)
}

/// (origem, critério, primeira checagem que o aponta) e os resultados que somam nele.
type Grupo<'a> = ((&'a str, &'a Criterio, &'a Checagem), Vec<&'a ResultadoChecagem>);

impl Pipeline {
    /// Sugestões a partir dos resultados, na ordem das checagens.
    /// Checagens que apontam o mesmo critério somam seus acertos.
    pub fn sugestoes(&self, av: &Avaliacao, resultados: &[ResultadoChecagem]) -> Vec<Sugestao> {
        let mut grupos: Vec<Grupo> = Vec::new();
        for (ch, r) in self.checagens.iter().zip(resultados) {
            if ch.criterio.is_empty() {
                continue;
            }
            let Ok((origem, c)) = resolver(av, &ch.rubrica, &ch.criterio) else { continue };
            match grupos.iter_mut().find(|((o, x, _), _)| *o == origem && x.id == c.id) {
                Some((_, v)) => v.push(r),
                None => grupos.push(((origem, c, ch), vec![r])),
            }
        }
        let mut saida = Vec::new();
        for ((origem, c, ch), rs) in grupos {
            if rs.iter().any(|r| r.situacao == Situacao::Erro) {
                continue;
            }
            let (passaram, total) = rs.iter().fold((0, 0), |(p, t), r| (p + r.passaram, t + r.total));
            if total == 0 {
                continue;
            }
            let fracao = f64::from(passaram) / f64::from(total);
            let marcacao = if ch.sugestao.is_empty() {
                marcacao_padrao(c, passaram == total, passaram == 0, fracao)
            } else {
                ch.sugestao.iter().find(|r| fracao >= r.minimo - 1e-9).and_then(|r| marcacao_da_regra(c, r).ok())
            };
            let Some(marcacao) = marcacao else { continue };
            let motivo = match rs.as_slice() {
                [r] if matches!(r.tipo, TipoChecagem::Estilo | TipoChecagem::Comando) && !r.resumo.is_empty() => r.resumo.clone(),
                _ => {
                    let unidade = match rs[0].tipo {
                        TipoChecagem::Casos if rs.iter().all(|r| r.tipo == TipoChecagem::Casos) => "casos",
                        TipoChecagem::Pytest if rs.iter().all(|r| r.tipo == TipoChecagem::Pytest) => "testes",
                        _ => "checagens",
                    };
                    format!("{passaram}/{total} {unidade}")
                }
            };
            saida.push(Sugestao { rubrica: origem.into(), criterio: c.id.clone(), marcacao, motivo });
        }
        saida
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::{Rubrica, RubricaAplicada};

    fn avaliacao() -> Avaliacao {
        let mut r: Rubrica = toml::from_str(
            r#"
            titulo = "Lista 5"
            [[criterios]]
            nome = "Funciona"
            [criterios.escala]
            tipo = "niveis"
            [[criterios.escala.niveis]]
            rotulo = "Atende"
            pontos = 2.0
            [[criterios.escala.niveis]]
            rotulo = "Parcial"
            pontos = 1.0
            [[criterios.escala.niveis]]
            rotulo = "Não atende"
            pontos = 0.0
            [[criterios]]
            nome = "Estilo"
            escala = { tipo = "binaria", pontos = 1.0 }
            [[criterios]]
            nome = "Testes"
            escala = { tipo = "livre", max = 2.0, passo = 0.5 }
            "#,
        )
        .unwrap();
        r.normalizar();
        Avaliacao {
            id: "l5".into(),
            titulo: "L5".into(),
            turma: "d".into(),
            nota_maxima: 10.0,
            rubricas: vec![RubricaAplicada { origem: "lista5".into(), peso: 1.0, rubrica: r }],
        }
    }

    const PIPELINE: &str = r#"
arquivo = "main.py"

[[checagens]]
tipo = "casos"
nome = "Exemplos"
criterio = "funciona"
sugestao = [{ minimo = 1.0, nivel = "atende" }, { minimo = 0.5, nivel = "Parcial" }]
[[checagens.casos]]
entrada = "2\n3\n"
saida = "5\n"

[[checagens]]
tipo = "estilo"
criterio = "estilo"

[[checagens]]
tipo = "pytest"
testes = "testes.py"
criterio = "testes"
"#;

    #[test]
    fn le_um_pipeline_e_recusa_o_que_nao_combina_com_a_avaliacao() {
        let av = avaliacao();
        let p = ler(PIPELINE, &av).unwrap();
        assert_eq!(p.checagens.len(), 3);
        assert_eq!((p.tempo, p.imagem.as_str()), (10.0, "python:3.12-slim"));
        assert_eq!(p.checagens[0].nome, "Exemplos");
        assert_eq!(p.checagens[1].nome, "estilo", "sem nome, a checagem leva o nome do tipo");

        let erro = |de: &str, para: &str| ler(&PIPELINE.replace(de, para), &av).unwrap_err();
        assert!(erro("criterio = \"funciona\"", "criterio = \"sumiu\"").contains("sumiu"));
        assert!(erro("nivel = \"Parcial\"", "nivel = \"Meio\"").contains("Meio"));
        assert!(erro("nivel = \"Parcial\"", "atende = true").contains("Exemplos"), "regra de outra escala");
        assert!(erro("tipo = \"estilo\"", "tipo = \"estilo\"\nignorar_maiuscula = true").contains("linha"), "campo com erro de digitação");
        assert!(erro("testes = \"testes.py\"\n", "").contains("testes"), "pytest sem arquivo de testes");
        assert!(erro("[[checagens.casos]]\nentrada = \"2\\n3\\n\"\nsaida = \"5\\n\"\n", "").contains("Exemplos"), "casos sem casos");
        assert!(erro("arquivo = \"main.py\"", "arquivo = \"../x.py\"").contains("arquivo"));
        // prompts do input() são ignorados nos casos, a não ser que a checagem peça
        assert!(!p.checagens[0].considerar_prompts);
        let com = ler(&PIPELINE.replace("nome = \"Exemplos\"", "nome = \"Exemplos\"\nconsiderar_prompts = true"), &av).unwrap();
        assert!(com.checagens[0].considerar_prompts);
        assert!(erro("tipo = \"estilo\"", "tipo = \"estilo\"\nconsiderar_prompts = true").contains("considerar_prompts"));
        assert!(erro("arquivo = \"main.py\"", "plagio = { limiar = 2.0 }").contains("limiar"));
    }

    fn resultado(tipo: TipoChecagem, passaram: u32, total: u32, resumo: &str) -> ResultadoChecagem {
        let situacao = if passaram == total { Situacao::Passou } else { Situacao::Falhou };
        ResultadoChecagem { nome: String::new(), tipo, situacao, passaram, total, resumo: resumo.into(), casos: vec![], saida: String::new() }
    }

    #[test]
    fn sugere_a_marcacao_pela_fracao_de_acertos() {
        let av = avaliacao();
        let p = ler(PIPELINE, &av).unwrap();
        let sugere = |casos: u32, estilo: u32, testes: u32| {
            p.sugestoes(
                &av,
                &[
                    resultado(TipoChecagem::Casos, casos, 8, ""),
                    resultado(TipoChecagem::Estilo, estilo, 1, "3 problemas de estilo"),
                    resultado(TipoChecagem::Pytest, testes, 4, ""),
                ],
            )
            .into_iter()
            .map(|s| (s.criterio, s.marcacao, s.motivo))
            .collect::<Vec<_>>()
        };
        let nivel = |n: &str| Marcacao::Nivel { nivel: n.into() };
        assert_eq!(
            sugere(8, 1, 4),
            [
                ("funciona".into(), nivel("atende"), "8/8 casos".into()),
                ("estilo".into(), Marcacao::Binaria { atende: true }, "3 problemas de estilo".into()),
                ("testes".into(), Marcacao::Nota { nota: 2.0 }, "4/4 testes".into()),
            ]
        );
        // regras do pipeline: ≥50% → Parcial; abaixo disso, nenhuma sugestão
        assert_eq!(sugere(5, 0, 3)[0], ("funciona".into(), nivel("parcial"), "5/8 casos".into()));
        // sem regras: binária só atende com tudo certo; nota livre proporcional, no passo da escala
        assert_eq!(sugere(5, 0, 3)[1].1, Marcacao::Binaria { atende: false });
        assert_eq!(sugere(5, 0, 3)[2].1, Marcacao::Nota { nota: 1.5 });
        assert_eq!(sugere(2, 0, 3).iter().filter(|s| s.0 == "funciona").count(), 0);

        // erro de execução não vira sugestão
        let mut erro = resultado(TipoChecagem::Casos, 0, 8, "");
        erro.situacao = Situacao::Erro;
        let s = p.sugestoes(&av, &[erro, resultado(TipoChecagem::Estilo, 1, 1, "ok"), resultado(TipoChecagem::Pytest, 0, 0, "")]);
        assert_eq!(s.iter().map(|s| s.criterio.as_str()).collect::<Vec<_>>(), ["estilo"], "pytest sem testes também não sugere");
    }

    #[test]
    fn nota_sugerida_sem_passo_usa_o_passo_padrao_da_tela() {
        // web/js/nota.js (passoDe): até 2 → 0,1; até 5 → 0,25; acima → 0,5
        let mut av = avaliacao();
        av.rubricas[0].rubrica.criterios[2].escala = Some(Escala::Livre { max: 10.0, passo: None });
        let p = ler(&PIPELINE.replace("criterio = \"funciona\"\nsugestao = [{ minimo = 1.0, nivel = \"atende\" }, { minimo = 0.5, nivel = \"Parcial\" }]", ""), &av).unwrap();
        let s = p.sugestoes(&av, &[resultado(TipoChecagem::Casos, 0, 1, ""), resultado(TipoChecagem::Estilo, 1, 1, ""), resultado(TipoChecagem::Pytest, 2, 3, "")]);
        assert_eq!(s.last().unwrap().marcacao, Marcacao::Nota { nota: 6.5 }, "10 × 2/3 = 6,67 → 6,5");
    }
}
