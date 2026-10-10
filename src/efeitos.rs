//! Efeitos sobre a nota final: zero por plágio, desconto por atraso, bônus.
//!
//! ```text
//! dados/efeitos/<id>.toml                    um tipo de efeito por arquivo (o catálogo)
//! dados/submissoes/<avaliacao>/.envios.json  data do envio de cada aluno, da planilha do Moodle
//! ```
//!
//! A avaliação guarda uma cópia dos tipos que usa, como faz com as rubricas. A
//! conta é a mesma em `web/js/nota.js`: mude as duas juntas.

use crate::modelo::slug;
use crate::notas::numero;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn sim() -> bool {
    true
}
fn e_sim(v: &bool) -> bool {
    *v
}
fn nao(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TipoEfeito {
    /// Nome do arquivo sem extensão; não é gravado dentro do arquivo.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub titulo: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub descricao: String,
    pub operacao: Operacao,
    /// O sinal que faz o efeito aparecer como sugestão para um aluno.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sugerir: Option<Sinal>,
    /// Vai aos comentários de feedback do Moodle.
    #[serde(default = "sim", skip_serializing_if = "e_sim")]
    pub feedback: bool,
    /// Aplicado, conta a correção como concluída sem marcar os critérios.
    #[serde(default, skip_serializing_if = "nao")]
    pub encerra: bool,
}

/// O que o efeito faz com a nota. A ordem na conta é fixa, não a do arquivo:
/// multiplicar, depois somar e por unidade, depois o teto; `fixar` vence tudo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Operacao {
    Fixar {
        nota: Valor,
    },
    Multiplicar {
        fator: Valor,
    },
    /// Pontos negativos subtraem.
    Somar {
        pontos: Valor,
    },
    /// `pontos` × quantidade (dias de atraso, por exemplo), sem passar de `limite`.
    PorUnidade {
        pontos: Valor,
        #[serde(default, skip_serializing_if = "Unidade::e_dia")]
        unidade: Unidade,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limite: Option<f64>,
    },
    Teto {
        nota: Valor,
    },
}

/// Um número, ou `"na_hora"`: quem aplica digita.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Valor {
    Fixo(f64),
    Aberto(NaHora),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NaHora {
    #[serde(rename = "na_hora")]
    NaHora,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unidade {
    #[default]
    Dia,
    Hora,
}

impl Unidade {
    fn e_dia(&self) -> bool {
        *self == Unidade::Dia
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sinal {
    Plagio,
    Atraso,
}

/// Um efeito aplicado a um aluno, na correção dele.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EfeitoAplicado {
    /// O número digitado, quando o tipo tem `"na_hora"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valor: Option<f64>,
    /// Quantas unidades, em `por_unidade`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantidade: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub justificativa: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub por: String,
    #[serde(default)]
    pub em: u64,
}

/// Uma sugestão que o avaliador dispensou. Ela volta só se o sinal piorar:
/// similaridade maior ou mais unidades de atraso do que `sinal`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Dispensa {
    pub sinal: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub por: String,
    #[serde(default)]
    pub em: u64,
}

fn finito(onde: &str, v: f64) -> Result<(), String> {
    if v.is_finite() { Ok(()) } else { Err(format!("{onde} precisa ser um número")) }
}

impl Valor {
    fn fixo(self) -> Option<f64> {
        match self {
            Valor::Fixo(v) => Some(v),
            Valor::Aberto(_) => None,
        }
    }
}

impl Operacao {
    fn valor(&self) -> Valor {
        match self {
            Operacao::Fixar { nota } | Operacao::Teto { nota } => *nota,
            Operacao::Multiplicar { fator } => *fator,
            Operacao::Somar { pontos } | Operacao::PorUnidade { pontos, .. } => *pontos,
        }
    }

    /// O valor pode ser `v`? Fixar, teto e fator não aceitam negativos.
    fn aceita(&self, v: f64) -> Result<(), String> {
        finito("O valor", v)?;
        match self {
            Operacao::Fixar { .. } | Operacao::Teto { .. } if v < 0.0 => Err("A nota não pode ser negativa".into()),
            Operacao::Multiplicar { .. } if v < 0.0 => Err("O fator não pode ser negativo".into()),
            _ => Ok(()),
        }
    }
}

impl TipoEfeito {
    pub fn validar(&self) -> Result<(), String> {
        if self.titulo.trim().is_empty() {
            return Err("O tipo de efeito precisa de um título".into());
        }
        let op = &self.operacao;
        if let Some(v) = op.valor().fixo() {
            op.aceita(v).map_err(|e| format!("operacao: {e}"))?;
        }
        if let Operacao::PorUnidade { pontos, limite: Some(l), .. } = op {
            finito("operacao.limite", *l)?;
            if pontos.fixo().is_some_and(|p| p * l < 0.0) {
                return Err("operacao.limite: use o mesmo sinal dos pontos (pontos = -1.0, limite = -5.0)".into());
            }
        }
        Ok(())
    }

    pub fn aberto(&self) -> bool {
        matches!(self.operacao.valor(), Valor::Aberto(_))
    }

    pub fn por_unidade(&self) -> bool {
        matches!(self.operacao, Operacao::PorUnidade { .. })
    }

    /// O que um efeito aplicado precisa trazer para valer neste tipo.
    pub fn conferir(&self, a: &EfeitoAplicado) -> Result<(), String> {
        let onde = format!("Efeito “{}”", self.titulo);
        if self.aberto() {
            let v = a.valor.ok_or(format!("{onde}: falta o valor"))?;
            self.operacao.aceita(v).map_err(|e| format!("{onde}: {e}"))?;
            if a.justificativa.trim().is_empty() {
                return Err(format!("{onde}: escreva a justificativa"));
            }
        }
        if self.por_unidade() {
            let q = a.quantidade.ok_or(format!("{onde}: falta a quantidade"))?;
            if !q.is_finite() || q < 0.0 {
                return Err(format!("{onde}: a quantidade precisa ser zero ou mais"));
            }
        }
        Ok(())
    }

    fn valor_de(&self, a: &EfeitoAplicado) -> Option<f64> {
        self.operacao.valor().fixo().or(a.valor)
    }

    /// Uma linha para o aluno ler: "Desconto por atraso (2 dias): −2".
    pub fn texto(&self, a: &EfeitoAplicado) -> String {
        let v = self.valor_de(a).unwrap_or(0.0);
        let sinal = |x: f64| if x < 0.0 { format!("−{}", numero(-x, true)) } else { format!("+{}", numero(x, true)) };
        let base = match &self.operacao {
            Operacao::Fixar { .. } => format!("{}: nota {}", self.titulo, numero(v, true)),
            Operacao::Multiplicar { .. } => format!("{}: {}%", self.titulo, sinal((v - 1.0) * 100.0)),
            Operacao::Somar { .. } => format!("{}: {}", self.titulo, sinal(v)),
            Operacao::PorUnidade { unidade, .. } => {
                let q = a.quantidade.unwrap_or(0.0);
                let nome = match (unidade, q == 1.0) {
                    (Unidade::Dia, true) => "dia",
                    (Unidade::Dia, false) => "dias",
                    (Unidade::Hora, true) => "hora",
                    (Unidade::Hora, false) => "horas",
                };
                format!("{} ({} {nome}): {}", self.titulo, numero(q, true), sinal(self.por_unidade_total(v, q)))
            }
            Operacao::Teto { .. } => format!("{}: nota até {}", self.titulo, numero(v, true)),
        };
        match a.justificativa.trim() {
            "" => base,
            j => format!("{base}. {j}"),
        }
    }

    fn por_unidade_total(&self, pontos: f64, quantidade: f64) -> f64 {
        let total = pontos * quantidade;
        match self.operacao {
            Operacao::PorUnidade { limite: Some(l), .. } if pontos < 0.0 => total.max(l),
            Operacao::PorUnidade { limite: Some(l), .. } => total.min(l),
            _ => total,
        }
    }
}

/// O resultado dos efeitos sobre a nota das rubricas.
#[derive(Debug, Clone, PartialEq)]
pub struct Aplicados {
    pub nota: f64,
    /// Algum efeito aplicado tem `encerra`.
    pub encerrada: bool,
    /// Ids dos tipos que entraram na conta, na ordem da avaliação.
    pub ids: Vec<String>,
}

/// Aplica os efeitos de uma correção. Os que não combinam com o tipo (sobras de
/// um tipo atualizado) ficam de fora, como as marcações que não cabem na escala.
pub fn aplicar(tipos: &[TipoEfeito], aplicados: &BTreeMap<String, EfeitoAplicado>, base: f64, maxima: f64) -> Aplicados {
    let (mut fator, mut soma, mut teto, mut fixa) = (1.0, 0.0, f64::INFINITY, None::<f64>);
    let mut r = Aplicados { nota: base, encerrada: false, ids: Vec::new() };
    for t in tipos {
        let Some(a) = aplicados.get(&t.id).filter(|a| t.conferir(a).is_ok()) else { continue };
        let Some(v) = t.valor_de(a) else { continue };
        match t.operacao {
            Operacao::Multiplicar { .. } => fator *= v,
            Operacao::Somar { .. } => soma += v,
            Operacao::PorUnidade { .. } => soma += t.por_unidade_total(v, a.quantidade.unwrap_or(0.0)),
            Operacao::Teto { .. } => teto = teto.min(v),
            Operacao::Fixar { .. } => fixa = Some(fixa.map_or(v, |f: f64| f.min(v))),
        }
        r.encerrada |= t.encerra;
        r.ids.push(t.id.clone());
    }
    if r.ids.is_empty() {
        return r;
    }
    let nota = fixa.unwrap_or_else(|| (base * fator + soma).min(teto));
    r.nota = nota.clamp(0.0, maxima);
    r
}

// ---------- Datas: prazo da avaliação e envio no Moodle ----------

/// Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
fn dias_civis(ano: i64, mes: i64, dia: i64) -> i64 {
    let a = if mes <= 2 { ano - 1 } else { ano };
    let era = a.div_euclid(400);
    let ano_da_era = a - era * 400;
    let m = if mes > 2 { mes - 3 } else { mes + 9 };
    let dia_do_ano = (153 * m + 2) / 5 + dia - 1;
    let dia_da_era = ano_da_era * 365 + ano_da_era / 4 - ano_da_era / 100 + dia_do_ano;
    era * 146097 + dia_da_era - 719468
}

fn dias_no_mes(ano: i64, mes: i64) -> i64 {
    match mes {
        2 if (ano % 4 == 0 && ano % 100 != 0) || ano % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn data(ano: i64, mes: i64, dia: i64, hora: i64, minuto: i64) -> Option<String> {
    let ok = (1970..=9999).contains(&ano)
        && (1..=12).contains(&mes)
        && (1..=dias_no_mes(ano, mes)).contains(&dia)
        && (0..24).contains(&hora)
        && (0..60).contains(&minuto);
    ok.then(|| format!("{ano:04}-{mes:02}-{dia:02}T{hora:02}:{minuto:02}"))
}

/// `"AAAA-MM-DDTHH:MM"` → minutos desde 1970. Sem fuso: o prazo e as datas do
/// Moodle são do mesmo relógio, então só a diferença entre eles importa.
pub fn minutos(s: &str) -> Option<i64> {
    let (d, h) = s.split_once('T')?;
    let partes: Vec<i64> = d.split('-').chain(h.split(':')).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [ano, mes, dia, hora, minuto] = partes[..] else { return None };
    data(ano, mes, dia, hora, minuto)?;
    Some(dias_civis(ano, mes, dia) * 1440 + hora * 60 + minuto)
}

const MESES: [[&str; 2]; 12] = [
    ["jan", "jan"],
    ["fev", "feb"],
    ["mar", "mar"],
    ["abr", "apr"],
    ["mai", "may"],
    ["jun", "jun"],
    ["jul", "jul"],
    ["ago", "aug"],
    ["set", "sep"],
    ["out", "oct"],
    ["nov", "nov"],
    ["dez", "dec"],
];

/// Uma data como o Moodle escreve na planilha de notas ("segunda-feira, 6 out.
/// 2026, 10:00", "Monday, 6 October 2026, 10:00 AM", "06/10/2026 10:00") em
/// `"AAAA-MM-DDTHH:MM"`. `None` se não dá para ler.
pub fn ler_data_moodle(v: &str) -> Option<String> {
    let limpo: String = v.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let palavras: Vec<String> = limpo.split_whitespace().map(slug).collect();
    let mes_escrito = palavras.iter().find_map(|p| {
        let p3 = p.get(..3)?;
        let e_palavra = p.chars().all(|c| c.is_ascii_alphabetic());
        MESES.iter().position(|m| e_palavra && m.contains(&p3)).map(|i| i as i64 + 1)
    });
    let pm = palavras.iter().any(|p| p == "pm");
    let am = palavras.iter().any(|p| p == "am");
    let numeros: Vec<(i64, usize)> = palavras.iter().filter_map(|p| Some((p.parse().ok()?, p.len()))).collect();
    let pos_ano = numeros.iter().position(|(_, n)| *n == 4)?;
    let ano = numeros[pos_ano].0;
    let (mes, dia, hora) = match (mes_escrito, pos_ano) {
        (Some(m), _) => (m, numeros.first().filter(|_| pos_ano > 0)?.0, pos_ano + 1),
        (None, 2) => (numeros[1].0, numeros[0].0, 3),
        (None, 0) => (numeros.get(1)?.0, numeros.get(2)?.0, 3),
        _ => return None,
    };
    let (mut h, mi) = (numeros.get(hora)?.0, numeros.get(hora + 1)?.0);
    if pm && h < 12 {
        h += 12;
    } else if am && h == 12 {
        h = 0;
    }
    data(ano, mes, dia, h, mi)
}

#[cfg(test)]
mod testes {
    use super::*;

    fn tipo(toml: &str) -> TipoEfeito {
        let mut t: TipoEfeito = toml::from_str(toml).unwrap();
        t.validar().unwrap();
        t.id = slug(&t.titulo);
        t
    }

    fn atraso() -> TipoEfeito {
        tipo("titulo = \"Atraso\"\nsugerir = \"atraso\"\noperacao = { tipo = \"por_unidade\", pontos = -1, limite = -3.0 }")
    }
    fn plagio() -> TipoEfeito {
        tipo("titulo = \"Plágio\"\nencerra = true\nfeedback = false\noperacao = { tipo = \"fixar\", nota = 0 }")
    }
    fn bonus() -> TipoEfeito {
        tipo("titulo = \"Bônus\"\noperacao = { tipo = \"somar\", pontos = \"na_hora\" }")
    }
    fn metade() -> TipoEfeito {
        tipo("titulo = \"Metade\"\noperacao = { tipo = \"multiplicar\", fator = 0.5 }")
    }

    fn com(pares: &[(&str, EfeitoAplicado)]) -> BTreeMap<String, EfeitoAplicado> {
        pares.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }
    fn dias(q: f64) -> EfeitoAplicado {
        EfeitoAplicado { quantidade: Some(q), ..Default::default() }
    }
    fn digitado(v: f64) -> EfeitoAplicado {
        EfeitoAplicado { valor: Some(v), justificativa: "Apresentou em aula".into(), ..Default::default() }
    }

    #[test]
    fn a_ordem_da_conta_e_fixa_e_a_nota_fica_entre_zero_e_a_maxima() {
        let tipos = [bonus(), atraso(), metade(), plagio()];
        let nada = aplicar(&tipos, &BTreeMap::new(), 8.0, 10.0);
        assert_eq!((nada.nota, nada.encerrada, nada.ids.len()), (8.0, false, 0));

        // multiplica antes de somar, seja qual for a ordem da lista: 8 × 0,5 + 1 − 2
        let r = aplicar(&tipos, &com(&[("bonus", digitado(1.0)), ("atraso", dias(2.0)), ("metade", EfeitoAplicado::default())]), 8.0, 10.0);
        assert_eq!(r.nota, 3.0);
        assert_eq!(r.ids, ["bonus", "atraso", "metade"]);

        // o limite segura o desconto; a nota não fica negativa nem passa da máxima
        assert_eq!(aplicar(&tipos, &com(&[("atraso", dias(9.0))]), 8.0, 10.0).nota, 5.0);
        assert_eq!(aplicar(&tipos, &com(&[("atraso", dias(9.0))]), 2.0, 10.0).nota, 0.0);
        assert_eq!(aplicar(&tipos, &com(&[("bonus", digitado(3.0))]), 9.0, 10.0).nota, 10.0);

        // fixar vence tudo e encerra a correção
        let r = aplicar(&tipos, &com(&[("bonus", digitado(3.0)), ("plagio", EfeitoAplicado::default())]), 9.0, 10.0);
        assert_eq!((r.nota, r.encerrada), (0.0, true));
    }

    #[test]
    fn efeito_que_nao_combina_com_o_tipo_fica_de_fora() {
        let tipos = [bonus(), atraso()];
        // sem valor, sem justificativa, sem quantidade, ou de um tipo que a avaliação não tem
        let sem_justificativa = EfeitoAplicado { valor: Some(1.0), ..Default::default() };
        let aplicados = com(&[("bonus", sem_justificativa), ("atraso", EfeitoAplicado::default()), ("sumiu", dias(1.0))]);
        assert_eq!(aplicar(&tipos, &aplicados, 7.0, 10.0).nota, 7.0);
        assert!(tipos[0].conferir(&aplicados["bonus"]).unwrap_err().contains("justificativa"));
        assert!(tipos[1].conferir(&aplicados["atraso"]).unwrap_err().contains("quantidade"));
        assert!(tipos[1].conferir(&dias(-1.0)).is_err());
    }

    #[test]
    fn valida_o_tipo() {
        let erro = |t: &str| toml::from_str::<TipoEfeito>(t).map_err(|e| e.to_string()).and_then(|t| t.validar()).unwrap_err();
        assert!(erro("titulo = \" \"\noperacao = { tipo = \"fixar\", nota = 0 }").contains("título"));
        assert!(erro("titulo = \"x\"\noperacao = { tipo = \"fixar\", nota = -1 }").contains("negativa"));
        assert!(erro("titulo = \"x\"\noperacao = { tipo = \"por_unidade\", pontos = -1, limite = 5 }").contains("mesmo sinal"));
        assert!(erro("titulo = \"x\"\noperacao = { tipo = \"somar\", pontos = \"depois\" }").contains("pontos"));
        assert!(erro("titulo = \"x\"\nsugere = \"plagio\"\noperacao = { tipo = \"fixar\", nota = 0 }").contains("sugere"));
    }

    #[test]
    fn tipo_em_toml_vai_e_volta_enxuto() {
        let t = atraso();
        let texto = toml::to_string_pretty(&TipoEfeito { id: String::new(), ..t.clone() }).unwrap();
        assert!(!texto.contains("feedback") && !texto.contains("encerra"), "{texto}");
        assert_eq!(TipoEfeito { id: t.id.clone(), ..toml::from_str(&texto).unwrap() }, t);
        let j = serde_json::to_value(&bonus()).unwrap();
        assert_eq!(j["operacao"], serde_json::json!({"tipo": "somar", "pontos": "na_hora"}));
    }

    #[test]
    fn texto_para_o_aluno() {
        assert_eq!(atraso().texto(&dias(2.0)), "Atraso (2 dias): −2");
        assert_eq!(atraso().texto(&dias(5.0)), "Atraso (5 dias): −3");
        assert_eq!(atraso().texto(&dias(1.0)), "Atraso (1 dia): −1");
        assert_eq!(plagio().texto(&EfeitoAplicado::default()), "Plágio: nota 0");
        assert_eq!(bonus().texto(&digitado(0.5)), "Bônus: +0,5. Apresentou em aula");
        assert_eq!(metade().texto(&EfeitoAplicado::default()), "Metade: −50%");
    }

    #[test]
    fn minutos_entre_datas_sem_fuso() {
        let virada = minutos("2026-10-07T00:00").unwrap() - minutos("2026-10-06T23:59").unwrap();
        assert_eq!(virada, 1);
        assert_eq!(minutos("2027-03-01T00:00").unwrap() - minutos("2027-02-28T00:00").unwrap(), 1440);
        assert_eq!(minutos("1970-01-01T00:00"), Some(0));
        for ruim in ["", "2026-13-01T00:00", "2026-02-29T00:00", "2026-10-06", "2026-10-06T24:00", "2026-10-06T10:00:00"] {
            assert_eq!(minutos(ruim), None, "{ruim}");
        }
    }

    #[test]
    fn le_as_datas_que_o_moodle_escreve() {
        let casos = [
            ("segunda, 6 out 2026, 10:00", "2026-10-06T10:00"),
            ("segunda-feira, 6 outubro 2026, 23:59", "2026-10-06T23:59"),
            ("terça-feira, 7 de out. de 2026, 09:05", "2026-10-07T09:05"),
            ("Monday, 6 October 2026, 10:00 PM", "2026-10-06T22:00"),
            ("Tuesday, 7 October 2026, 12:30 AM", "2026-10-07T00:30"),
            ("sábado, 1 mar 2025, 8:00", "2025-03-01T08:00"),
            ("06/10/2026 10:00", "2026-10-06T10:00"),
            ("2026-10-06 10:00", "2026-10-06T10:00"),
        ];
        for (moodle, esperado) in casos {
            assert_eq!(ler_data_moodle(moodle).as_deref(), Some(esperado), "{moodle}");
        }
        for ruim in ["-", "", "Nenhuma submissão", "31 fev 2026, 10:00"] {
            assert_eq!(ler_data_moodle(ruim), None, "{ruim}");
        }
    }
}
