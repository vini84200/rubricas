//! Tipos do domínio: rubricas, turmas, avaliações, correções e o cálculo da nota.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

fn um() -> f64 {
    1.0
}
fn dez() -> f64 {
    10.0
}
#[allow(clippy::ptr_arg)] // assinatura exigida pelo serde
fn vazio(s: &String) -> bool {
    s.is_empty()
}
fn e_um(v: &f64) -> bool {
    *v == 1.0
}

// ---------- Rubrica: o que o usuário constrói ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rubrica {
    /// Nome do arquivo sem extensão; não é gravado dentro do arquivo.
    #[serde(default, skip_serializing_if = "vazio")]
    pub id: String,
    pub titulo: String,
    #[serde(default, skip_serializing_if = "vazio")]
    pub descricao: String,
    #[serde(default)]
    pub criterios: Vec<Criterio>,
}

/// Um item da rubrica: um critério avaliado ou um passo de roteiro, que só
/// orienta quem avalia e fica fora da nota.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Criterio {
    #[serde(default, skip_serializing_if = "TipoItem::e_criterio")]
    pub tipo: TipoItem,
    #[serde(default, skip_serializing_if = "vazio")]
    pub id: String,
    pub nome: String,
    #[serde(default, skip_serializing_if = "vazio")]
    pub descricao: String,
    #[serde(default = "um", skip_serializing_if = "e_um")]
    pub peso: f64,
    /// Obrigatória em critérios; ausente em passos de roteiro.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escala: Option<Escala>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoItem {
    #[default]
    Criterio,
    Roteiro,
}

impl TipoItem {
    fn e_criterio(&self) -> bool {
        *self == TipoItem::Criterio
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Escala {
    /// Atendeu ou não atendeu.
    Binaria { pontos: f64 },
    /// Níveis fixos, cada um com descritor e pontos.
    Niveis { niveis: Vec<Nivel> },
    /// Nota livre de 0 até `max`.
    Livre {
        max: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passo: Option<f64>,
    },
    /// Só o nível esperado é descrito.
    PontoUnico { esperado: String, max: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nivel {
    #[serde(default, skip_serializing_if = "vazio")]
    pub id: String,
    pub rotulo: String,
    #[serde(default, skip_serializing_if = "vazio")]
    pub descritor: String,
    pub pontos: f64,
}

// ---------- Turma ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aluno {
    pub id: String,
    pub nome: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turma {
    pub id: String,
    pub alunos: Vec<Aluno>,
}

// ---------- Avaliação: rubricas atreladas a uma turma ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Avaliacao {
    #[serde(default, skip_serializing_if = "vazio")]
    pub id: String,
    pub titulo: String,
    pub turma: String,
    #[serde(default = "dez")]
    pub nota_maxima: f64,
    pub rubricas: Vec<RubricaAplicada>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RubricaAplicada {
    /// Id da rubrica na pasta de onde a cópia foi tirada.
    pub origem: String,
    #[serde(default = "um")]
    pub peso: f64,
    /// Cópia congelada: editar a rubrica depois não altera correções feitas.
    pub rubrica: Rubrica,
}

// ---------- Correção: o preenchimento por aluno ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Correcao {
    /// origem da rubrica → id do critério → resposta
    #[serde(default)]
    pub respostas: BTreeMap<String, BTreeMap<String, Resposta>>,
    #[serde(default, skip_serializing_if = "vazio")]
    pub comentario_geral: String,
    #[serde(default, skip_serializing_if = "vazio")]
    pub avaliador: String,
    #[serde(default)]
    pub atualizado_em: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resposta {
    #[serde(flatten)]
    pub marcacao: Marcacao,
    #[serde(default, skip_serializing_if = "vazio")]
    pub comentario: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Marcacao {
    Binaria { atende: bool },
    Nivel { nivel: String },
    Nota { nota: f64 },
    PontoUnico { desempenho: Desempenho, nota: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Desempenho {
    Abaixo,
    Atende,
    Acima,
}

// ---------- Identificadores ----------

/// Um id vira nome de arquivo, então não pode conter separadores de caminho.
pub fn id_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('.')
        && !id.ends_with(' ')
        && !id.starts_with(' ')
        && !id.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// Transforma um texto livre em id legível: minúsculas, sem acentos, com hífens.
pub fn slug(texto: &str) -> String {
    let mut s = String::new();
    for c in texto.trim().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            c => c.to_ascii_lowercase(),
        };
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-');
    let s: String = s.chars().take(60).collect();
    s.trim_end_matches('-').to_string()
}

/// Devolve `base` ou `base-2`, `base-3`… — o primeiro que ainda não está em uso.
pub fn id_livre(base: &str, usado: impl Fn(&str) -> bool) -> String {
    let base = if base.is_empty() { "item" } else { base };
    if !usado(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base}-{n}")).find(|c| !usado(c)).unwrap()
}

// ---------- Validação e pontuação ----------

fn finito(nome: &str, v: f64) -> Result<(), String> {
    if v.is_finite() { Ok(()) } else { Err(format!("{nome} precisa ser um número")) }
}

impl Rubrica {
    /// Preenche ids que faltam (arquivos escritos à mão podem omiti-los).
    pub fn normalizar(&mut self) {
        let mut usados = HashSet::new();
        for c in &mut self.criterios {
            if c.id.trim().is_empty() || usados.contains(&c.id) {
                c.id = id_livre(&slug(&c.nome), |x| usados.contains(x));
            }
            usados.insert(c.id.clone());
            if let Some(Escala::Niveis { niveis }) = &mut c.escala {
                let mut usados_n = HashSet::new();
                for n in niveis {
                    if n.id.trim().is_empty() || usados_n.contains(&n.id) {
                        n.id = id_livre(&slug(&n.rotulo), |x| usados_n.contains(x));
                    }
                    usados_n.insert(n.id.clone());
                }
            }
        }
    }

    pub fn validar(&self) -> Result<(), String> {
        if self.titulo.trim().is_empty() {
            return Err("A rubrica precisa de um título".into());
        }
        if !self.criterios.iter().any(|c| !c.roteiro()) {
            return Err("A rubrica precisa de pelo menos um critério".into());
        }
        let mut ids = HashSet::new();
        for c in &self.criterios {
            let onde = format!("{} “{}”", if c.roteiro() { "Passo de roteiro" } else { "Critério" }, c.nome);
            if c.nome.trim().is_empty() {
                return Err("Todo critério e passo de roteiro precisa de um nome".into());
            }
            if c.id.is_empty() || !ids.insert(&c.id) {
                return Err(format!("{onde}: id vazio ou repetido"));
            }
            if c.roteiro() {
                if c.escala.is_some() || c.peso != 1.0 {
                    return Err(format!("{onde}: passo de roteiro não tem escala nem peso"));
                }
                continue;
            }
            finito(&format!("{onde}: o peso"), c.peso)?;
            if c.peso < 0.0 {
                return Err(format!("{onde}: o peso não pode ser negativo"));
            }
            let Some(escala) = &c.escala else {
                return Err(format!("{onde}: falta a escala"));
            };
            match escala {
                Escala::Binaria { pontos } => {
                    finito(&format!("{onde}: os pontos"), *pontos)?;
                    if *pontos <= 0.0 {
                        return Err(format!("{onde}: os pontos precisam ser maiores que zero"));
                    }
                }
                Escala::Niveis { niveis } => {
                    if niveis.len() < 2 {
                        return Err(format!("{onde}: defina pelo menos dois níveis"));
                    }
                    let mut ids_n = HashSet::new();
                    for n in niveis {
                        if n.rotulo.trim().is_empty() {
                            return Err(format!("{onde}: todo nível precisa de um rótulo"));
                        }
                        if n.id.is_empty() || !ids_n.insert(&n.id) {
                            return Err(format!("{onde}: nível com id vazio ou repetido"));
                        }
                        finito(&format!("{onde}: os pontos de “{}”", n.rotulo), n.pontos)?;
                    }
                }
                Escala::Livre { max, passo } => {
                    finito(&format!("{onde}: a nota máxima"), *max)?;
                    if *max <= 0.0 {
                        return Err(format!("{onde}: a nota máxima precisa ser maior que zero"));
                    }
                    if let Some(p) = passo
                        && (!p.is_finite() || *p <= 0.0 || *p > *max) {
                            return Err(format!("{onde}: o passo precisa ser maior que zero e não passar da nota máxima"));
                        }
                }
                Escala::PontoUnico { max, .. } => {
                    finito(&format!("{onde}: a nota máxima"), *max)?;
                    if *max <= 0.0 {
                        return Err(format!("{onde}: a nota máxima precisa ser maior que zero"));
                    }
                }
            }
        }
        Ok(())
    }

    /// Itens e níveis sem id recebem o id do item de mesmo nome em `anterior`.
    /// Assim, reescrever o arquivo inteiro sem ids não muda identidades — e as
    /// correções feitas continuam valendo. Chame antes de `normalizar`.
    pub fn herdar_ids(&mut self, anterior: &Rubrica) {
        let mesmo = |a: &str, b: &str| a.trim().to_lowercase() == b.trim().to_lowercase();
        let mut usados: HashSet<String> = self.criterios.iter().map(|c| c.id.clone()).filter(|id| !id.is_empty()).collect();
        for c in &mut self.criterios {
            if c.id.trim().is_empty()
                && let Some(a) = anterior.criterios.iter().find(|a| mesmo(&a.nome, &c.nome) && !usados.contains(&a.id))
            {
                c.id = a.id.clone();
                usados.insert(c.id.clone());
            }
            let antigos = match anterior.criterio(&c.id).and_then(|a| a.escala.as_ref()) {
                Some(Escala::Niveis { niveis }) => niveis,
                _ => continue,
            };
            if let Some(Escala::Niveis { niveis }) = &mut c.escala {
                let mut usados_n: HashSet<String> = niveis.iter().map(|n| n.id.clone()).filter(|id| !id.is_empty()).collect();
                for n in niveis.iter_mut().filter(|n| n.id.trim().is_empty()) {
                    if let Some(a) = antigos.iter().find(|a| mesmo(&a.rotulo, &n.rotulo) && !usados_n.contains(&a.id)) {
                        n.id = a.id.clone();
                        usados_n.insert(n.id.clone());
                    }
                }
            }
        }
    }

    /// TOML sem os ids que `normalizar` recriaria a partir dos nomes: menos texto
    /// para ler e reescrever, mesma rubrica de volta.
    pub fn toml_enxuto(&self) -> String {
        let mut completa = self.clone();
        completa.id.clear();
        let mut enxuta = completa.clone();
        for c in &mut enxuta.criterios {
            if c.id == slug(&c.nome) {
                c.id.clear();
            }
            if let Some(Escala::Niveis { niveis }) = &mut c.escala {
                for n in niveis.iter_mut().filter(|n| n.id == slug(&n.rotulo)) {
                    n.id.clear();
                }
            }
        }
        let texto = toml::to_string_pretty(&enxuta).unwrap_or_default();
        // Nomes repetidos podem fazer a releitura gerar outros ids; aí vai completo.
        let volta = toml::from_str::<Rubrica>(&texto).map(|mut r| {
            r.normalizar();
            r
        });
        match volta {
            Ok(r) if r == completa => texto,
            _ => toml::to_string_pretty(&completa).unwrap_or_default(),
        }
    }

    /// (critérios avaliados, passos de roteiro)
    pub fn contagem(&self) -> (usize, usize) {
        let passos = self.criterios.iter().filter(|c| c.roteiro()).count();
        (self.criterios.len() - passos, passos)
    }

    pub fn criterio(&self, id: &str) -> Option<&Criterio> {
        self.criterios.iter().find(|c| c.id == id)
    }
}

impl Criterio {
    pub fn roteiro(&self) -> bool {
        self.tipo == TipoItem::Roteiro
    }

    /// Maior pontuação possível, antes do peso. Zero para passos de roteiro.
    pub fn maximo(&self) -> f64 {
        match &self.escala {
            None => 0.0,
            Some(Escala::Binaria { pontos }) => *pontos,
            Some(Escala::Niveis { niveis }) => niveis.iter().map(|n| n.pontos).fold(0.0, f64::max),
            Some(Escala::Livre { max, .. } | Escala::PontoUnico { max, .. }) => *max,
        }
    }

    /// Pontos de uma marcação, antes do peso. Erro se ela não combina com a escala.
    pub fn pontos(&self, m: &Marcacao) -> Result<f64, String> {
        let erro = |msg: &str| Err(format!("Critério “{}”: {msg}", self.nome));
        let Some(escala) = self.escala.as_ref().filter(|_| !self.roteiro()) else {
            return erro("é um passo de roteiro, não recebe marcação");
        };
        match (escala, m) {
            (Escala::Binaria { pontos }, Marcacao::Binaria { atende }) => Ok(if *atende { *pontos } else { 0.0 }),
            (Escala::Niveis { niveis }, Marcacao::Nivel { nivel }) => match niveis.iter().find(|n| &n.id == nivel) {
                Some(n) => Ok(n.pontos),
                None => erro("nível desconhecido"),
            },
            (Escala::Livre { max, .. }, Marcacao::Nota { nota })
            | (Escala::PontoUnico { max, .. }, Marcacao::PontoUnico { nota, .. }) => {
                if nota.is_finite() && *nota >= 0.0 && *nota <= *max + 1e-9 {
                    Ok(*nota)
                } else {
                    erro(&format!("a nota precisa estar entre 0 e {max}"))
                }
            }
            _ => erro("a marcação não combina com a escala"),
        }
    }
}

impl Avaliacao {
    pub fn validar(&self) -> Result<(), String> {
        if self.titulo.trim().is_empty() {
            return Err("A avaliação precisa de um título".into());
        }
        if !self.nota_maxima.is_finite() || self.nota_maxima <= 0.0 {
            return Err("A nota máxima precisa ser maior que zero".into());
        }
        if self.rubricas.is_empty() {
            return Err("Escolha pelo menos uma rubrica".into());
        }
        let mut origens = HashSet::new();
        for r in &self.rubricas {
            if !origens.insert(&r.origem) {
                return Err(format!("A rubrica “{}” aparece duas vezes", r.rubrica.titulo));
            }
            if !r.peso.is_finite() || r.peso < 0.0 {
                return Err(format!("Peso inválido para a rubrica “{}”", r.rubrica.titulo));
            }
        }
        if self.rubricas.iter().map(|r| r.peso).sum::<f64>() <= 0.0 {
            return Err("Pelo menos uma rubrica precisa ter peso maior que zero".into());
        }
        Ok(())
    }

    pub fn rubrica(&self, origem: &str) -> Option<&RubricaAplicada> {
        self.rubricas.iter().find(|r| r.origem == origem)
    }

    pub fn total_criterios(&self) -> usize {
        self.rubricas.iter().map(|r| r.rubrica.criterios.iter().filter(|c| !c.roteiro()).count()).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Parcial {
    pub pontos: f64,
    pub maximo: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resultado {
    /// Uma entrada por rubrica, na ordem da avaliação; já com os pesos dos critérios.
    pub rubricas: Vec<Parcial>,
    pub nota: f64,
    pub respondidos: usize,
    pub total: usize,
}

impl Resultado {
    pub fn concluida(&self) -> bool {
        self.total > 0 && self.respondidos == self.total
    }
}

/// nota = nota_maxima × Σ peso_r × (pontos_r ÷ máximo_r) ÷ Σ peso_r.
/// Critério sem resposta conta zero. Marcações que não combinam com a escala
/// (sobras de uma rubrica atualizada) são ignoradas.
pub fn calcular(av: &Avaliacao, correcao: Option<&Correcao>) -> Resultado {
    let mut rubricas = Vec::new();
    let (mut respondidos, mut total) = (0, 0);
    let (mut soma, mut soma_pesos) = (0.0, 0.0);
    for ra in &av.rubricas {
        let respostas = correcao.and_then(|c| c.respostas.get(&ra.origem));
        let (mut pontos, mut maximo) = (0.0, 0.0);
        for c in ra.rubrica.criterios.iter().filter(|c| !c.roteiro()) {
            total += 1;
            maximo += c.peso * c.maximo();
            let p = respostas.and_then(|r| r.get(&c.id)).and_then(|r| c.pontos(&r.marcacao).ok());
            if let Some(p) = p {
                respondidos += 1;
                pontos += c.peso * p;
            }
        }
        if maximo > 0.0 && ra.peso > 0.0 {
            soma += ra.peso * (pontos / maximo);
            soma_pesos += ra.peso;
        }
        rubricas.push(Parcial { pontos, maximo });
    }
    let nota = if soma_pesos > 0.0 { av.nota_maxima * soma / soma_pesos } else { 0.0 };
    Resultado { rubricas, nota, respondidos, total }
}

// ---------- Lista de alunos em CSV ----------

fn parece_id(s: &str) -> bool {
    !s.is_empty() && !s.contains(' ') && s.chars().any(|c| c.is_ascii_digit())
}

fn id_seguro(s: &str) -> String {
    let mut limpo = String::new();
    for c in s.trim().chars() {
        let c = if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c };
        // o limite de `id_valido` é em bytes, não em caracteres
        if limpo.len() + c.len_utf8() > 100 {
            break;
        }
        limpo.push(c);
    }
    limpo.trim_start_matches('.').trim().to_string()
}

/// `,`, `;` (Excel em português) ou tabulação, o que mais aparece na primeira linha.
pub fn separador(texto: &str) -> u8 {
    let primeira = texto.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let conta = |c: char| primeira.matches(c).count();
    if conta('\t') > 0 && conta('\t') >= conta(';').max(conta(',')) {
        b'\t'
    } else if conta(';') > conta(',') {
        b';'
    } else {
        b','
    }
}

/// Lê uma lista de alunos. Aceita `,`, `;` ou tabulação, com ou sem cabeçalho,
/// com colunas de matrícula e nome em qualquer ordem — inclusive a exportação
/// de participantes do Moodle (Nome, Sobrenome, Número de identificação).
pub fn ler_alunos(texto: &str) -> Result<Vec<Aluno>, String> {
    let texto = texto.trim_start_matches('\u{feff}');
    let mut leitor = csv::ReaderBuilder::new()
        .delimiter(separador(texto))
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(texto.as_bytes());
    let mut linhas: Vec<Vec<String>> = Vec::new();
    for reg in leitor.records() {
        let reg = reg.map_err(|e| format!("Não foi possível ler a lista: {e}"))?;
        let campos: Vec<String> = reg.iter().map(|c| c.to_string()).collect();
        if campos.iter().any(|c| !c.is_empty()) {
            linhas.push(campos);
        }
    }
    if linhas.is_empty() {
        return Err("A lista de alunos está vazia".into());
    }

    const COL_ID: &[&str] = &[
        "matricula", "cartao", "id", "ra", "codigo", "numero", "numero-de-identificacao", "id-number", "idnumber",
        "identificacao",
    ];
    const COL_NOME: &[&str] = &["nome", "name", "aluno", "aluna", "estudante", "nome-completo", "first-name", "nome-do-aluno"];
    const COL_SOBRENOME: &[&str] = &["sobrenome", "surname", "last-name", "ultimo-nome"];
    let chaves: Vec<String> = linhas[0].iter().map(|c| slug(c)).collect();
    let acha = |nomes: &[&str]| chaves.iter().position(|c| nomes.contains(&c.as_str()));
    let (col_id, col_nome, col_sobrenome) = (acha(COL_ID), acha(COL_NOME), acha(COL_SOBRENOME));
    let tem_cabecalho = col_id.is_some() || col_nome.is_some();

    let mut alunos = Vec::new();
    let mut usados = HashSet::new();
    for campos in linhas.iter().skip(if tem_cabecalho { 1 } else { 0 }) {
        let campo = |i: Option<usize>| i.and_then(|i| campos.get(i)).cloned().unwrap_or_default();
        let (bruto_id, mut nome) = if tem_cabecalho {
            (campo(col_id), campo(col_nome))
        } else if campos.len() == 1 {
            (String::new(), campos[0].clone())
        } else if parece_id(&campos[0]) {
            (campos[0].clone(), campos[1].clone())
        } else if parece_id(&campos[1]) {
            (campos[1].clone(), campos[0].clone())
        } else {
            // Sem cabeçalho e sem nada que pareça matrícula: a primeira coluna é o nome.
            (String::new(), campos[0].clone())
        };
        let sobrenome = campo(col_sobrenome);
        if !sobrenome.is_empty() {
            nome = format!("{nome} {sobrenome}").trim().to_string();
        }
        if nome.is_empty() {
            nome = bruto_id.clone();
        }
        if nome.is_empty() {
            continue;
        }
        let base = id_seguro(&bruto_id);
        let base = if base.is_empty() { slug(&nome) } else { base };
        let id = id_livre(&base, |x| usados.contains(x));
        usados.insert(id.clone());
        alunos.push(Aluno { id, nome });
    }
    if alunos.is_empty() {
        return Err("Nenhum aluno encontrado na lista".into());
    }
    Ok(alunos)
}

pub fn escrever_alunos(alunos: &[Aluno]) -> String {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(["matricula", "nome"]).unwrap();
    for a in alunos {
        w.write_record([&a.id, &a.nome]).unwrap();
    }
    String::from_utf8(w.into_inner().unwrap()).unwrap()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn rubrica() -> Rubrica {
        let mut r: Rubrica = toml::from_str(
            r#"
            titulo = "Código"
            [[criterios]]
            nome = "Compila"
            escala = { tipo = "binaria", pontos = 1.0 }
            [[criterios]]
            nome = "Modularização"
            peso = 2.0
            [criterios.escala]
            tipo = "niveis"
            [[criterios.escala.niveis]]
            rotulo = "Insuficiente"
            pontos = 0.0
            [[criterios.escala.niveis]]
            rotulo = "Bom"
            pontos = 2.0
            [[criterios.escala.niveis]]
            rotulo = "Excelente"
            pontos = 3.0
            [[criterios]]
            nome = "Apresentação"
            escala = { tipo = "livre", max = 3.0 }
            "#,
        )
        .unwrap();
        r.id = "codigo".into();
        r.normalizar();
        r
    }

    fn avaliacao() -> Avaliacao {
        Avaliacao {
            id: "tf".into(),
            titulo: "TF".into(),
            turma: "d".into(),
            nota_maxima: 10.0,
            rubricas: vec![RubricaAplicada { origem: "codigo".into(), peso: 1.0, rubrica: rubrica() }],
        }
    }

    fn resp(m: Marcacao) -> Resposta {
        Resposta { marcacao: m, comentario: String::new() }
    }

    #[test]
    fn normalizar_gera_ids_a_partir_dos_nomes() {
        let r = rubrica();
        assert_eq!(r.criterios[1].id, "modularizacao");
        let Some(Escala::Niveis { niveis }) = &r.criterios[1].escala else { panic!() };
        assert_eq!(niveis[2].id, "excelente");
        r.validar().unwrap();
    }

    #[test]
    fn rubrica_sobrevive_a_ida_e_volta_em_toml() {
        let mut r = rubrica();
        r.id.clear();
        let texto = toml::to_string_pretty(&r).unwrap();
        let de_volta: Rubrica = toml::from_str(&texto).unwrap();
        assert_eq!(r, de_volta, "{texto}");
    }

    #[test]
    fn nota_usa_pesos_e_ignora_o_que_falta() {
        let av = avaliacao();
        // máximo: 1 + 2×3 + 3 = 10
        let vazio = calcular(&av, None);
        assert_eq!((vazio.nota, vazio.respondidos, vazio.total), (0.0, 0, 3));

        let mut c = Correcao::default();
        let r = c.respostas.entry("codigo".into()).or_default();
        r.insert("compila".into(), resp(Marcacao::Binaria { atende: true }));
        r.insert("modularizacao".into(), resp(Marcacao::Nivel { nivel: "bom".into() }));
        let parcial = calcular(&av, Some(&c));
        assert_eq!(parcial.rubricas[0], Parcial { pontos: 5.0, maximo: 10.0 });
        assert_eq!(parcial.nota, 5.0);
        assert!(!parcial.concluida());

        c.respostas.get_mut("codigo").unwrap().insert("apresentacao".into(), resp(Marcacao::Nota { nota: 1.5 }));
        let fim = calcular(&av, Some(&c));
        assert_eq!(fim.nota, 6.5);
        assert!(fim.concluida());
    }

    #[test]
    fn pesos_entre_rubricas_normalizam_para_a_nota_maxima() {
        let mut av = avaliacao();
        let mut outra = rubrica();
        outra.criterios.truncate(1);
        av.rubricas.push(RubricaAplicada { origem: "fala".into(), peso: 3.0, rubrica: outra });
        let mut c = Correcao::default();
        c.respostas.entry("fala".into()).or_default().insert("compila".into(), resp(Marcacao::Binaria { atende: true }));
        // rubrica 1 zerada (peso 1), rubrica 2 completa (peso 3) → 10 × 3/4
        assert_eq!(calcular(&av, Some(&c)).nota, 7.5);
    }

    const PRATICA: &str = r#"
        titulo = "Prática"
        [[criterios]]
        tipo = "roteiro"
        nome = "Rode com entrada vazia"
        descricao = "Aperte `Enter` sem digitar nada."
        [[criterios]]
        nome = "Trata entrada vazia"
        escala = { tipo = "binaria", pontos = 2.0 }
    "#;

    #[test]
    fn passos_de_roteiro_nao_contam_na_nota() {
        let mut r: Rubrica = toml::from_str(PRATICA).unwrap();
        r.normalizar();
        r.validar().unwrap();
        assert!(r.criterios[0].roteiro() && !r.criterios[1].roteiro());
        let av = Avaliacao {
            id: "p".into(),
            titulo: "P".into(),
            turma: "d".into(),
            nota_maxima: 10.0,
            rubricas: vec![RubricaAplicada { origem: "p".into(), peso: 1.0, rubrica: r }],
        };
        assert_eq!(av.total_criterios(), 1);
        let mut c = Correcao::default();
        c.respostas.entry("p".into()).or_default().insert("trata-entrada-vazia".into(), resp(Marcacao::Binaria { atende: true }));
        let fim = calcular(&av, Some(&c));
        assert_eq!((fim.nota, fim.respondidos, fim.total), (10.0, 1, 1));
        assert!(fim.concluida());
    }

    #[test]
    fn passo_de_roteiro_em_toml_fica_enxuto() {
        let mut r: Rubrica = toml::from_str(PRATICA).unwrap();
        r.normalizar();
        let texto = toml::to_string_pretty(&r).unwrap();
        assert!(texto.contains("tipo = \"roteiro\""), "{texto}");
        assert!(!texto.contains("peso"), "{texto}");
        assert_eq!(toml::from_str::<Rubrica>(&texto).unwrap(), r);
    }

    #[test]
    fn roteiro_nao_tem_escala_e_criterio_precisa_de_uma() {
        let ler = |t: &str| {
            let mut r: Rubrica = toml::from_str(t).unwrap();
            r.normalizar();
            r.validar()
        };
        let com_escala = PRATICA.replace("descricao = \"Aperte `Enter` sem digitar nada.\"", "escala = { tipo = \"binaria\", pontos = 1.0 }");
        assert!(ler(&com_escala).unwrap_err().contains("Rode com entrada vazia"));
        let sem_escala = PRATICA.replace("escala = { tipo = \"binaria\", pontos = 2.0 }", "");
        assert!(ler(&sem_escala).unwrap_err().contains("Trata entrada vazia"));
        let so_roteiro = PRATICA.split("[[criterios]]").take(2).collect::<Vec<_>>().join("[[criterios]]");
        assert!(ler(&so_roteiro).is_err());
    }

    #[test]
    fn marcacao_precisa_combinar_com_a_escala() {
        let r = rubrica();
        assert!(r.criterios[0].pontos(&Marcacao::Nota { nota: 1.0 }).is_err());
        assert!(r.criterios[1].pontos(&Marcacao::Nivel { nivel: "inexistente".into() }).is_err());
        assert!(r.criterios[2].pontos(&Marcacao::Nota { nota: 3.5 }).is_err());
        assert_eq!(r.criterios[2].pontos(&Marcacao::Nota { nota: 3.0 }), Ok(3.0));
    }

    #[test]
    fn resposta_em_json_fica_plana() {
        let r = Resposta { marcacao: Marcacao::Nivel { nivel: "bom".into() }, comentario: "ok".into() };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j, serde_json::json!({"tipo": "nivel", "nivel": "bom", "comentario": "ok"}));
        assert_eq!(serde_json::from_value::<Resposta>(j).unwrap(), r);
    }

    #[test]
    fn le_listas_de_alunos_em_varios_formatos() {
        let a = ler_alunos("matricula,nome\n00123,Ana Souza\n00456,Bruno Lima\n").unwrap();
        assert_eq!(a[1], Aluno { id: "00456".into(), nome: "Bruno Lima".into() });

        let a = ler_alunos("\u{feff}Nome;Cartão\nAna Souza;00123\n").unwrap();
        assert_eq!(a[0], Aluno { id: "00123".into(), nome: "Ana Souza".into() });

        let a = ler_alunos("Ana Souza\nBruno Lima\nAna Souza\n").unwrap();
        assert_eq!(a.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["ana-souza", "bruno-lima", "ana-souza-2"]);

        let a = ler_alunos("Bruno Lima\t00456\n").unwrap();
        assert_eq!(a[0], Aluno { id: "00456".into(), nome: "Bruno Lima".into() });

        let a = ler_alunos("Nome,Sobrenome,Número de identificação\nAna,Souza,00123\n").unwrap();
        assert_eq!(a[0], Aluno { id: "00123".into(), nome: "Ana Souza".into() });

        // sem cabeçalho e sem matrícula, a primeira coluna é o nome
        let a = ler_alunos("Souza,Ana\n").unwrap();
        assert_eq!(a[0], Aluno { id: "souza".into(), nome: "Souza".into() });

        // ids longos e acentuados continuam válidos como nome de arquivo
        let a = ler_alunos(&format!("id,nome\n{},Zé\n", "ç".repeat(120))).unwrap();
        assert!(id_valido(&a[0].id), "{}", a[0].id.len());

        let a = ler_alunos("id,nome\n../../etc,Zé\n").unwrap();
        assert!(id_valido(&a[0].id), "{}", a[0].id);
        assert!(ler_alunos("\n\n").is_err());
    }

    #[test]
    fn ids_nao_escapam_da_pasta() {
        for ruim in ["", ".", "..", "../x", "a/b", "a\\b", ".oculto", "a\0b"] {
            assert!(!id_valido(ruim), "{ruim:?}");
        }
        assert!(id_valido("INF01202-D 2026_2"));
        assert_eq!(slug("  Trabalho Final — Código!  "), "trabalho-final-codigo");
    }
}
