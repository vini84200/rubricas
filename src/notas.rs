//! As notas para fora: o CSV de exportação e o formato dos números, o mesmo da
//! tela e da planilha do Moodle.

use crate::modelo::*;
use std::collections::BTreeMap;

pub fn numero(v: f64, br: bool) -> String {
    let s = format!("{:.2}", arredondar(v));
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    let s = if s == "-0" { "0".to_string() } else { s };
    if br { s.replace('.', ",") } else { s }
}

/// Duas casas, meio para cima. A folga cobre empates que o ponto flutuante deixa
/// um fio abaixo (10 × 2,05 ÷ 20 dá 1,0249999…, que é 1,03). `web/js/util.js` faz
/// a mesma conta, para a tela e o CSV nunca divergirem.
pub fn arredondar(v: f64) -> f64 {
    v.signum() * (v.abs() * 100.0 + 0.5 + 1e-7).floor() / 100.0
}

/// Planilhas executam células que começam com `=`, `+`, `-` ou `@`; o apóstrofo
/// na frente faz o texto ser tratado como texto.
fn texto_csv(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() }
}

pub fn notas_csv(av: &Avaliacao, turma: &Turma, correcoes: &BTreeMap<String, Correcao>, br: bool) -> String {
    let mut w = csv::WriterBuilder::new().delimiter(if br { b';' } else { b',' }).from_writer(Vec::new());
    let varias = av.rubricas.len() > 1;
    let mut cab = vec!["matricula".to_string(), "nome".to_string()];
    for ra in &av.rubricas {
        for c in ra.rubrica.avaliados() {
            cab.push(texto_csv(&if varias { format!("{}: {}", ra.rubrica.titulo, c.nome) } else { c.nome.clone() }));
        }
        if varias {
            cab.push(texto_csv(&format!("{} (total)", ra.rubrica.titulo)));
        }
    }
    cab.extend(["nota".to_string(), "situacao".to_string(), "comentarios".to_string()]);
    w.write_record(&cab).unwrap();

    for aluno in &turma.alunos {
        let correcao = correcoes.get(&aluno.id);
        let r = calcular(av, correcao);
        let mut linha = vec![texto_csv(&aluno.id), texto_csv(&aluno.nome)];
        let mut comentarios = Vec::new();
        for (ra, parcial) in av.rubricas.iter().zip(&r.rubricas) {
            let respostas = correcao.and_then(|c| c.respostas.get(&ra.origem));
            for c in ra.rubrica.avaliados() {
                let resposta = respostas.and_then(|m| m.get(&c.id));
                let pontos = resposta.and_then(|x| c.pontos(&x.marcacao).ok());
                linha.push(pontos.map(|p| numero(p * c.peso, br)).unwrap_or_default());
                if let Some(x) = resposta.filter(|x| !x.comentario.trim().is_empty()) {
                    comentarios.push(format!("{}: {}", c.nome, x.comentario.trim()));
                }
            }
            if varias {
                linha.push(numero(parcial.pontos, br));
            }
        }
        if let Some(c) = correcao.filter(|c| !c.comentario_geral.trim().is_empty()) {
            comentarios.push(c.comentario_geral.trim().to_string());
        }
        linha.push(if r.respondidos > 0 { numero(r.nota, br) } else { String::new() });
        linha.push(match r.andamento() {
            Andamento::Concluida => "concluída".to_string(),
            Andamento::Parcial => format!("{} de {}", r.respondidos, r.total),
            Andamento::NaoAvaliado => "não avaliado".to_string(),
        });
        linha.push(texto_csv(&comentarios.join(" | ")));
        w.write_record(&linha).unwrap();
    }
    String::from_utf8(w.into_inner().unwrap()).unwrap()
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Os mesmos casos de `testes/nota.test.mjs`: a tela e o CSV arredondam igual.
    #[test]
    fn arredonda_como_a_tela() {
        for (v, esperado) in [(1.005, 1.01), (1.025, 1.03), (2.675, 2.68), (-1.005, -1.01), (7.123, 7.12), (-0.001, 0.0)] {
            assert_eq!(arredondar(v), esperado, "{v}");
        }
    }

    #[test]
    fn numeros_no_csv() {
        assert_eq!(numero(7.5, false), "7.5");
        assert_eq!(numero(7.5, true), "7,5");
        assert_eq!(numero(10.0, true), "10");
        assert_eq!(numero(2.0 / 3.0, false), "0.67");
        assert_eq!(numero(0.0, false), "0");
        // empates arredondam para cima, como na tela
        assert_eq!(numero(3.125, false), "3.13");
        assert_eq!(numero(0.625, true), "0,63");
        assert_eq!(numero(10.0 * 2.05 / 20.0, false), "1.03");
        assert_eq!(numero(10.0 * 4.35 / 20.0, false), "2.18");
        assert_eq!(numero(10.0 * 2.55 / 20.0, false), "1.28");
        assert_eq!(numero(10.0 * 10.25 / 100.0, false), "1.03");
        assert_eq!(numero(1.0249, false), "1.02");
        assert_eq!(numero(-0.004, false), "0");
        assert_eq!(texto_csv("=1+1"), "'=1+1");
        assert_eq!(texto_csv("Ana"), "Ana");
    }

    #[test]
    fn csv_nao_tem_coluna_para_passo_de_roteiro() {
        let mut r: Rubrica = toml::from_str(
            r#"
            titulo = "Prática"
            [[criterios]]
            tipo = "roteiro"
            nome = "Rode com entrada vazia"
            [[criterios]]
            nome = "Trata entrada vazia"
            escala = { tipo = "binaria", pontos = 1.0 }
            "#,
        )
        .unwrap();
        r.normalizar();
        let av = Avaliacao {
            id: "p".into(),
            titulo: "P".into(),
            turma: "d".into(),
            nota_maxima: 10.0,
            rubricas: vec![RubricaAplicada { origem: "p".into(), peso: 1.0, rubrica: r }],
        };
        let turma = Turma { id: "d".into(), alunos: vec![Aluno { id: "1".into(), nome: "Ana".into() }] };
        let csv = notas_csv(&av, &turma, &Default::default(), false);
        assert_eq!(csv.lines().next().unwrap(), "matricula,nome,Trata entrada vazia,nota,situacao,comentarios");
        assert_eq!(csv.lines().nth(1).unwrap(), "1,Ana,,,não avaliado,");
    }
}
