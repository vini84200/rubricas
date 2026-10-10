//! Leitura e escrita na pasta de dados.
//!
//! ```text
//! dados/
//!   rubricas/<id>.toml              uma rubrica por arquivo, editável à mão
//!   turmas/<id>.csv                 matricula,nome
//!   avaliacoes/<id>.json            título, turma e cópia das rubricas usadas
//!   correcoes/<avaliacao>/<aluno>.json
//!   efeitos/<id>.toml               tipos de efeito sobre a nota (zero por plágio, atraso…)
//!   arquivadas.toml                 ids de rubricas, turmas e avaliações fora das listas
//!   usuarios.toml                   criado por `rubricas usuario <nome>`
//! ```

use crate::efeitos::{Dispensa, EfeitoAplicado, TipoEfeito};
use crate::modelo::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::sync::watch;
use std::{fs, io};

#[derive(Debug)]
pub enum Erro {
    NaoEncontrado(String),
    Invalido(String),
    Conflito(String),
    Io(io::Error),
}

impl std::fmt::Display for Erro {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Erro::NaoEncontrado(m) | Erro::Invalido(m) | Erro::Conflito(m) => f.write_str(m),
            Erro::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for Erro {
    fn from(e: io::Error) -> Self {
        Erro::Io(e)
    }
}

pub type Res<T> = Result<T, Erro>;

pub struct Dados {
    raiz: PathBuf,
    /// Uma escrita por vez: ler–alterar–gravar precisa ser atômico entre requisições.
    trava: Mutex<()>,
    /// Muda a cada gravação que pode dar trabalho ao executor (o contador não importa).
    fila: watch::Sender<u64>,
}

/// Grava num temporário e renomeia, para nunca deixar um arquivo pela metade.
pub fn gravar(caminho: &Path, conteudo: &[u8]) -> io::Result<()> {
    if let Some(pai) = caminho.parent() {
        fs::create_dir_all(pai)?;
    }
    let nome = caminho.file_name().unwrap_or_default().to_string_lossy();
    let tmp = caminho.with_file_name(format!(".{nome}.tmp"));
    fs::write(&tmp, conteudo)?;
    fs::rename(&tmp, caminho)
}

pub(crate) fn checar_id(id: &str) -> Res<()> {
    if id_valido(id) {
        Ok(())
    } else {
        Err(Erro::Invalido(format!(
            "Nome inválido: {id:?}. Use até 100 bytes, sem / \\ : * ? \" < > | e sem começar com ponto."
        )))
    }
}

/// Lê um arquivo de texto. `None` se não existe; `Invalido` se não é UTF-8.
pub(crate) fn ler_opcional(caminho: &Path) -> Res<Option<String>> {
    match fs::read(caminho) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
            let nome = caminho.file_name().unwrap_or_default().to_string_lossy();
            Erro::Invalido(format!("{nome}: o arquivo não está em UTF-8"))
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Lê um JSON. `None` se o arquivo não existe.
pub(crate) fn ler_json<T: serde::de::DeserializeOwned>(caminho: &Path) -> Res<Option<T>> {
    match ler_opcional(caminho)? {
        None => Ok(None),
        Some(t) => serde_json::from_str(&t).map(Some).map_err(|e| {
            Erro::Invalido(format!("{}: {e}", caminho.file_name().unwrap_or_default().to_string_lossy()))
        }),
    }
}

/// Lê os bytes de um arquivo; se não existe, `NaoEncontrado` com a frase dada.
pub(crate) fn ler_ou(caminho: &Path, nao_existe: impl FnOnce() -> String) -> Res<Vec<u8>> {
    match fs::read(caminho) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Erro::NaoEncontrado(nao_existe())),
        outro => Ok(outro?),
    }
}

/// Erro de leitura de TOML com linha e coluna, para quem escreveu o arquivo achar.
pub(crate) fn erro_toml(texto: &str, e: &toml::de::Error) -> String {
    let onde = e.span().map(|s| {
        let antes = &texto[..s.start.min(texto.len())];
        let linha = antes.matches('\n').count() + 1;
        let coluna = antes.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("linha {linha}, coluna {coluna}: ")
    });
    format!("{}{}", onde.unwrap_or_default(), e.message().trim())
}

/// Uma marcação que não pôde ser aplicada (critério removido, nível inexistente…).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Recusada {
    pub rubrica: String,
    pub criterio: String,
    pub motivo: String,
}

/// O que mudar nos efeitos de uma correção: `None` tira o efeito ou a dispensa.
#[derive(Debug, Default)]
pub struct AlteracaoEfeitos {
    pub efeitos: Vec<(String, Option<EfeitoAplicado>)>,
    pub dispensadas: Vec<(String, Option<Dispensa>)>,
}

/// Os efeitos que uma avaliação usa, ao criá-la ou alterá-la.
#[derive(Debug, Default)]
pub struct EscolhaEfeitos {
    /// Ids no catálogo (`efeitos/<id>.toml`).
    pub ids: Vec<String>,
    pub prazo: Option<String>,
    pub tolerancia: u32,
    /// Ids cuja cópia deve ser refeita a partir do catálogo.
    pub atualizar: Vec<String>,
}

/// Os itens que se criam, compartilham, arquivam e apagam. Cada um é um
/// arquivo `<pasta>/<id>.<ext>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    Rubricas,
    Turmas,
    Avaliacoes,
}

impl Tipo {
    pub fn pasta(self) -> &'static str {
        match self {
            Tipo::Rubricas => "rubricas",
            Tipo::Turmas => "turmas",
            Tipo::Avaliacoes => "avaliacoes",
        }
    }

    fn extensao(self) -> &'static str {
        match self {
            Tipo::Rubricas => "toml",
            Tipo::Turmas => "csv",
            Tipo::Avaliacoes => "json",
        }
    }

    /// No singular, como a política e o terminal o chamam.
    pub fn nome(self) -> &'static str {
        match self {
            Tipo::Rubricas => "rubrica",
            Tipo::Turmas => "turma",
            Tipo::Avaliacoes => "avaliacao",
        }
    }

    /// Aceita o nome no singular ou a pasta.
    pub fn por_nome(nome: &str) -> Option<Tipo> {
        [Tipo::Avaliacoes, Tipo::Rubricas, Tipo::Turmas].into_iter().find(|t| t.nome() == nome || t.pasta() == nome)
    }

    /// Para frases: "Você não tem acesso a esta avaliação".
    pub fn este(self) -> &'static str {
        match self {
            Tipo::Rubricas => "esta rubrica",
            Tipo::Turmas => "esta turma",
            Tipo::Avaliacoes => "esta avaliação",
        }
    }

    fn nao_existe(self, id: &str) -> Erro {
        let nome = match self {
            Tipo::Rubricas => "Rubrica",
            Tipo::Turmas => "Turma",
            Tipo::Avaliacoes => "Avaliação",
        };
        Erro::NaoEncontrado(format!("{nome} “{id}” não existe"))
    }
}

/// Ids arquivados: somem das listas e das escolhas, mas continuam no disco e
/// funcionando. Fica num arquivo à parte porque turma é CSV e não tem onde marcar.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arquivadas {
    #[serde(default)]
    rubricas: BTreeSet<String>,
    #[serde(default)]
    turmas: BTreeSet<String>,
    #[serde(default)]
    avaliacoes: BTreeSet<String>,
}

impl Arquivadas {
    fn ids_mut(&mut self, tipo: Tipo) -> &mut BTreeSet<String> {
        match tipo {
            Tipo::Rubricas => &mut self.rubricas,
            Tipo::Turmas => &mut self.turmas,
            Tipo::Avaliacoes => &mut self.avaliacoes,
        }
    }

    pub fn contem(&self, tipo: Tipo, id: &str) -> bool {
        let todas = [(Tipo::Rubricas, &self.rubricas), (Tipo::Turmas, &self.turmas), (Tipo::Avaliacoes, &self.avaliacoes)];
        todas.iter().any(|(t, ids)| *t == tipo && ids.contains(id))
    }
}

impl Dados {
    pub fn abrir(raiz: impl Into<PathBuf>) -> io::Result<Self> {
        let raiz = raiz.into();
        for sub in ["rubricas", "turmas", "avaliacoes", "correcoes"] {
            fs::create_dir_all(raiz.join(sub))?;
        }
        Ok(Dados { raiz, trava: Mutex::new(()), fila: watch::Sender::new(0) })
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// Uma escrita por vez na pasta inteira.
    pub(crate) fn travar(&self) -> std::sync::MutexGuard<'_, ()> {
        self.trava.lock().unwrap()
    }

    /// Avisa quem espera a fila (o executor). Chame depois de gravar.
    pub(crate) fn fila_mudou(&self) {
        self.fila.send_modify(|n| *n += 1);
    }

    pub fn ouvir_fila(&self) -> watch::Receiver<u64> {
        self.fila.subscribe()
    }

    fn caminho(&self, tipo: Tipo, id: &str) -> Res<PathBuf> {
        checar_id(id)?;
        Ok(self.raiz.join(tipo.pasta()).join(format!("{id}.{}", tipo.extensao())))
    }

    pub fn existe(&self, tipo: Tipo, id: &str) -> Res<bool> {
        Ok(self.caminho(tipo, id)?.is_file())
    }

    /// Um id ainda não usado, derivado do título.
    fn id_livre(&self, tipo: Tipo, titulo: &str) -> String {
        id_livre(&slug(titulo), |c| self.caminho(tipo, c).is_ok_and(|p| p.exists()))
    }

    /// Apaga o arquivo do item e o tira das arquivadas. Precisa da trava já tomada.
    fn apagar_arquivo(&self, tipo: Tipo, id: &str) -> Res<()> {
        match fs::remove_file(self.caminho(tipo, id)?) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(tipo.nao_existe(id)),
            outro => {
                outro?;
                self.esquecer_arquivada(tipo, id);
                Ok(())
            }
        }
    }

    /// Apaga o item: o arquivo, e numa avaliação também o que é dela (correções,
    /// submissões, pipeline, relatórios). As pastas vão antes, para uma falha no
    /// meio deixar a avaliação ainda lá, e o apagar poder ser repetido.
    pub fn apagar(&self, tipo: Tipo, id: &str) -> Res<()> {
        let _g = self.travar();
        match tipo {
            Tipo::Rubricas => {}
            Tipo::Turmas => {
                let em_uso: Vec<String> = self.listar_avaliacoes()?.into_iter().filter(|a| a.turma == id).map(|a| a.titulo).collect();
                if !em_uso.is_empty() {
                    return Err(Erro::Conflito(format!("A turma está em uso por: {}", em_uso.join(", "))));
                }
            }
            Tipo::Avaliacoes => {
                if !self.existe(tipo, id)? {
                    return Err(tipo.nao_existe(id));
                }
                let pastas = [self.pasta_correcoes(id)?, self.pasta_submissoes(id)?, self.pasta_pipeline(id)?, self.pasta_relatorios(id)?];
                for pasta in pastas {
                    match fs::remove_dir_all(pasta) {
                        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                        _ => {}
                    }
                }
            }
        }
        self.apagar_arquivo(tipo, id)?;
        if tipo == Tipo::Avaliacoes {
            self.fila_mudou();
        }
        Ok(())
    }

    /// Ids (nomes de arquivo sem extensão) de uma subpasta, em ordem alfabética.
    fn ids(&self, dir: &Path, ext: &str) -> Res<Vec<String>> {
        let mut ids = Vec::new();
        let dir = match fs::read_dir(dir) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(ids),
            Err(e) => return Err(e.into()),
        };
        for entrada in dir {
            let caminho = entrada?.path();
            if caminho.extension().and_then(|e| e.to_str()) != Some(ext) || !caminho.is_file() {
                continue;
            }
            if let Some(id) = caminho.file_stem().and_then(|s| s.to_str())
                && id_valido(id) {
                    ids.push(id.to_string());
                }
        }
        ids.sort_by_key(|a| a.to_lowercase());
        Ok(ids)
    }

    // ----- arquivadas -----

    fn ler_arquivadas(&self) -> Res<Arquivadas> {
        match ler_opcional(&self.raiz.join("arquivadas.toml"))? {
            None => Ok(Arquivadas::default()),
            Some(t) => toml::from_str(&t).map_err(|e| Erro::Invalido(format!("arquivadas.toml: {}", erro_toml(&t, &e)))),
        }
    }

    /// Para mostrar: um arquivo estragado à mão vale como nada arquivado, em vez
    /// de derrubar as listas, a tela de avaliar e a fila. Arquivar pela tela mostra o erro.
    pub fn arquivadas(&self) -> Arquivadas {
        self.ler_arquivadas().unwrap_or_else(|e| {
            eprintln!("arquivadas.toml ignorado: {e:?}");
            Arquivadas::default()
        })
    }

    /// Arquiva (`true`) ou devolve às listas (`false`).
    pub fn arquivar(&self, tipo: Tipo, id: &str, arquivada: bool) -> Res<()> {
        let _g = self.travar();
        // Desarquivar não exige o arquivo: limpa um id que sumiu por fora.
        if arquivada && !self.existe(tipo, id)? {
            return Err(Erro::NaoEncontrado(format!("“{id}” não existe em {}/", tipo.pasta())));
        }
        self.marcar_arquivada(tipo, id, arquivada)?;
        if tipo == Tipo::Avaliacoes {
            self.fila_mudou();
        }
        Ok(())
    }

    /// Ao apagar: o item já se foi, e um `arquivadas.toml` ruim não deve virar erro do apagar.
    fn esquecer_arquivada(&self, tipo: Tipo, id: &str) {
        if let Err(e) = self.marcar_arquivada(tipo, id, false) {
            eprintln!("{}/{id} apagado, mas continua em arquivadas.toml: {e:?}", tipo.pasta());
        }
    }

    /// Precisa da trava já tomada.
    fn marcar_arquivada(&self, tipo: Tipo, id: &str, arquivada: bool) -> Res<()> {
        let mut a = self.ler_arquivadas()?;
        let ids = a.ids_mut(tipo);
        let mudou = if arquivada { ids.insert(id.to_string()) } else { ids.remove(id) };
        if mudou {
            let texto = toml::to_string_pretty(&a).map_err(|e| Erro::Invalido(e.to_string()))?;
            gravar(&self.raiz.join("arquivadas.toml"), texto.as_bytes())?;
        }
        Ok(())
    }

    // ----- rubricas -----

    pub fn ler_rubrica(&self, id: &str) -> Res<Rubrica> {
        let texto = ler_opcional(&self.caminho(Tipo::Rubricas, id)?)?.ok_or_else(|| Tipo::Rubricas.nao_existe(id))?;
        let mut r: Rubrica =
            toml::from_str(&texto).map_err(|e| Erro::Invalido(format!("rubricas/{id}.toml: {}", e.message())))?;
        r.id = id.to_string();
        r.normalizar();
        r.validar().map_err(|e| Erro::Invalido(format!("rubricas/{id}.toml: {e}")))?;
        Ok(r)
    }

    /// Rubricas legíveis e, à parte, os arquivos com problema (para avisar na tela).
    pub fn listar_rubricas(&self) -> Res<(Vec<Rubrica>, Vec<String>)> {
        let (mut boas, mut problemas) = (Vec::new(), Vec::new());
        for id in self.ids(&self.raiz.join("rubricas"), "toml")? {
            // Um arquivo ruim vira aviso; nunca derruba a lista inteira.
            match self.ler_rubrica(&id) {
                Ok(r) => boas.push(r),
                Err(Erro::Invalido(m)) => problemas.push(m),
                Err(Erro::Io(e)) => problemas.push(format!("rubricas/{id}.toml: {e}")),
                Err(e) => return Err(e),
            }
        }
        Ok((boas, problemas))
    }

    /// Grava a rubrica. Sem `id`, cria um arquivo novo com nome derivado do título.
    pub fn salvar_rubrica(&self, id: Option<&str>, mut r: Rubrica) -> Res<Rubrica> {
        let _g = self.travar();
        r.normalizar();
        r.validar().map_err(Erro::Invalido)?;
        let id = match id {
            Some(id) => id.to_string(),
            None => self.id_livre(Tipo::Rubricas, &r.titulo),
        };
        let caminho = self.caminho(Tipo::Rubricas, &id)?;
        r.id.clear();
        let texto = toml::to_string_pretty(&r).map_err(|e| Erro::Invalido(e.to_string()))?;
        gravar(&caminho, texto.as_bytes())?;
        r.id = id;
        Ok(r)
    }

    /// A rubrica como TOML enxuto (sem os ids que saem dos nomes).
    pub fn rubrica_toml(&self, id: &str) -> Res<String> {
        Ok(self.ler_rubrica(id)?.toml_enxuto())
    }

    /// Grava a rubrica a partir de um TOML inteiro, criando o arquivo se não
    /// existe. Itens sem id herdam o do item de mesmo nome na versão gravada.
    pub fn salvar_rubrica_toml(&self, id: &str, texto: &str) -> Res<Rubrica> {
        checar_id(id)?;
        let mut r: Rubrica = toml::from_str(texto).map_err(|e| Erro::Invalido(erro_toml(texto, &e)))?;
        match self.ler_rubrica(id) {
            Ok(anterior) => r.herdar_ids(&anterior),
            Err(Erro::NaoEncontrado(_) | Erro::Invalido(_)) => {}
            Err(e) => return Err(e),
        }
        self.salvar_rubrica(Some(id), r)
    }

    // ----- turmas -----

    pub fn listar_turmas(&self) -> Res<Vec<Turma>> {
        let mut turmas = Vec::new();
        for id in self.ids(&self.raiz.join("turmas"), "csv")? {
            match self.ler_turma(&id) {
                Ok(t) => turmas.push(t),
                Err(Erro::Invalido(_) | Erro::Io(_)) => turmas.push(Turma { id, alunos: Vec::new() }),
                Err(e) => return Err(e),
            }
        }
        Ok(turmas)
    }

    pub fn ler_turma(&self, id: &str) -> Res<Turma> {
        let bytes = match fs::read(self.caminho(Tipo::Turmas, id)?) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Tipo::Turmas.nao_existe(id)),
            Err(e) => return Err(e.into()),
        };
        // Planilhas antigas exportam em Latin-1; cada byte vira o caractere de mesmo código.
        let texto = String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());
        let alunos = ler_alunos(&texto).map_err(|e| Erro::Invalido(format!("turmas/{id}.csv: {e}")))?;
        Ok(Turma { id: id.to_string(), alunos })
    }

    /// Cria a turma; recusa se o nome já existe.
    pub fn criar_turma(&self, id: &str, csv: &str) -> Res<Turma> {
        let _g = self.travar();
        if self.existe(Tipo::Turmas, id)? {
            return Err(Erro::Conflito(format!("Já existe uma turma chamada “{id}”")));
        }
        self.gravar_turma(id, csv)
    }

    /// Troca a lista de alunos de uma turma que existe.
    pub fn substituir_turma(&self, id: &str, csv: &str) -> Res<Turma> {
        let _g = self.travar();
        if !self.existe(Tipo::Turmas, id)? {
            return Err(Tipo::Turmas.nao_existe(id));
        }
        self.gravar_turma(id, csv)
    }

    fn gravar_turma(&self, id: &str, csv: &str) -> Res<Turma> {
        let alunos = ler_alunos(csv).map_err(Erro::Invalido)?;
        gravar(&self.caminho(Tipo::Turmas, id)?, escrever_alunos(&alunos).as_bytes())?;
        Ok(Turma { id: id.to_string(), alunos })
    }

    // ----- avaliações -----

    pub fn ler_avaliacao(&self, id: &str) -> Res<Avaliacao> {
        let texto = ler_opcional(&self.caminho(Tipo::Avaliacoes, id)?)?.ok_or_else(|| Tipo::Avaliacoes.nao_existe(id))?;
        let mut a: Avaliacao =
            serde_json::from_str(&texto).map_err(|e| Erro::Invalido(format!("avaliacoes/{id}.json: {e}")))?;
        a.id = id.to_string();
        Ok(a)
    }

    pub fn listar_avaliacoes(&self) -> Res<Vec<Avaliacao>> {
        let mut todas = Vec::new();
        for id in self.ids(&self.raiz.join("avaliacoes"), "json")? {
            match self.ler_avaliacao(&id) {
                Ok(a) => todas.push(a),
                Err(Erro::Invalido(_) | Erro::Io(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(todas)
    }

    /// Cria ou atualiza. Cada rubrica é copiada da pasta na primeira vez; depois a
    /// cópia só é refeita se `atualizar` contiver sua origem.
    #[allow(clippy::too_many_arguments)] // um parâmetro por campo do formulário
    pub fn salvar_avaliacao(
        &self,
        id: Option<&str>,
        titulo: String,
        turma: String,
        nota_maxima: f64,
        rubricas: Vec<(String, f64)>,
        atualizar: &[String],
        efeitos: &EscolhaEfeitos,
    ) -> Res<Avaliacao> {
        // Tudo sob a trava: a turma e as rubricas lidas não somem antes de gravar.
        let _g = self.travar();
        // O que a avaliação cita e não existe é erro do pedido, não da rota.
        let citado = |e| match e {
            Erro::NaoEncontrado(m) => Erro::Invalido(m),
            e => e,
        };
        self.ler_turma(&turma).map_err(citado)?;
        let anterior = match id {
            Some(id) => Some(self.ler_avaliacao(id)?),
            None => None,
        };
        let mut aplicadas = Vec::new();
        for (origem, peso) in rubricas {
            let copia = anterior
                .as_ref()
                .filter(|_| !atualizar.contains(&origem))
                .and_then(|a| a.rubrica(&origem))
                .map(|r| r.rubrica.clone());
            let rubrica = match copia {
                Some(r) => r,
                None => self.ler_rubrica(&origem).map_err(citado)?,
            };
            aplicadas.push(RubricaAplicada { origem, peso, rubrica });
        }
        let mut tipos = Vec::new();
        for id in &efeitos.ids {
            let copia = anterior.as_ref().filter(|_| !efeitos.atualizar.contains(id)).and_then(|a| a.efeito(id)).cloned();
            let tipo = match copia {
                Some(t) => t,
                None => self.ler_efeito(id).map_err(citado)?,
            };
            tipos.push(tipo);
        }
        let prazo = efeitos.prazo.clone().filter(|p| !p.trim().is_empty());
        let mut a = Avaliacao {
            id: String::new(),
            titulo,
            turma,
            nota_maxima,
            rubricas: aplicadas,
            efeitos: tipos,
            prazo,
            tolerancia: efeitos.tolerancia,
        };
        a.validar().map_err(Erro::Invalido)?;

        let id = match id {
            Some(id) => id.to_string(),
            None => self.id_livre(Tipo::Avaliacoes, &a.titulo),
        };
        gravar(&self.caminho(Tipo::Avaliacoes, &id)?, serde_json::to_string_pretty(&a).unwrap().as_bytes())?;
        a.id = id;
        // Rubricas atualizadas podem mudar as sugestões do pipeline.
        self.fila_mudou();
        Ok(a)
    }

    /// Origens cuja rubrica na pasta difere da cópia guardada na avaliação.
    pub fn rubricas_desatualizadas(&self, a: &Avaliacao) -> Vec<String> {
        a.rubricas
            .iter()
            .filter(|ra| matches!(self.ler_rubrica(&ra.origem), Ok(atual) if atual != ra.rubrica))
            .map(|ra| ra.origem.clone())
            .collect()
    }

    /// Ids dos tipos de efeito cujo arquivo no catálogo difere da cópia da avaliação.
    pub fn efeitos_desatualizados(&self, a: &Avaliacao) -> Vec<String> {
        a.efeitos.iter().filter(|t| matches!(self.ler_efeito(&t.id), Ok(atual) if atual != **t)).map(|t| t.id.clone()).collect()
    }

    // ----- tipos de efeito -----

    fn caminho_efeito(&self, id: &str) -> Res<PathBuf> {
        checar_id(id)?;
        Ok(self.raiz.join("efeitos").join(format!("{id}.toml")))
    }

    pub fn ler_efeito(&self, id: &str) -> Res<TipoEfeito> {
        let texto = ler_opcional(&self.caminho_efeito(id)?)?
            .ok_or_else(|| Erro::NaoEncontrado(format!("Tipo de efeito “{id}” não existe")))?;
        let mut t: TipoEfeito = toml::from_str(&texto).map_err(|e| Erro::Invalido(format!("efeitos/{id}.toml: {}", erro_toml(&texto, &e))))?;
        t.id = id.to_string();
        t.validar().map_err(|e| Erro::Invalido(format!("efeitos/{id}.toml: {e}")))?;
        Ok(t)
    }

    /// Tipos legíveis e, à parte, os arquivos com problema.
    pub fn listar_efeitos(&self) -> Res<(Vec<TipoEfeito>, Vec<String>)> {
        let (mut bons, mut problemas) = (Vec::new(), Vec::new());
        for id in self.ids(&self.raiz.join("efeitos"), "toml")? {
            match self.ler_efeito(&id) {
                Ok(t) => bons.push(t),
                Err(Erro::Invalido(m)) => problemas.push(m),
                Err(Erro::Io(e)) => problemas.push(format!("efeitos/{id}.toml: {e}")),
                Err(e) => return Err(e),
            }
        }
        Ok((bons, problemas))
    }

    /// O arquivo como está no disco.
    pub fn efeito_toml(&self, id: &str) -> Res<String> {
        let mut t = self.ler_efeito(id)?;
        t.id.clear();
        toml::to_string_pretty(&t).map_err(|e| Erro::Invalido(e.to_string()))
    }

    /// Valida e grava (ou cria) o tipo a partir do TOML inteiro.
    pub fn salvar_efeito_toml(&self, id: &str, texto: &str) -> Res<TipoEfeito> {
        let caminho = self.caminho_efeito(id)?;
        let mut t: TipoEfeito = toml::from_str(texto).map_err(|e| Erro::Invalido(erro_toml(texto, &e)))?;
        t.id.clear();
        t.validar().map_err(Erro::Invalido)?;
        let _g = self.travar();
        let texto = toml::to_string_pretty(&t).map_err(|e| Erro::Invalido(e.to_string()))?;
        gravar(&caminho, texto.as_bytes())?;
        t.id = id.to_string();
        Ok(t)
    }

    /// Apaga do catálogo. As avaliações que o usam continuam com a cópia delas.
    pub fn apagar_efeito(&self, id: &str) -> Res<()> {
        let _g = self.travar();
        match fs::remove_file(self.caminho_efeito(id)?) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Erro::NaoEncontrado(format!("Tipo de efeito “{id}” não existe"))),
            outro => Ok(outro?),
        }
    }

    // ----- correções -----

    fn pasta_correcoes(&self, avaliacao: &str) -> Res<PathBuf> {
        checar_id(avaliacao)?;
        Ok(self.raiz.join("correcoes").join(avaliacao))
    }

    fn caminho_correcao(&self, avaliacao: &str, aluno: &str) -> Res<PathBuf> {
        checar_id(aluno)?;
        Ok(self.pasta_correcoes(avaliacao)?.join(format!("{aluno}.json")))
    }

    pub fn ler_correcao(&self, avaliacao: &str, aluno: &str) -> Res<Option<Correcao>> {
        let caminho = self.caminho_correcao(avaliacao, aluno)?;
        match ler_opcional(&caminho)? {
            None => Ok(None),
            Some(t) => serde_json::from_str(&t)
                .map(Some)
                .map_err(|e| Erro::Invalido(format!("correcoes/{avaliacao}/{aluno}.json: {e}"))),
        }
    }

    pub fn ler_correcoes(&self, avaliacao: &str) -> Res<BTreeMap<String, Correcao>> {
        let mut todas = BTreeMap::new();
        for aluno in self.ids(&self.pasta_correcoes(avaliacao)?, "json")? {
            if let Some(c) = self.ler_correcao(avaliacao, &aluno)? {
                todas.insert(aluno, c);
            }
        }
        Ok(todas)
    }

    /// Aplica alterações por critério (`None` apaga a marcação) sobre o que já está
    /// gravado, de modo que dois avaliadores no mesmo aluno não se sobrescrevem.
    /// Marcações que não cabem na rubrica são devolvidas como recusadas; as demais
    /// são gravadas mesmo assim.
    #[allow(clippy::too_many_arguments)] // um parâmetro por parte do pedido
    pub fn alterar_correcao(
        &self,
        avaliacao: &str,
        aluno: &str,
        respostas: Vec<(String, String, Option<Resposta>)>,
        comentario_geral: Option<String>,
        efeitos: AlteracaoEfeitos,
        avaliador: &str,
        agora: u64,
    ) -> Res<(Correcao, Vec<Recusada>)> {
        let av = self.ler_avaliacao(avaliacao)?;
        let turma = self.ler_turma(&av.turma)?;
        if !turma.alunos.iter().any(|a| a.id == aluno) {
            return Err(Erro::NaoEncontrado(format!("O aluno “{aluno}” não está na turma {}", turma.id)));
        }
        let mut aceitas = Vec::new();
        let mut recusadas = Vec::new();
        for (origem, criterio, resposta) in respostas {
            let motivo = match av.rubrica(&origem).and_then(|r| r.rubrica.criterio(&criterio)) {
                None => Some(format!("o critério “{criterio}” não existe mais na rubrica “{origem}”")),
                Some(c) => resposta.as_ref().and_then(|r| c.pontos(&r.marcacao).err()),
            };
            match motivo {
                Some(motivo) => recusadas.push(Recusada { rubrica: origem, criterio, motivo }),
                None => aceitas.push((origem, criterio, resposta)),
            }
        }
        // Efeitos recusados vêm com `rubrica` vazia e o id do tipo em `criterio`.
        let mut recusar = |id: &str, motivo: String| recusadas.push(Recusada { rubrica: String::new(), criterio: id.to_string(), motivo });
        let mut efeitos_aceitos = Vec::new();
        for (id, aplicado) in efeitos.efeitos {
            match (av.efeito(&id), aplicado) {
                (None, _) => recusar(&id, format!("o efeito “{id}” não existe mais nesta avaliação")),
                (Some(t), Some(a)) => match t.conferir(&a) {
                    Ok(()) => efeitos_aceitos.push((id, Some(EfeitoAplicado { por: avaliador.to_string(), em: agora, ..a }))),
                    Err(m) => recusar(&id, m),
                },
                (Some(_), None) => efeitos_aceitos.push((id, None)),
            }
        }
        let mut dispensas_aceitas = Vec::new();
        for (id, d) in efeitos.dispensadas {
            match av.efeito(&id) {
                None => recusar(&id, format!("o efeito “{id}” não existe mais nesta avaliação")),
                Some(_) => dispensas_aceitas.push((id, d.map(|d| Dispensa { por: avaliador.to_string(), em: agora, ..d }))),
            }
        }

        let _g = self.travar();
        let mut c = self.ler_correcao(avaliacao, aluno)?.unwrap_or_default();
        if aceitas.is_empty() && comentario_geral.is_none() && efeitos_aceitos.is_empty() && dispensas_aceitas.is_empty() {
            return Ok((c, recusadas));
        }
        for (id, a) in efeitos_aceitos {
            match a {
                Some(a) => c.efeitos.insert(id, a),
                None => c.efeitos.remove(&id),
            };
        }
        for (id, d) in dispensas_aceitas {
            match d {
                Some(d) => c.dispensadas.insert(id, d),
                None => c.dispensadas.remove(&id),
            };
        }
        for (origem, criterio, resposta) in aceitas {
            match resposta {
                Some(r) => {
                    c.respostas.entry(origem).or_default().insert(criterio, r);
                }
                None => {
                    if let Some(m) = c.respostas.get_mut(&origem) {
                        m.remove(&criterio);
                    }
                }
            }
        }
        c.respostas.retain(|_, m| !m.is_empty());
        if let Some(texto) = comentario_geral {
            c.comentario_geral = texto;
        }
        c.avaliador = avaliador.to_string();
        c.atualizado_em = agora;
        let caminho = self.caminho_correcao(avaliacao, aluno)?;
        gravar(&caminho, serde_json::to_string_pretty(&c).unwrap().as_bytes())?;
        Ok((c, recusadas))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rubricas-teste-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    const RUBRICA: &str = r#"
titulo = "Código"

[[criterios]]
nome = "Compila"
escala = { tipo = "binaria", pontos = 1.0 }

[[criterios]]
nome = "Clareza"
[criterios.escala]
tipo = "niveis"
[[criterios.escala.niveis]]
rotulo = "Ruim"
pontos = 0.0
[[criterios.escala.niveis]]
rotulo = "Boa"
pontos = 1.0
"#;

    #[test]
    fn rubrica_como_texto_toml() {
        let raiz = pasta("texto");
        let d = Dados::abrir(&raiz).unwrap();
        // "Clareza" foi renomeado no editor web: o id antigo não sai mais do nome
        fs::write(raiz.join("rubricas/codigo.toml"), RUBRICA.replace("nome = \"Clareza\"", "id = \"legibilidade\"\nnome = \"Clareza\"")).unwrap();
        d.salvar_rubrica(Some("codigo"), d.ler_rubrica("codigo").unwrap()).unwrap();

        let texto = d.rubrica_toml("codigo").unwrap();
        assert!(texto.contains("id = \"legibilidade\""), "{texto}");
        assert_eq!(texto.matches("id =").count(), 1, "só o id que não sai do nome: {texto}");

        // quem reescreve o arquivo sem ids não muda a identidade dos itens
        let reescrito = texto.replace("id = \"legibilidade\"\n", "").replace("rotulo = \"Boa\"", "rotulo = \"Boa\"\ndescritor = \"Nomes claros.\"")
            + "\n[[criterios]]\ntipo = \"roteiro\"\nnome = \"Rode os testes\"\n";
        let r = d.salvar_rubrica_toml("codigo", &reescrito).unwrap();
        assert_eq!(r.criterios.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["compila", "legibilidade", "rode-os-testes"]);
        assert_eq!(d.ler_rubrica("codigo").unwrap(), r);

        // rubrica nova pelo mesmo caminho
        d.salvar_rubrica_toml("nova", RUBRICA).unwrap();
        assert_eq!(d.ler_rubrica("nova").unwrap().titulo, "Código");

        // erro de sintaxe diz onde
        let Err(Erro::Invalido(m)) = d.salvar_rubrica_toml("codigo", "titulo = \"x\"\n[[criterios]\n") else { panic!() };
        assert!(m.contains("linha 2"), "{m}");
        // erro de validação também é recusado e não toca no arquivo
        assert!(matches!(d.salvar_rubrica_toml("codigo", "titulo = \"x\"\n"), Err(Erro::Invalido(_))));
        assert_eq!(d.ler_rubrica("codigo").unwrap(), r);
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn efeitos_do_catalogo_na_avaliacao_e_na_correcao() {
        let raiz = pasta("efeitos");
        let d = Dados::abrir(&raiz).unwrap();
        fs::write(raiz.join("rubricas/codigo.toml"), RUBRICA).unwrap();
        d.criar_turma("Turma D", "1,Ana\n2,Bruno\n").unwrap();
        let atraso = "titulo = \"Atraso\"\nsugerir = \"atraso\"\noperacao = { tipo = \"por_unidade\", pontos = -1.0 }\n";
        d.salvar_efeito_toml("atraso", atraso).unwrap();
        d.salvar_efeito_toml("plagio", "titulo = \"Plágio\"\nencerra = true\noperacao = { tipo = \"fixar\", nota = 0 }\n").unwrap();
        let Err(Erro::Invalido(m)) = d.salvar_efeito_toml("ruim", "titulo = \"x\"\noperacao = { tipo = \"girar\" }\n") else { panic!() };
        assert!(m.contains("linha 2"), "{m}");
        let volta: TipoEfeito = toml::from_str(&d.efeito_toml("atraso").unwrap()).unwrap();
        assert_eq!(TipoEfeito { id: "atraso".into(), ..volta }, d.ler_efeito("atraso").unwrap());
        let (tipos, problemas) = d.listar_efeitos().unwrap();
        assert_eq!((tipos.len(), problemas.len()), (2, 0));

        let escolha = EscolhaEfeitos { ids: vec!["atraso".into(), "plagio".into()], prazo: Some("2026-10-06T23:59".into()), tolerancia: 10, atualizar: vec![] };
        let av = d.salvar_avaliacao(None, "EP".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &escolha).unwrap();
        assert_eq!(av.efeitos.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["atraso", "plagio"]);
        assert_eq!((av.prazo.as_deref(), av.tolerancia), (Some("2026-10-06T23:59"), 10));
        let ruim = EscolhaEfeitos { prazo: Some("amanhã".into()), ..Default::default() };
        assert!(matches!(d.salvar_avaliacao(Some("ep"), "EP".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &ruim), Err(Erro::Invalido(_))));

        // mudar o tipo no catálogo não muda a cópia; atualizar a pedido, sim
        d.salvar_efeito_toml("atraso", &atraso.replace("-1.0", "-2.0")).unwrap();
        assert_eq!(d.efeitos_desatualizados(&d.ler_avaliacao("ep").unwrap()), ["atraso"]);
        let marca = Some(Resposta { marcacao: Marcacao::Binaria { atende: true }, comentario: String::new() });
        let um_dia = EfeitoAplicado { quantidade: Some(1.0), ..Default::default() };
        let efeitos = AlteracaoEfeitos {
            efeitos: vec![("atraso".into(), Some(um_dia.clone())), ("sumiu".into(), Some(um_dia.clone())), ("plagio".into(), None)],
            dispensadas: vec![("plagio".into(), Some(Dispensa { sinal: 0.8, ..Default::default() }))],
        };
        let (c, recusadas) = d.alterar_correcao("ep", "1", vec![("codigo".into(), "compila".into(), marca)], None, efeitos, "vini", 5).unwrap();
        assert_eq!(recusadas.iter().map(|r| r.criterio.as_str()).collect::<Vec<_>>(), ["sumiu"]);
        assert_eq!((c.efeitos["atraso"].por.as_str(), c.efeitos["atraso"].em), ("vini", 5));
        assert_eq!((c.dispensadas["plagio"].sinal, c.dispensadas["plagio"].por.as_str()), (0.8, "vini"));
        let av = d.ler_avaliacao("ep").unwrap();
        // 1 de 2 pontos = 5, menos 1 dia de atraso com o desconto da cópia (−1)
        assert_eq!(calcular(&av, Some(&c)).nota, 4.0);
        let av = d
            .salvar_avaliacao(Some("ep"), "EP".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &EscolhaEfeitos { atualizar: vec!["atraso".into()], ..escolha })
            .unwrap();
        assert_eq!(calcular(&av, Some(&c)).nota, 3.0);

        // um efeito que não combina com o tipo é recusado; o de encerrar conclui sem marcar nada
        let sem_quantidade = AlteracaoEfeitos { efeitos: vec![("atraso".into(), Some(EfeitoAplicado::default()))], ..Default::default() };
        let (_, recusadas) = d.alterar_correcao("ep", "2", vec![], None, sem_quantidade, "vini", 6).unwrap();
        assert!(recusadas[0].motivo.contains("quantidade"), "{recusadas:?}");
        let zero = AlteracaoEfeitos { efeitos: vec![("plagio".into(), Some(EfeitoAplicado::default()))], ..Default::default() };
        let (c, _) = d.alterar_correcao("ep", "2", vec![], None, zero, "vini", 7).unwrap();
        let r = calcular(&av, Some(&c));
        assert_eq!((r.nota, r.respondidos, r.concluida()), (0.0, 0, true));
        // tirar o efeito
        let tirar = AlteracaoEfeitos { efeitos: vec![("plagio".into(), None)], ..Default::default() };
        let (c, _) = d.alterar_correcao("ep", "2", vec![], None, tirar, "vini", 8).unwrap();
        assert!(c.efeitos.is_empty());

        // o catálogo perde o tipo, a avaliação continua com a cópia
        d.apagar_efeito("plagio").unwrap();
        assert!(d.ler_avaliacao("ep").unwrap().efeito("plagio").is_some());
        assert!(matches!(d.apagar_efeito("plagio"), Err(Erro::NaoEncontrado(_))));
        assert!(matches!(d.ler_efeito("../usuarios"), Err(Erro::Invalido(_))));
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn arquivar_e_desarquivar() {
        let raiz = pasta("arquivar");
        let d = Dados::abrir(&raiz).unwrap();
        fs::write(raiz.join("rubricas/codigo.toml"), RUBRICA).unwrap();
        d.criar_turma("Turma D", "1,Ana\n").unwrap();
        d.salvar_avaliacao(None, "TF".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &Default::default()).unwrap();

        d.arquivar(Tipo::Rubricas, "codigo", true).unwrap();
        d.arquivar(Tipo::Turmas, "Turma D", true).unwrap();
        d.arquivar(Tipo::Avaliacoes, "tf", true).unwrap();
        let a = d.arquivadas();
        assert!(a.contem(Tipo::Rubricas, "codigo") && a.contem(Tipo::Turmas, "Turma D") && a.contem(Tipo::Avaliacoes, "tf"));
        assert!(!a.contem(Tipo::Turmas, "codigo"));
        // um arquivo só, legível e editável à mão
        assert!(fs::read_to_string(raiz.join("arquivadas.toml")).unwrap().contains("\"Turma D\""));

        // arquivar de novo não duplica; desarquivar devolve
        d.arquivar(Tipo::Turmas, "Turma D", true).unwrap();
        d.arquivar(Tipo::Turmas, "Turma D", false).unwrap();
        assert!(!d.arquivadas().contem(Tipo::Turmas, "Turma D"));

        // o que não existe não se arquiva
        assert!(matches!(d.arquivar(Tipo::Rubricas, "sumiu", true), Err(Erro::NaoEncontrado(_))));
        assert!(matches!(d.arquivar(Tipo::Rubricas, "../usuarios", true), Err(Erro::Invalido(_))));

        // apagar tira do arquivo: recriar com o mesmo nome não nasce arquivado
        d.arquivar(Tipo::Turmas, "Turma D", true).unwrap();
        d.apagar(Tipo::Avaliacoes, "tf").unwrap();
        d.apagar(Tipo::Turmas, "Turma D").unwrap();
        d.apagar(Tipo::Rubricas, "codigo").unwrap();
        let a = d.arquivadas();
        assert!(!a.contem(Tipo::Rubricas, "codigo") && !a.contem(Tipo::Turmas, "Turma D") && !a.contem(Tipo::Avaliacoes, "tf"));

        // um id cujo arquivo sumiu por fora ainda pode ser desarquivado
        fs::write(raiz.join("arquivadas.toml"), "turmas = [\"sumiu\"]\n").unwrap();
        d.arquivar(Tipo::Turmas, "sumiu", false).unwrap();
        assert!(!d.arquivadas().contem(Tipo::Turmas, "sumiu"));

        // o arquivo estragado à mão não derruba a leitura (nada arquivado) nem o apagar;
        // só gravar por cima dele é recusado, para não perder o que estava lá
        d.criar_turma("Turma E", "1,Ana\n").unwrap();
        fs::write(raiz.join("arquivadas.toml"), "turmas = [").unwrap();
        assert!(!d.arquivadas().contem(Tipo::Turmas, "Turma E"));
        assert!(matches!(d.arquivar(Tipo::Turmas, "Turma E", true), Err(Erro::Invalido(_))));
        d.apagar(Tipo::Turmas, "Turma E").unwrap();
        assert_eq!(fs::read_to_string(raiz.join("arquivadas.toml")).unwrap(), "turmas = [");
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn fluxo_completo_na_pasta() {
        let raiz = pasta("fluxo");
        let d = Dados::abrir(&raiz).unwrap();
        // um arquivo escrito à mão, sem ids
        fs::write(raiz.join("rubricas/codigo.toml"), RUBRICA).unwrap();
        fs::write(raiz.join("rubricas/quebrada.toml"), "titulo = ").unwrap();
        fs::write(raiz.join("rubricas/latin1.toml"), b"titulo = \"Apresenta\xe7\xe3o\"\n").unwrap();
        fs::create_dir(raiz.join("rubricas/pasta.toml")).unwrap();
        let (boas, problemas) = d.listar_rubricas().unwrap();
        assert_eq!(boas.len(), 1);
        assert_eq!(boas[0].criterios[1].id, "clareza");
        assert_eq!(problemas.len(), 2, "{problemas:?}");
        assert!(problemas.iter().any(|p| p.contains("quebrada.toml")));
        assert!(problemas.iter().any(|p| p.contains("latin1.toml") && p.contains("UTF-8")));

        // criar pela API dá um arquivo novo, com nome derivado do título
        let mut nova = boas[0].clone();
        nova.titulo = "Código".into();
        let nova = d.salvar_rubrica(None, nova).unwrap();
        assert_eq!(nova.id, "codigo-2");
        assert_eq!(d.ler_rubrica("codigo-2").unwrap(), nova);

        d.criar_turma("Turma D", "matricula;nome\n1;Ana\n2;Bruno\n").unwrap();
        assert!(matches!(d.criar_turma("Turma D", "x"), Err(Erro::Conflito(_))));
        assert_eq!(fs::read_to_string(raiz.join("turmas/Turma D.csv")).unwrap(), "matricula,nome\n1,Ana\n2,Bruno\n");

        let av = d
            .salvar_avaliacao(None, "TF".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &Default::default())
            .unwrap();
        assert_eq!(av.id, "tf");
        assert!(matches!(d.apagar(Tipo::Turmas, "Turma D"), Err(Erro::Conflito(_))));

        // editar a rubrica na pasta não muda a cópia da avaliação, só a marca como desatualizada
        let mut editada = d.ler_rubrica("codigo").unwrap();
        editada.criterios.pop();
        d.salvar_rubrica(Some("codigo"), editada).unwrap();
        let av = d.ler_avaliacao("tf").unwrap();
        assert_eq!(av.rubricas[0].rubrica.criterios.len(), 2);
        assert_eq!(d.rubricas_desatualizadas(&av), ["codigo"]);

        let marca = |m| Some(Resposta { marcacao: m, comentario: String::new() });
        let (c, recusadas) = d
            .alterar_correcao(
                "tf",
                "1",
                vec![("codigo".into(), "compila".into(), marca(Marcacao::Binaria { atende: true }))],
                None,
                Default::default(),
                "vini",
                1,
            )
            .unwrap();
        assert!(recusadas.is_empty());
        assert_eq!(c.respostas["codigo"].len(), 1);
        // outra alteração no mesmo aluno soma, não substitui
        let (c, _) = d
            .alterar_correcao(
                "tf",
                "1",
                vec![("codigo".into(), "clareza".into(), marca(Marcacao::Nivel { nivel: "boa".into() }))],
                Some("Bom trabalho".into()),
                Default::default(),
                "gustavo",
                2,
            )
            .unwrap();
        assert_eq!(c.respostas["codigo"].len(), 2);
        assert_eq!(calcular(&av, Some(&c)).nota, 10.0);
        // apagar uma marcação
        let (c, _) = d.alterar_correcao("tf", "1", vec![("codigo".into(), "compila".into(), None)], None, Default::default(), "vini", 3).unwrap();
        assert_eq!(c.respostas["codigo"].len(), 1);
        assert_eq!(c.comentario_geral, "Bom trabalho");

        // uma marcação que não cabe é recusada, mas as outras do mesmo lote são gravadas
        let (c, recusadas) = d
            .alterar_correcao(
                "tf",
                "1",
                vec![
                    ("codigo".into(), "compila".into(), marca(Marcacao::Nota { nota: 1.0 })),
                    ("codigo".into(), "sumiu".into(), marca(Marcacao::Binaria { atende: true })),
                    ("codigo".into(), "clareza".into(), marca(Marcacao::Nivel { nivel: "ruim".into() })),
                ],
                None,
                Default::default(),
                "vini",
                4,
            )
            .unwrap();
        assert_eq!(recusadas.iter().map(|r| r.criterio.as_str()).collect::<Vec<_>>(), ["compila", "sumiu"]);
        assert_eq!(c.respostas["codigo"]["clareza"].marcacao, Marcacao::Nivel { nivel: "ruim".into() });
        assert_eq!(d.ler_correcao("tf", "1").unwrap().unwrap(), c);
        assert!(matches!(d.alterar_correcao("tf", "99", vec![], None, Default::default(), "vini", 4), Err(Erro::NaoEncontrado(_))));
        assert!(matches!(d.ler_rubrica("../usuarios"), Err(Erro::Invalido(_))));

        // atualizar a cópia a pedido
        let av = d
            .salvar_avaliacao(Some("tf"), "TF".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &["codigo".into()], &Default::default())
            .unwrap();
        assert_eq!(av.rubricas[0].rubrica.criterios.len(), 1);
        assert!(d.rubricas_desatualizadas(&av).is_empty());

        d.apagar(Tipo::Avaliacoes, "tf").unwrap();
        assert!(!raiz.join("correcoes/tf").exists());
        d.apagar(Tipo::Turmas, "Turma D").unwrap();
        fs::remove_dir_all(&raiz).unwrap();
    }
}
