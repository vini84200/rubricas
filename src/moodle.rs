//! Planilha de notas do Moodle: a tarefa exporta uma planilha para preencher
//! ("Baixar planilha de notas") e importa de volta ("Enviar planilha de notas").
//! Aqui ela recebe as notas da avaliação e volta como veio, só com as notas e os
//! comentários preenchidos.

use crate::modelo::*;
use crate::submissoes::dono_pelo_nome;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Default, Serialize)]
pub struct Preenchida {
    pub csv: String,
    /// Linhas que receberam nota.
    pub preenchidas: usize,
    /// Nomes (da turma) com correção começada mas não concluída: a linha volta como veio.
    pub incompletas: Vec<String>,
    /// Nomes (da turma) com correção concluída que não estão na planilha.
    pub fora_da_planilha: Vec<String>,
    /// Nomes (do Moodle) que não casaram com ninguém da turma.
    pub sem_aluno: Vec<String>,
    /// Nomes (do Moodle) cuja nota o Moodle não deixa alterar.
    pub bloqueadas: Vec<String>,
    /// Nota máxima no Moodle, quando difere da avaliação e as notas foram convertidas.
    pub convertida_para: Option<f64>,
}

/// As linhas da planilha (a primeira é o cabeçalho) e o cabeçalho em `slug`.
struct Planilha {
    linhas: Vec<Vec<String>>,
    cab: Vec<String>,
    col_nome: usize,
}

impl Planilha {
    fn ler(texto: &str) -> Result<Planilha, String> {
        let texto = texto.trim_start_matches('\u{feff}');
        let mut leitor = csv::ReaderBuilder::new().delimiter(separador(texto)).has_headers(false).flexible(true).from_reader(texto.as_bytes());
        let mut linhas: Vec<Vec<String>> = Vec::new();
        for reg in leitor.records() {
            let reg = reg.map_err(|e| format!("Não foi possível ler a planilha: {e}"))?;
            linhas.push(reg.iter().map(String::from).collect());
        }
        let cab: Vec<String> = linhas.first().map(|l| l.iter().map(|c| slug(c)).collect()).unwrap_or_default();
        let col_nome = cab.iter().position(|c| c == "nome-completo" || c == "full-name");
        let col_nome = col_nome.ok_or("Isto não parece a planilha de notas do Moodle: falta a coluna “Nome completo”")?;
        Ok(Planilha { linhas, cab, col_nome })
    }

    fn coluna(&self, nomes: &[&str]) -> Option<usize> {
        self.cab.iter().position(|c| nomes.contains(&c.as_str()))
    }

    /// Quem é o aluno de uma linha: pelo número de identificação, se a planilha o
    /// traz, ou pelo nome (com as associações feitas no ZIP).
    fn dono<'a>(&self, turma: &'a Turma, apelidos: &'a BTreeMap<String, String>) -> impl Fn(&[String]) -> Option<String> + 'a {
        let col_matricula = self.coluna(&["numero-de-identificacao", "id-number", "matricula"]);
        let col_nome = self.col_nome;
        let pelo_nome = dono_pelo_nome(turma, apelidos);
        move |l: &[String]| {
            if let Some(id) = col_matricula.and_then(|i| l.get(i)).map(|v| v.trim()).filter(|id| turma.alunos.iter().any(|a| a.id == *id)) {
                return Some(id.to_string());
            }
            pelo_nome(l.get(col_nome)?)
        }
    }
}

/// Datas de envio lidas da planilha de notas.
#[derive(Debug, Default, Serialize)]
pub struct Envios {
    /// aluno → `"AAAA-MM-DDTHH:MM"`, ou `None` para quem está na planilha sem envio.
    #[serde(skip)]
    pub datas: BTreeMap<String, Option<String>>,
    pub com_data: usize,
    /// Nomes (do Moodle) com uma data que não deu para ler.
    pub ilegiveis: Vec<String>,
    /// Nomes (do Moodle) que não casaram com ninguém da turma.
    pub sem_aluno: Vec<String>,
}

/// A coluna "Última modificação (envio)" da planilha de notas, por aluno.
pub fn datas_de_envio(texto: &str, turma: &Turma, apelidos: &BTreeMap<String, String>) -> Result<Envios, String> {
    let p = Planilha::ler(texto)?;
    let col = p.coluna(&["ultima-modificacao-envio", "last-modified-submission"]);
    let col = col.ok_or("A planilha não tem a coluna “Última modificação (envio)”")?;
    let dono = p.dono(turma, apelidos);
    let mut r = Envios::default();
    for l in p.linhas.iter().skip(1) {
        let nome = l.get(p.col_nome).cloned().unwrap_or_default();
        let Some(aluno) = dono(l) else {
            if !nome.trim().is_empty() {
                r.sem_aluno.push(nome);
            }
            continue;
        };
        let bruto = l.get(col).map(|v| v.trim()).unwrap_or("");
        let data = if bruto.is_empty() || bruto == "-" { None } else { crate::efeitos::ler_data_moodle(bruto) };
        if data.is_none() && !(bruto.is_empty() || bruto == "-") {
            r.ilegiveis.push(nome);
            continue;
        }
        r.com_data += usize::from(data.is_some());
        r.datas.insert(aluno, data);
    }
    Ok(r)
}

/// `apelidos`: nome no Moodle (em `slug`) → aluno, das associações feitas à mão no ZIP.
pub fn preencher_planilha(
    texto: &str,
    av: &Avaliacao,
    turma: &Turma,
    correcoes: &BTreeMap<String, Correcao>,
    apelidos: &BTreeMap<String, String>,
) -> Result<Preenchida, String> {
    let bom = texto.starts_with('\u{feff}');
    let planilha = Planilha::ler(texto)?;
    let (cab, col_nome) = (&planilha.cab, planilha.col_nome);
    let coluna = |nomes: &[&str]| planilha.coluna(nomes);
    let Some(col_nota) = coluna(&["nota", "grade"]) else {
        return Err("Isto não parece a planilha de notas do Moodle: falta a coluna “Nota”".into());
    };
    let dono = planilha.dono(turma, apelidos);
    let mut linhas = planilha.linhas.clone();

    let col_maxima = coluna(&["nota-maxima", "maximum-grade"]);
    // O Moodle lê a nota com o separador decimal do idioma dele (com o ponto, "3,5"
    // viraria 35). Vale o que a própria planilha usa; sem número com casas, o idioma.
    let numeros = linhas.iter().skip(1).flat_map(|l| [Some(col_nota), col_maxima].into_iter().flatten().filter_map(|i| l.get(i)));
    let virgula = match numeros.map(|v| v.trim()).find(|v| v.contains([',', '.']) && v.chars().any(|c| c.is_ascii_digit())) {
        Some(v) => v.contains(','),
        None => cab[col_nota] == "nota",
    };

    let col_alteravel = cab.iter().position(|c| c.contains("pode-ser-alterada") || c.contains("can-be-changed"));
    let col_comentarios = coluna(&["comentarios-de-feedback", "feedback-comments"]);
    let mut p = Preenchida::default();
    let mut na_planilha = Vec::new();
    for l in linhas.iter_mut().skip(1) {
        let nome = l.get(col_nome).cloned().unwrap_or_default();
        let Some(aluno) = dono(l) else {
            if !nome.trim().is_empty() {
                p.sem_aluno.push(nome);
            }
            continue;
        };
        na_planilha.push(aluno.clone());
        let r = calcular(av, correcoes.get(&aluno));
        match r.andamento() {
            Andamento::Concluida => {}
            Andamento::Parcial => {
                p.incompletas.push(nome);
                continue;
            }
            Andamento::NaoAvaliado => continue,
        }
        if col_alteravel.and_then(|i| l.get(i)).is_some_and(|v| matches!(slug(v).as_str(), "nao" | "no")) {
            p.bloqueadas.push(nome);
            continue;
        }
        if let Some(i) = col_comentarios.filter(|i| *i < l.len()) {
            let html = comentarios(av, &correcoes[&aluno]);
            if !html.is_empty() {
                l[i] = html;
            }
        }
        if col_nota < l.len() {
            let maxima = col_maxima.and_then(|i| l.get(i)).and_then(|v| ler_numero(v, virgula)).filter(|m| *m > 0.0);
            let nota = match maxima {
                Some(m) if (m - av.nota_maxima).abs() > 1e-9 => {
                    p.convertida_para = Some(m);
                    r.nota * m / av.nota_maxima
                }
                _ => r.nota,
            };
            l[col_nota] = crate::notas::numero(nota, virgula);
            p.preenchidas += 1;
        }
    }
    p.fora_da_planilha = turma
        .alunos
        .iter()
        .filter(|a| !na_planilha.contains(&a.id) && calcular(av, correcoes.get(&a.id)).concluida())
        .map(|a| a.nome.clone())
        .collect();

    // Sai sempre com vírgula, o padrão do formulário de envio do Moodle.
    let mut w = csv::Writer::from_writer(Vec::new());
    for l in &linhas {
        w.write_record(l).map_err(|e| e.to_string())?;
    }
    let corpo = String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    p.csv = if bom { format!("\u{feff}{corpo}") } else { corpo };
    Ok(p)
}

/// Os comentários de cada critério, os efeitos aplicados (os que vão ao aluno) e o
/// geral, um parágrafo cada: o Moodle guarda o feedback em HTML.
fn comentarios(av: &Avaliacao, correcao: &Correcao) -> String {
    let mut partes = Vec::new();
    for ra in &av.rubricas {
        let respostas = correcao.respostas.get(&ra.origem);
        for c in ra.rubrica.avaliados() {
            if let Some(x) = respostas.and_then(|m| m.get(&c.id)).filter(|x| !x.comentario.trim().is_empty()) {
                partes.push(format!("{}: {}", c.nome, x.comentario.trim()));
            }
        }
    }
    for t in av.efeitos.iter().filter(|t| t.feedback) {
        if let Some(a) = correcao.efeitos.get(&t.id).filter(|a| t.conferir(a).is_ok()) {
            partes.push(t.texto(a));
        }
    }
    if !correcao.comentario_geral.trim().is_empty() {
        partes.push(correcao.comentario_geral.trim().to_string());
    }
    partes.iter().map(|t| format!("<p>{}</p>", escapar(t).replace('\n', "<br>"))).collect()
}

fn escapar(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn ler_numero(v: &str, virgula: bool) -> Option<f64> {
    let v = v.trim();
    let v = if virgula { v.replace('.', "").replace(',', ".") } else { v.replace(',', "") };
    v.parse().ok()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn avaliacao() -> Avaliacao {
        let mut r: Rubrica = toml::from_str(
            r#"
            titulo = "Lista 3"
            [[criterios]]
            nome = "Compila"
            escala = { tipo = "binaria", pontos = 1.0 }
            [[criterios]]
            nome = "Casos de teste"
            escala = { tipo = "binaria", pontos = 1.0 }
            "#,
        )
        .unwrap();
        r.normalizar();
        Avaliacao {
            id: "l3".into(),
            titulo: "Lista 3".into(),
            turma: "t".into(),
            nota_maxima: 10.0,
            rubricas: vec![RubricaAplicada { origem: "l3".into(), peso: 1.0, rubrica: r }],
            efeitos: vec![],
            prazo: None,
            tolerancia: 0,
        }
    }

    fn turma() -> Turma {
        let alunos = [("00123", "Ana Souza"), ("00456", "Bruno Lima"), ("00789", "Carla Dias"), ("00999", "Davi Reis")];
        Turma { id: "t".into(), alunos: alunos.iter().map(|(id, nome)| Aluno { id: id.to_string(), nome: nome.to_string() }).collect() }
    }

    /// `marcas`: atende? por critério, na ordem; `None` = sem resposta.
    fn correcao(marcas: &[Option<bool>], comentario: &str) -> Correcao {
        let av = avaliacao();
        let mut respostas = BTreeMap::new();
        for (c, m) in av.rubricas[0].rubrica.criterios.iter().zip(marcas) {
            if let Some(atende) = m {
                respostas.insert(c.id.clone(), Resposta { marcacao: Marcacao::Binaria { atende: *atende }, comentario: String::new() });
            }
        }
        Correcao { respostas: BTreeMap::from([("l3".into(), respostas)]), comentario_geral: comentario.into(), ..Default::default() }
    }

    fn correcoes() -> BTreeMap<String, Correcao> {
        BTreeMap::from([
            ("00123".into(), correcao(&[Some(true), Some(true)], "")),
            ("00456".into(), correcao(&[Some(true), Some(false)], "")),
            ("00789".into(), correcao(&[Some(true), None], "")),
        ])
    }

    const CABECALHO: &str = "Identificador,\"Nome completo\",\"Endereço de email\",Status,Nota,\"Nota máxima\",\"A nota pode ser alterada\",\"Última modificação (envio)\",\"Última modificação (nota)\",\"Comentários de feedback\"";

    fn registros(csv: &str) -> Vec<Vec<String>> {
        let mut r = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(csv.trim_start_matches('\u{feff}').as_bytes());
        r.records().map(|l| l.unwrap().iter().map(String::from).collect()).collect()
    }

    fn linha(csv: &str, nome: &str) -> Vec<String> {
        registros(csv).into_iter().find(|l| l[1] == nome).unwrap()
    }

    #[test]
    fn nota_de_correcao_concluida_vai_para_a_coluna_nota_e_o_resto_fica_como_veio() {
        let planilha = format!(
            "{CABECALHO}\n\
             \"Participante 101\",\"Ana Souza\",ana@ufrgs.br,\"Enviado para avaliação\",,\"10,00\",Sim,\"segunda, 6 out 2026, 10:00\",-,\n\
             \"Participante 102\",\"Bruno Lima\",bruno@ufrgs.br,\"Enviado para avaliação\",,\"10,00\",Sim,\"terça, 7 out 2026, 09:00\",-,\n"
        );
        let p = preencher_planilha(&planilha, &avaliacao(), &turma(), &correcoes(), &BTreeMap::new()).unwrap();
        assert_eq!(p.preenchidas, 2);
        assert_eq!(
            linha(&p.csv, "Ana Souza"),
            ["Participante 101", "Ana Souza", "ana@ufrgs.br", "Enviado para avaliação", "10", "10,00", "Sim", "segunda, 6 out 2026, 10:00", "-", ""]
        );
        assert_eq!(linha(&p.csv, "Bruno Lima")[4], "5");
        assert_eq!(registros(&p.csv)[0], registros(CABECALHO)[0]);
    }

    #[test]
    fn planilha_em_ingles_usa_ponto_decimal() {
        let mut av = avaliacao();
        av.nota_maxima = 7.0;
        let planilha = "Identifier,\"Full name\",Status,Grade,\"Maximum grade\",\"Grade can be changed\",\"Last modified (grade)\"\n\
                        \"Participant 102\",\"Bruno Lima\",\"Submitted for grading\",,7.00,Yes,-\n";
        let p = preencher_planilha(planilha, &av, &turma(), &correcoes(), &BTreeMap::new()).unwrap();
        assert_eq!(linha(&p.csv, "Bruno Lima")[3], "3.5");
    }

    #[test]
    fn nota_maxima_diferente_no_moodle_converte_a_nota_e_avisa() {
        let planilha = format!("{CABECALHO}\n\"Participante 102\",\"Bruno Lima\",b@x,,,\"100,00\",Sim,,-,\n");
        let p = preencher_planilha(&planilha, &avaliacao(), &turma(), &correcoes(), &BTreeMap::new()).unwrap();
        assert_eq!(linha(&p.csv, "Bruno Lima")[4], "50");
        assert_eq!(p.convertida_para, Some(100.0));
    }

    #[test]
    fn quem_fica_de_fora_aparece_no_resumo_e_a_linha_fica_como_veio() {
        let planilha = format!(
            "{CABECALHO}\n\
             \"Participante 102\",\"Bruno Lima\",b@x,,,\"10,00\",Sim,,-,\n\
             \"Participante 103\",\"Carla Dias\",c@x,,\"8,00\",\"10,00\",Sim,,-,\n\
             \"Participante 104\",\"Davi Reis\",d@x,,,\"10,00\",Sim,,-,\n\
             \"Participante 105\",\"Zé Ninguém\",z@x,,,\"10,00\",Sim,,-,\n"
        );
        let p = preencher_planilha(&planilha, &avaliacao(), &turma(), &correcoes(), &BTreeMap::new()).unwrap();
        assert_eq!(p.preenchidas, 1);
        assert_eq!(p.incompletas, ["Carla Dias"]);
        assert_eq!(p.sem_aluno, ["Zé Ninguém"]);
        assert_eq!(p.fora_da_planilha, ["Ana Souza"]);
        assert_eq!(linha(&p.csv, "Carla Dias")[4], "8,00");
        assert_eq!(linha(&p.csv, "Davi Reis")[4], "");
    }

    #[test]
    fn casa_pelo_numero_de_identificacao_e_pelos_apelidos_do_zip() {
        let planilha = "Identificador,\"Nome completo\",\"Número de identificação\",Status,Nota,\"Nota máxima\"\n\
                        \"Participante 101\",\"Ana S.\",,,,\"10,00\"\n\
                        \"Participante 102\",\"Bruninho L.\",00456,,,\"10,00\"\n";
        let apelidos = BTreeMap::from([("ana-s".to_string(), "00123".to_string())]);
        let p = preencher_planilha(planilha, &avaliacao(), &turma(), &correcoes(), &apelidos).unwrap();
        assert_eq!(linha(&p.csv, "Ana S.")[4], "10");
        assert_eq!(linha(&p.csv, "Bruninho L.")[4], "5");
        assert!(p.sem_aluno.is_empty());
    }

    #[test]
    fn comentarios_vao_para_o_feedback_e_nota_travada_no_moodle_nao_e_mexida() {
        let mut cs = correcoes();
        let ana = cs.get_mut("00123").unwrap();
        ana.comentario_geral = "Muito bem <3".into();
        ana.respostas.get_mut("l3").unwrap().get_mut("compila").unwrap().comentario = "Sem avisos".into();
        let planilha = format!(
            "{CABECALHO}\n\
             \"Participante 101\",\"Ana Souza\",a@x,,,\"10,00\",Sim,,-,\"<p>antigo</p>\"\n\
             \"Participante 102\",\"Bruno Lima\",b@x,,\"9,00\",\"10,00\",Não,,-,\n"
        );
        let p = preencher_planilha(&planilha, &avaliacao(), &turma(), &cs, &BTreeMap::new()).unwrap();
        assert_eq!(linha(&p.csv, "Ana Souza")[9], "<p>Compila: Sem avisos</p><p>Muito bem &lt;3</p>");
        assert_eq!(linha(&p.csv, "Bruno Lima")[4], "9,00");
        assert_eq!(p.bloqueadas, ["Bruno Lima"]);
        assert_eq!(p.preenchidas, 1);
    }

    #[test]
    fn efeitos_entram_na_nota_e_no_feedback_e_encerrar_dispensa_os_criterios() {
        let mut av = avaliacao();
        let tipo = |id: &str, t: &str| crate::efeitos::TipoEfeito { id: id.into(), ..toml::from_str(t).unwrap() };
        av.efeitos = vec![
            tipo("atraso", "titulo = \"Atraso\"\noperacao = { tipo = \"por_unidade\", pontos = -1.0 }"),
            tipo("plagio", "titulo = \"Plágio\"\nencerra = true\nfeedback = false\noperacao = { tipo = \"fixar\", nota = 0 }"),
        ];
        let mut cs = correcoes();
        let aplicado = |q| crate::efeitos::EfeitoAplicado { quantidade: q, ..Default::default() };
        cs.get_mut("00123").unwrap().efeitos.insert("atraso".into(), aplicado(Some(2.0)));
        // Carla não terminou a correção, mas o zero por plágio encerra
        cs.get_mut("00789").unwrap().efeitos.insert("plagio".into(), aplicado(None));
        let planilha = format!(
            "{CABECALHO}\n\
             \"Participante 101\",\"Ana Souza\",a@x,,,\"10,00\",Sim,,-,\n\
             \"Participante 103\",\"Carla Dias\",c@x,,,\"10,00\",Sim,,-,\n"
        );
        let p = preencher_planilha(&planilha, &av, &turma(), &cs, &BTreeMap::new()).unwrap();
        assert_eq!(linha(&p.csv, "Ana Souza")[4], "8");
        assert_eq!(linha(&p.csv, "Ana Souza")[9], "<p>Atraso (2 dias): −2</p>");
        assert_eq!(linha(&p.csv, "Carla Dias")[4], "0");
        assert_eq!(linha(&p.csv, "Carla Dias")[9], "", "o plágio não vai ao feedback");
        assert!(p.incompletas.is_empty());
    }

    #[test]
    fn datas_de_envio_da_planilha() {
        let planilha = format!(
            "{CABECALHO}\n\
             \"Participante 101\",\"Ana Souza\",a@x,,,\"10,00\",Sim,\"segunda, 6 out 2026, 10:00\",-,\n\
             \"Participante 102\",\"Bruno Lima\",b@x,,,\"10,00\",Sim,-,-,\n\
             \"Participante 103\",\"Carla Dias\",c@x,,,\"10,00\",Sim,ontem,-,\n\
             \"Participante 105\",\"Zé Ninguém\",z@x,,,\"10,00\",Sim,-,-,\n"
        );
        let r = datas_de_envio(&planilha, &turma(), &BTreeMap::new()).unwrap();
        assert_eq!(r.datas, BTreeMap::from([("00123".into(), Some("2026-10-06T10:00".into())), ("00456".into(), None)]));
        assert_eq!((r.com_data, r.ilegiveis.as_slice(), r.sem_aluno.as_slice()), (1, &["Carla Dias".to_string()][..], &["Zé Ninguém".to_string()][..]));
        assert!(datas_de_envio("Identificador,\"Nome completo\",Nota\n", &turma(), &BTreeMap::new()).unwrap_err().contains("envio"));
    }

    #[test]
    fn planilha_salva_pelo_excel_com_ponto_e_virgula_volta_com_virgula_como_o_moodle_espera() {
        let planilha = "\u{feff}Identificador;Nome completo;Nota;Nota máxima\nParticipante 102;Bruno Lima;;10,00\n";
        let p = preencher_planilha(planilha, &avaliacao(), &turma(), &correcoes(), &BTreeMap::new()).unwrap();
        assert_eq!(p.csv, "\u{feff}Identificador,Nome completo,Nota,Nota máxima\nParticipante 102,Bruno Lima,5,\"10,00\"\n");
    }
}
