//! Submissões do Moodle: o ZIP de "Baixar todas as submissões" vira uma pasta
//! por aluno.
//!
//! ```text
//! dados/submissoes/<avaliacao>/<aluno>/...        arquivos enviados pelo aluno
//! dados/submissoes/<avaliacao>/.sem-dono/<nome>/  nomes que não casaram com a turma
//! dados/submissoes/<avaliacao>/.indice.json       hash e data de cada submissão
//! dados/apelidos/<turma>.json                     nome no Moodle → aluno (associação manual)
//! ```

use crate::dados::{Dados, Erro, Res, checar_id, gravar, ler_json, ler_ou};
use crate::modelo::{Turma, caminho_seguro, slug};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// O que uma importação fez, para mostrar a quem enviou o ZIP.
#[derive(Debug, Default, Serialize)]
pub struct Importacao {
    /// Submissões novas ou com conteúdo diferente do que estava guardado.
    pub recebidas: usize,
    /// Submissões iguais às guardadas.
    pub iguais: usize,
    /// Nomes do Moodle que não casaram com ninguém da turma.
    pub sem_dono: Vec<String>,
    /// Entradas do ZIP fora do padrão do Moodle.
    pub ignorados: Vec<String>,
    /// Alunos com código novo cujas edições do professor foram descartadas.
    pub edicoes_descartadas: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfoArquivo {
    pub caminho: String,
    pub tamanho: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Submissao {
    pub nome_moodle: String,
    pub hash: String,
    /// Quando o conteúdo mudou pela última vez (não muda com um reenvio igual).
    pub alterada_em: u64,
    pub arquivos: Vec<InfoArquivo>,
    /// Já havia marcações e a submissão mudou depois delas.
    pub mudou_depois_da_correcao: bool,
    /// Resumo do que as checagens veem: o original com as edições por cima.
    pub hash_checagem: String,
    pub edicao: Option<Edicao>,
}

/// Mudanças pontuais do professor sobre o código do aluno (o original fica guardado).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edicao {
    pub por: String,
    pub em: u64,
    /// Arquivos editados.
    pub arquivos: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemDono {
    pub chave: String,
    pub nome_moodle: String,
    pub arquivos: Vec<InfoArquivo>,
}

/// Também o que o executor recebe de `GET /avaliacoes/{id}/submissoes`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Submissoes {
    pub alunos: BTreeMap<String, Submissao>,
    pub sem_dono: Vec<SemDono>,
}

/// Arquivos de uma submissão: caminho relativo (com `/`) → conteúdo.
pub(crate) type Arquivos = BTreeMap<String, Vec<u8>>;

const SEM_DONO: &str = ".sem-dono";
const INDICE: &str = ".indice.json";
const EDICOES: &str = ".edicoes";
const MAX_TOTAL: u64 = 300 << 20;
const MAX_ARQUIVO: u64 = 20 << 20;
const MAX_ENTRADAS: usize = 20_000;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Indice {
    #[serde(default)]
    alunos: BTreeMap<String, Registro>,
    #[serde(default)]
    sem_dono: BTreeMap<String, Registro>,
    #[serde(default)]
    edicoes: BTreeMap<String, RegistroEdicao>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistroEdicao {
    por: String,
    em: u64,
    /// Resumo do original com as edições por cima.
    hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Registro {
    nome_moodle: String,
    hash: String,
    alterada_em: u64,
}

/// `Nome Sobrenome_12345_assignsubmission_file_/pasta/a.py` → ("Nome Sobrenome", "pasta/a.py").
/// Também aceita o formato sem pastas (`…_assignsubmission_file_a.py`) e texto online.
fn separar(entrada: &str) -> Option<(String, String)> {
    let (prefixo, resto) = entrada.split_once("_assignsubmission_")?;
    let (nome, id) = prefixo.rsplit_once('_')?;
    if nome.trim().is_empty() || nome.contains('/') || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (_plugin, caminho) = resto.split_once('_')?;
    Some((nome.trim().to_string(), caminho.trim_start_matches('/').to_string()))
}

/// Lê o ZIP do Moodle: nome no Moodle → arquivos, mais as entradas ignoradas.
fn ler_zip(bytes: &[u8]) -> Res<(BTreeMap<String, Arquivos>, Vec<String>)> {
    use std::io::Read;
    let invalido = |m: String| Erro::Invalido(m);
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| invalido(format!("O arquivo não é um ZIP válido: {e}")))?;
    if z.len() > MAX_ENTRADAS {
        return Err(invalido(format!("O ZIP tem {} entradas; o limite é {MAX_ENTRADAS}", z.len())));
    }
    let (mut entregas, mut ignorados): (BTreeMap<String, Arquivos>, Vec<String>) = Default::default();
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| invalido(format!("ZIP corrompido: {e}")))?;
        let nome = f.name().map(|n| n.into_owned()).unwrap_or_default();
        if f.is_dir() || nome.starts_with("__MACOSX/") || nome.rsplit('/').next().is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        let Some((aluno, caminho)) = separar(&nome).filter(|(_, c)| caminho_seguro(c)) else {
            ignorados.push(nome);
            continue;
        };
        if f.size() > MAX_ARQUIVO {
            return Err(invalido(format!("{nome}: arquivo maior que {} MB", MAX_ARQUIVO >> 20)));
        }
        let mut conteudo = Vec::new();
        // O tamanho declarado pode mentir: a leitura também é limitada.
        (&mut f).take(MAX_ARQUIVO + 1).read_to_end(&mut conteudo).map_err(|e| invalido(format!("{nome}: {e}")))?;
        total += conteudo.len() as u64;
        if conteudo.len() as u64 > MAX_ARQUIVO || total > MAX_TOTAL {
            return Err(invalido(format!("O conteúdo do ZIP passa de {} MB", MAX_TOTAL >> 20)));
        }
        entregas.entry(aluno).or_default().insert(caminho, conteudo);
    }
    Ok((entregas, ignorados))
}

pub(crate) fn resumo(arquivos: &Arquivos) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for (caminho, conteudo) in arquivos {
        h.update(caminho.as_bytes());
        h.update([0]);
        h.update((conteudo.len() as u64).to_le_bytes());
        h.update(conteudo);
    }
    h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// Arquivos de uma pasta, recursivamente, em ordem.
pub(crate) fn listar(dir: &Path) -> Vec<InfoArquivo> {
    fn andar(dir: &Path, prefixo: &str, saida: &mut Vec<InfoArquivo>) {
        let Ok(entradas) = fs::read_dir(dir) else { return };
        for e in entradas.flatten() {
            let nome = e.file_name().to_string_lossy().into_owned();
            let caminho = if prefixo.is_empty() { nome.clone() } else { format!("{prefixo}/{nome}") };
            match e.file_type() {
                Ok(t) if t.is_dir() => andar(&e.path(), &caminho, saida),
                Ok(t) if t.is_file() => {
                    saida.push(InfoArquivo { caminho, tamanho: e.metadata().map(|m| m.len()).unwrap_or(0) })
                }
                _ => {}
            }
        }
    }
    let mut saida = Vec::new();
    andar(dir, "", &mut saida);
    saida.sort_by(|a, b| a.caminho.cmp(&b.caminho));
    saida
}

fn remover_pasta(pasta: &Path) -> io::Result<()> {
    match fs::remove_dir_all(pasta) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Troca `destino` por `nova` com dois renomes: a pasta do aluno nunca fica
/// pela metade nem some enquanto alguém lê.
fn trocar_pasta(nova: &Path, destino: &Path) -> io::Result<()> {
    let nome = destino.file_name().unwrap_or_default().to_string_lossy();
    let velha = destino.with_file_name(format!(".velha-{nome}"));
    remover_pasta(&velha)?;
    if destino.exists() {
        fs::rename(destino, &velha)?;
    }
    fs::rename(nova, destino)?;
    remover_pasta(&velha)
}

/// Grava os arquivos numa pasta nova ao lado e só então troca pela antiga.
fn substituir_pasta(destino: &Path, arquivos: &Arquivos) -> io::Result<()> {
    let nome = destino.file_name().unwrap_or_default().to_string_lossy();
    let tmp = destino.with_file_name(format!(".novo-{nome}"));
    remover_pasta(&tmp)?;
    for (caminho, conteudo) in arquivos {
        gravar(&tmp.join(caminho), conteudo)?;
    }
    trocar_pasta(&tmp, destino)
}

/// Para onde vai a pasta de um nome do ZIP.
enum Destino {
    Aluno(String),
    /// Não casou com ninguém da turma; a chave sai do nome.
    SemDono(String),
}

impl Destino {
    fn chave(&self) -> &str {
        match self {
            Destino::Aluno(c) | Destino::SemDono(c) => c,
        }
    }

    fn pasta(&self, base: &Path) -> PathBuf {
        match self {
            Destino::Aluno(aluno) => base.join(aluno),
            Destino::SemDono(chave) => base.join(SEM_DONO).join(chave),
        }
    }

    fn registros<'a>(&self, indice: &'a mut Indice) -> &'a mut BTreeMap<String, Registro> {
        match self {
            Destino::Aluno(_) => &mut indice.alunos,
            Destino::SemDono(_) => &mut indice.sem_dono,
        }
    }
}

const ENVIOS: &str = ".envios.json";

impl Dados {
    pub(crate) fn pasta_submissoes(&self, avaliacao: &str) -> Res<PathBuf> {
        checar_id(avaliacao)?;
        Ok(self.raiz().join("submissoes").join(avaliacao))
    }

    fn ler_indice(&self, pasta: &Path) -> Res<Indice> {
        Ok(ler_json(&pasta.join(INDICE))?.unwrap_or_default())
    }

    fn gravar_indice(&self, pasta: &Path, indice: &Indice) -> Res<()> {
        Ok(gravar(&pasta.join(INDICE), serde_json::to_string_pretty(indice).unwrap().as_bytes())?)
    }

    fn arquivo_apelidos(&self, turma: &str) -> Res<PathBuf> {
        checar_id(turma)?;
        Ok(self.raiz().join("apelidos").join(format!("{turma}.json")))
    }

    pub(crate) fn apelidos(&self, turma: &str) -> Res<BTreeMap<String, String>> {
        Ok(ler_json(&self.arquivo_apelidos(turma)?)?.unwrap_or_default())
    }

    /// Quando cada aluno enviou no Moodle (`"AAAA-MM-DDTHH:MM"`), da planilha de notas.
    pub fn envios(&self, avaliacao: &str) -> Res<BTreeMap<String, String>> {
        Ok(ler_json(&self.pasta_submissoes(avaliacao)?.join(ENVIOS))?.unwrap_or_default())
    }

    /// Guarda as datas lidas da planilha. Quem está nela fica com a data dela (ou
    /// sem data, se não enviou); quem não está fica com a que tinha.
    pub fn gravar_envios(&self, avaliacao: &str, lidas: &BTreeMap<String, Option<String>>) -> Res<()> {
        let caminho = self.pasta_submissoes(avaliacao)?.join(ENVIOS);
        let _g = self.travar();
        let mut envios: BTreeMap<String, String> = ler_json(&caminho)?.unwrap_or_default();
        for (aluno, data) in lidas {
            match data {
                Some(d) => envios.insert(aluno.clone(), d.clone()),
                None => envios.remove(aluno),
            };
        }
        Ok(gravar(&caminho, serde_json::to_string_pretty(&envios).unwrap().as_bytes())?)
    }

    /// Guarda as submissões do ZIP. Quem está no ZIP tem a submissão substituída;
    /// quem não está fica com a que tinha.
    pub fn importar_submissoes(&self, avaliacao: &str, zip: &[u8], agora: u64) -> Res<Importacao> {
        let av = self.ler_avaliacao(avaliacao)?;
        let turma = self.ler_turma(&av.turma)?;
        let apelidos = self.apelidos(&turma.id)?;
        let (entregas, ignorados) = ler_zip(zip)?;
        let dono = dono_pelo_nome(&turma, &apelidos);

        let pasta = self.pasta_submissoes(avaliacao)?;
        let _g = self.travar();
        let mut indice = self.ler_indice(&pasta)?;
        let mut r = Importacao { ignorados, ..Default::default() };
        // O índice é gravado a cada pasta trocada: um erro no meio não deixa código
        // novo com o resumo antigo (que faria um relatório velho valer para ele).
        for (nome, arquivos) in entregas {
            let hash = resumo(&arquivos);
            let destino = match dono(&nome) {
                Some(aluno) => {
                    // Quem ficou sem dono num ZIP anterior e agora casa sai da lista.
                    let chave = chave_sem_dono(&nome);
                    if indice.sem_dono.remove(&chave).is_some() {
                        self.gravar_indice(&pasta, &indice)?;
                        remover_pasta(&pasta.join(SEM_DONO).join(chave))?;
                    }
                    Destino::Aluno(aluno)
                }
                None => {
                    r.sem_dono.push(nome.clone());
                    Destino::SemDono(chave_sem_dono(&nome))
                }
            };
            let lugar = destino.pasta(&pasta);
            let igual = destino.registros(&mut indice).get(destino.chave()).is_some_and(|reg| reg.hash == hash) && lugar.is_dir();
            if !igual {
                substituir_pasta(&lugar, &arquivos)?;
                destino.registros(&mut indice).insert(destino.chave().to_string(), Registro { nome_moodle: nome, hash, alterada_em: agora });
                // Edições feitas sobre o código antigo não valem para o novo.
                let descartada = match &destino {
                    Destino::Aluno(aluno) => indice.edicoes.remove(aluno).map(|_| aluno.clone()),
                    Destino::SemDono(_) => None,
                };
                self.gravar_indice(&pasta, &indice)?;
                if let Some(aluno) = descartada {
                    remover_pasta(&pasta.join(EDICOES).join(&aluno))?;
                    r.edicoes_descartadas.push(aluno);
                }
            }
            if let Destino::Aluno(_) = destino {
                if igual {
                    r.iguais += 1;
                } else {
                    r.recebidas += 1;
                }
            }
        }
        self.fila_mudou();
        Ok(r)
    }

    pub fn submissoes(&self, avaliacao: &str) -> Res<Submissoes> {
        let pasta = self.pasta_submissoes(avaliacao)?;
        let indice = self.ler_indice(&pasta)?;
        let mut s = Submissoes::default();
        for (aluno, reg) in indice.alunos {
            let marcada = self.ler_correcao(avaliacao, &aluno)?.filter(|c| !c.respostas.is_empty());
            let edicao = indice.edicoes.get(&aluno).map(|e| Edicao {
                por: e.por.clone(),
                em: e.em,
                arquivos: listar(&pasta.join(EDICOES).join(&aluno)).into_iter().map(|a| a.caminho).collect(),
            });
            let hash_checagem = indice.edicoes.get(&aluno).map_or_else(|| reg.hash.clone(), |e| e.hash.clone());
            s.alunos.insert(
                aluno.clone(),
                Submissao {
                    arquivos: listar(&pasta.join(&aluno)),
                    mudou_depois_da_correcao: marcada.is_some_and(|c| reg.alterada_em > c.atualizado_em),
                    hash_checagem,
                    edicao,
                    nome_moodle: reg.nome_moodle,
                    hash: reg.hash,
                    alterada_em: reg.alterada_em,
                },
            );
        }
        for (chave, reg) in indice.sem_dono {
            let arquivos = listar(&pasta.join(SEM_DONO).join(&chave));
            s.sem_dono.push(SemDono { chave, nome_moodle: reg.nome_moodle, arquivos });
        }
        Ok(s)
    }

    /// Dá dono a uma submissão que não casou com ninguém e lembra a associação
    /// para os próximos ZIPs da turma.
    pub fn associar_sem_dono(&self, avaliacao: &str, chave: &str, aluno: &str, agora: u64) -> Res<()> {
        checar_id(chave)?;
        checar_id(aluno)?;
        let av = self.ler_avaliacao(avaliacao)?;
        let turma = self.ler_turma(&av.turma)?;
        if !turma.alunos.iter().any(|a| a.id == aluno) {
            return Err(Erro::Invalido(format!("O aluno “{aluno}” não está na turma {}", turma.id)));
        }
        let pasta = self.pasta_submissoes(avaliacao)?;
        let _g = self.travar();
        let mut indice = self.ler_indice(&pasta)?;
        let mut reg = indice
            .sem_dono
            .remove(chave)
            .ok_or_else(|| Erro::NaoEncontrado(format!("Não há submissão sem dono “{chave}”")))?;
        trocar_pasta(&pasta.join(SEM_DONO).join(chave), &pasta.join(aluno))?;
        if indice.edicoes.remove(aluno).is_some() {
            remover_pasta(&pasta.join(EDICOES).join(aluno))?;
        }
        let mut apelidos = self.apelidos(&turma.id)?;
        apelidos.insert(slug(&reg.nome_moodle), aluno.to_string());
        gravar(&self.arquivo_apelidos(&turma.id)?, serde_json::to_string_pretty(&apelidos).unwrap().as_bytes())?;
        reg.alterada_em = agora;
        indice.alunos.insert(aluno.to_string(), reg);
        self.gravar_indice(&pasta, &indice)?;
        self.fila_mudou();
        Ok(())
    }

    /// Grava uma mudança do professor num arquivo que o aluno enviou. O original
    /// fica guardado; gravar o conteúdo original desfaz a edição daquele arquivo.
    pub fn editar_arquivo(&self, avaliacao: &str, aluno: &str, caminho: &str, conteudo: &[u8], por: &str, agora: u64) -> Res<()> {
        let original = self.arquivo_original(avaliacao, aluno, caminho)?;
        if conteudo.len() as u64 > MAX_ARQUIVO {
            return Err(Erro::Invalido(format!("O arquivo passa de {} MB", MAX_ARQUIVO >> 20)));
        }
        let pasta = self.pasta_submissoes(avaliacao)?;
        let edicoes = pasta.join(EDICOES).join(aluno);
        let _g = self.travar();
        let mut indice = self.ler_indice(&pasta)?;
        if conteudo == original {
            match fs::remove_file(edicoes.join(caminho)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        } else {
            gravar(&edicoes.join(caminho), conteudo)?;
        }
        let editados = listar(&edicoes);
        if editados.is_empty() {
            remover_pasta(&edicoes)?;
            indice.edicoes.remove(aluno);
        } else {
            let mut efetivos = Arquivos::new();
            for a in listar(&pasta.join(aluno)) {
                efetivos.insert(a.caminho.clone(), fs::read(pasta.join(aluno).join(&a.caminho))?);
            }
            for a in editados {
                efetivos.insert(a.caminho.clone(), fs::read(edicoes.join(&a.caminho))?);
            }
            let reg = RegistroEdicao { por: por.into(), em: agora, hash: resumo(&efetivos) };
            indice.edicoes.insert(aluno.into(), reg);
        }
        self.gravar_indice(&pasta, &indice)?;
        self.fila_mudou();
        Ok(())
    }

    fn caminho_enviado(&self, avaliacao: &str, aluno: &str, caminho: &str) -> Res<PathBuf> {
        checar_id(aluno)?;
        if !caminho_seguro(caminho) {
            return Err(Erro::Invalido(format!("Caminho inválido: {caminho:?}")));
        }
        Ok(self.pasta_submissoes(avaliacao)?.join(aluno).join(caminho))
    }

    /// O arquivo como o aluno enviou.
    pub fn arquivo_original(&self, avaliacao: &str, aluno: &str, caminho: &str) -> Res<Vec<u8>> {
        ler_ou(&self.caminho_enviado(avaliacao, aluno, caminho)?, || format!("{aluno} não enviou {caminho}"))
    }

    /// O arquivo que as checagens e o visualizador usam: a edição do professor, se há.
    pub fn arquivo_submissao(&self, avaliacao: &str, aluno: &str, caminho: &str) -> Res<Vec<u8>> {
        let original = self.caminho_enviado(avaliacao, aluno, caminho)?;
        match fs::read(self.pasta_submissoes(avaliacao)?.join(EDICOES).join(aluno).join(caminho)) {
            Ok(editado) if original.is_file() => Ok(editado),
            _ => self.arquivo_original(avaliacao, aluno, caminho),
        }
    }
}

/// A chave de um nome sem dono vira nome de pasta.
/// O aluno da turma de um nome do Moodle: o associado à mão (`apelidos`) ou,
/// sem associação, o único aluno de mesmo nome.
pub(crate) fn dono_pelo_nome<'a>(turma: &'a Turma, apelidos: &'a BTreeMap<String, String>) -> impl Fn(&str) -> Option<String> + 'a {
    let mut por_nome: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for a in &turma.alunos {
        por_nome.entry(slug(&a.nome)).or_default().push(&a.id);
    }
    move |nome| {
        let chave = slug(nome);
        if let Some(id) = apelidos.get(&chave).filter(|id| turma.alunos.iter().any(|a| &a.id == *id)) {
            return Some(id.clone());
        }
        match por_nome.get(&chave).map(|v| v.as_slice()) {
            Some([id]) => Some(id.to_string()),
            _ => None,
        }
    }
}

fn chave_sem_dono(nome: &str) -> String {
    let s = slug(nome);
    if s.is_empty() { "sem-nome".into() } else { s }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rubricas-sub-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    pub fn zip(arquivos: &[(&str, &str)]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opcoes = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (nome, conteudo) in arquivos {
            z.start_file(*nome, opcoes).unwrap();
            z.write_all(conteudo.as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    /// Avaliação "ead" sobre a turma "d" (Ana Souza, Bruno Lima, João Prado).
    pub fn montar(nome: &str) -> (PathBuf, Dados) {
        let raiz = pasta(nome);
        let d = Dados::abrir(&raiz).unwrap();
        std::fs::write(
            raiz.join("rubricas/codigo.toml"),
            "titulo = \"Código\"\n[[criterios]]\nnome = \"Funciona\"\nescala = { tipo = \"binaria\", pontos = 1.0 }\n",
        )
        .unwrap();
        d.criar_turma("d", "matricula,nome\n1,Ana Souza\n2,Bruno Lima\n3,João Prado\n").unwrap();
        d.salvar_avaliacao(None, "EAD".into(), "d".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &Default::default()).unwrap();
        (raiz, d)
    }

    fn caminhos(s: &Submissao) -> Vec<&str> {
        s.arquivos.iter().map(|a| a.caminho.as_str()).collect()
    }

    #[test]
    fn importa_o_zip_do_moodle_associando_cada_pasta_a_um_aluno_pelo_nome() {
        let (raiz, d) = montar("importa");
        let z = zip(&[
            ("ANA SOUZA_101_assignsubmission_file_/main.py", "print('ana')\n"),
            ("Bruno Lima_102_assignsubmission_file_/main.py", "import util\n"),
            ("Bruno Lima_102_assignsubmission_file_/src/util.py", "x = 1\n"),
            ("Joao Prado_103_assignsubmission_file_/lista5.py", "print('joão')\n"),
            ("Zé Ninguém_104_assignsubmission_file_/main.py", "print('?')\n"),
        ]);
        let r = d.importar_submissoes("ead", &z, 10).unwrap();
        assert_eq!(r.recebidas, 3);
        assert_eq!(r.sem_dono, ["Zé Ninguém"]);

        let s = d.submissoes("ead").unwrap();
        assert_eq!(s.alunos.keys().collect::<Vec<_>>(), ["1", "2", "3"]);
        assert_eq!(caminhos(&s.alunos["2"]), ["main.py", "src/util.py"]);
        assert_eq!(s.alunos["1"].alterada_em, 10);
        assert_eq!(s.alunos["1"].nome_moodle, "ANA SOUZA");
        assert!(!s.alunos["1"].mudou_depois_da_correcao);
        assert_eq!(d.arquivo_submissao("ead", "2", "src/util.py").unwrap(), b"x = 1\n");
        assert_eq!(s.sem_dono.len(), 1);
        assert_eq!(s.sem_dono[0].nome_moodle, "Zé Ninguém");
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn reenvio_substitui_quem_esta_no_zip_mantem_os_ausentes_e_avisa_se_mudou_depois_da_correcao() {
        use crate::modelo::{Marcacao, Resposta};
        let (raiz, d) = montar("reenvio");
        d.importar_submissoes(
            "ead",
            &zip(&[
                ("Ana Souza_1_assignsubmission_file_/main.py", "v1"),
                ("Ana Souza_1_assignsubmission_file_/velho.py", "sai"),
                ("Bruno Lima_2_assignsubmission_file_/main.py", "b1"),
                ("João Prado_3_assignsubmission_file_/main.py", "j1"),
            ]),
            10,
        )
        .unwrap();
        // Ana e Bruno já foram corrigidos; João não.
        let marca = || vec![("codigo".into(), "funciona".into(), Some(Resposta { marcacao: Marcacao::Binaria { atende: true }, comentario: String::new() }))];
        d.alterar_correcao("ead", "1", marca(), None, Default::default(), "vini", 20).unwrap();
        d.alterar_correcao("ead", "2", marca(), None, Default::default(), "vini", 20).unwrap();

        let r = d
            .importar_submissoes(
                "ead",
                &zip(&[
                    ("Ana Souza_1_assignsubmission_file_/main.py", "v2"),
                    ("Bruno Lima_2_assignsubmission_file_/main.py", "b1"),
                ]),
                30,
            )
            .unwrap();
        assert_eq!((r.recebidas, r.iguais), (1, 1));
        let s = d.submissoes("ead").unwrap();
        // a pasta da Ana foi trocada inteira; a do João, ausente no ZIP, ficou
        assert_eq!(caminhos(&s.alunos["1"]), ["main.py"]);
        assert_eq!(d.arquivo_submissao("ead", "1", "main.py").unwrap(), b"v2");
        assert_eq!(d.arquivo_submissao("ead", "3", "main.py").unwrap(), b"j1");
        assert_eq!(s.alunos["1"].alterada_em, 30);
        assert_eq!(s.alunos["2"].alterada_em, 10, "reenvio igual não conta como mudança");
        assert!(s.alunos["1"].mudou_depois_da_correcao);
        assert!(!s.alunos["2"].mudou_depois_da_correcao);
        assert!(!s.alunos["3"].mudou_depois_da_correcao, "sem correção não há o que avisar");
        // as marcações continuam lá; mexer na correção depois apaga o aviso
        assert_eq!(d.ler_correcao("ead", "1").unwrap().unwrap().respostas["codigo"].len(), 1);
        d.alterar_correcao("ead", "1", marca(), None, Default::default(), "vini", 40).unwrap();
        assert!(!d.submissoes("ead").unwrap().alunos["1"].mudou_depois_da_correcao);
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn associacao_manual_de_quem_ficou_sem_dono_e_lembrada_nos_proximos_zips() {
        let (raiz, d) = montar("sem-dono");
        let r = d.importar_submissoes("ead", &zip(&[("Bruninho L._2_assignsubmission_file_/main.py", "b1")]), 10).unwrap();
        assert_eq!(r.sem_dono, ["Bruninho L."]);
        let chave = d.submissoes("ead").unwrap().sem_dono[0].chave.clone();

        assert!(matches!(d.associar_sem_dono("ead", &chave, "99", 11), Err(Erro::Invalido(_))), "aluno fora da turma");
        d.associar_sem_dono("ead", &chave, "2", 11).unwrap();
        let s = d.submissoes("ead").unwrap();
        assert!(s.sem_dono.is_empty());
        assert_eq!(s.alunos["2"].nome_moodle, "Bruninho L.");
        assert_eq!(d.arquivo_submissao("ead", "2", "main.py").unwrap(), b"b1");

        // o próximo ZIP, mesmo de outra avaliação da turma, já casa sozinho
        d.salvar_avaliacao(None, "EAD 2".into(), "d".into(), 10.0, vec![("codigo".into(), 1.0)], &[], &Default::default()).unwrap();
        let r = d.importar_submissoes("ead-2", &zip(&[("Bruninho L._7_assignsubmission_file_/main.py", "b2")]), 20).unwrap();
        assert!(r.sem_dono.is_empty());
        assert_eq!(d.arquivo_submissao("ead-2", "2", "main.py").unwrap(), b"b2");

        // um nome sem dono que passa a casar (a turma foi corrigida) sai da lista
        d.importar_submissoes("ead", &zip(&[("Zé Ninguém_8_assignsubmission_file_/main.py", "z")]), 30).unwrap();
        assert_eq!(d.submissoes("ead").unwrap().sem_dono.len(), 1);
        d.substituir_turma("d", "matricula,nome\n1,Ana Souza\n2,Bruno Lima\n3,João Prado\n5,Zé Ninguém\n").unwrap();
        d.importar_submissoes("ead", &zip(&[("Zé Ninguém_8_assignsubmission_file_/main.py", "z")]), 40).unwrap();
        let s = d.submissoes("ead").unwrap();
        assert!(s.sem_dono.is_empty(), "{:?}", s.sem_dono);
        assert_eq!(d.arquivo_submissao("ead", "5", "main.py").unwrap(), b"z");
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn entradas_fora_do_padrao_ou_que_escapam_da_pasta_sao_ignoradas() {
        let (raiz, d) = montar("perigo");
        let r = d
            .importar_submissoes(
                "ead",
                &zip(&[
                    ("Ana Souza_1_assignsubmission_file_/../../usuarios.toml", "x"),
                    ("Ana Souza_1_assignsubmission_file_/.oculto", "x"),
                    ("__MACOSX/Ana Souza_1_assignsubmission_file_/._main.py", "x"),
                    ("leiame.txt", "x"),
                    ("Ana Souza_1_assignsubmission_file_main.py", "plano"),
                    ("Ana Souza_1_assignsubmission_onlinetext_onlinetext.html", "<p>oi</p>"),
                ]),
                10,
            )
            .unwrap();
        assert_eq!(r.ignorados, ["Ana Souza_1_assignsubmission_file_/../../usuarios.toml", "leiame.txt"]);
        let s = d.submissoes("ead").unwrap();
        assert_eq!(caminhos(&s.alunos["1"]), ["main.py", "onlinetext.html"]);
        assert!(!raiz.join("usuarios.toml").exists());
        assert!(matches!(d.importar_submissoes("ead", b"isto nao e zip", 10), Err(Erro::Invalido(_))));
        assert!(matches!(d.arquivo_submissao("ead", "1", "../../usuarios.toml"), Err(Erro::Invalido(_))));
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn edicao_do_professor_fica_por_cima_do_original_ate_um_zip_trazer_codigo_novo() {
        let (raiz, d) = montar("edicao");
        d.importar_submissoes(
            "ead",
            &zip(&[("Ana Souza_1_assignsubmission_file_/main.py", "print(1\n"), ("Bruno Lima_2_assignsubmission_file_/main.py", "b")]),
            10,
        )
        .unwrap();
        let antes = d.submissoes("ead").unwrap().alunos["1"].clone();

        d.editar_arquivo("ead", "1", "main.py", b"print(1)\n", "vini", 20).unwrap();
        assert_eq!(d.arquivo_submissao("ead", "1", "main.py").unwrap(), b"print(1)\n");
        assert_eq!(d.arquivo_original("ead", "1", "main.py").unwrap(), b"print(1\n");
        let s = d.submissoes("ead").unwrap().alunos["1"].clone();
        let e = s.edicao.as_ref().unwrap();
        assert_eq!((e.por.as_str(), e.em, e.arquivos.as_slice()), ("vini", 20, ["main.py".to_string()].as_slice()));
        assert_eq!(s.hash, antes.hash, "o original não muda");
        assert_ne!(s.hash_checagem, antes.hash_checagem, "as checagens veem o código editado");
        assert_eq!(s.alterada_em, 10, "editar não é o aluno reenviar");

        // só arquivos que o aluno enviou podem ser editados
        assert!(matches!(d.editar_arquivo("ead", "1", "novo.py", b"x", "vini", 21), Err(Erro::NaoEncontrado(_))));
        assert!(matches!(d.editar_arquivo("ead", "3", "main.py", b"x", "vini", 21), Err(Erro::NaoEncontrado(_))));

        // reenvio igual mantém a edição; código novo do aluno a descarta, com aviso
        let r = d.importar_submissoes("ead", &zip(&[("Ana Souza_1_assignsubmission_file_/main.py", "print(1\n")]), 30).unwrap();
        assert!(r.edicoes_descartadas.is_empty());
        assert!(d.submissoes("ead").unwrap().alunos["1"].edicao.is_some());
        let r = d.importar_submissoes("ead", &zip(&[("Ana Souza_1_assignsubmission_file_/main.py", "print(2)\n")]), 40).unwrap();
        assert_eq!(r.edicoes_descartadas, ["1"]);
        let s = d.submissoes("ead").unwrap().alunos["1"].clone();
        assert!(s.edicao.is_none());
        assert_eq!(s.hash, s.hash_checagem);
        assert_eq!(d.arquivo_submissao("ead", "1", "main.py").unwrap(), b"print(2)\n");

        // voltar ao conteúdo original desfaz a edição daquele arquivo
        d.editar_arquivo("ead", "2", "main.py", b"b2", "vini", 50).unwrap();
        d.editar_arquivo("ead", "2", "main.py", b"b", "vini", 51).unwrap();
        let s = d.submissoes("ead").unwrap().alunos["2"].clone();
        assert!(s.edicao.is_none());
        assert_eq!(s.hash, s.hash_checagem);
        std::fs::remove_dir_all(&raiz).unwrap();
    }
}
