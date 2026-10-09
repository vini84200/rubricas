//! Leitura e escrita na pasta de dados.
//!
//! ```text
//! dados/
//!   rubricas/<id>.toml              uma rubrica por arquivo, editável à mão
//!   turmas/<id>.csv                 matricula,nome
//!   avaliacoes/<id>.json            título, turma e cópia das rubricas usadas
//!   correcoes/<avaliacao>/<aluno>.json
//!   usuarios.toml                   criado por `rubricas usuario <nome>`
//! ```

use crate::modelo::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{fs, io};

#[derive(Debug)]
pub enum Erro {
    NaoEncontrado(String),
    Invalido(String),
    Conflito(String),
    Io(io::Error),
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

fn checar_id(id: &str) -> Res<()> {
    if id_valido(id) {
        Ok(())
    } else {
        Err(Erro::Invalido(format!(
            "Nome inválido: {id:?}. Use até 100 bytes, sem / \\ : * ? \" < > | e sem começar com ponto."
        )))
    }
}

/// Lê um arquivo de texto. `None` se não existe; `Invalido` se não é UTF-8.
fn ler_opcional(caminho: &Path) -> Res<Option<String>> {
    match fs::read(caminho) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
            let nome = caminho.file_name().unwrap_or_default().to_string_lossy();
            Erro::Invalido(format!("{nome}: o arquivo não está em UTF-8"))
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Uma marcação que não pôde ser aplicada (critério removido, nível inexistente…).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Recusada {
    pub rubrica: String,
    pub criterio: String,
    pub motivo: String,
}

impl Dados {
    pub fn abrir(raiz: impl Into<PathBuf>) -> io::Result<Self> {
        let raiz = raiz.into();
        for sub in ["rubricas", "turmas", "avaliacoes", "correcoes"] {
            fs::create_dir_all(raiz.join(sub))?;
        }
        Ok(Dados { raiz, trava: Mutex::new(()) })
    }

    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    fn caminho(&self, sub: &str, id: &str, ext: &str) -> Res<PathBuf> {
        checar_id(id)?;
        Ok(self.raiz.join(sub).join(format!("{id}.{ext}")))
    }

    /// Ids (nomes de arquivo sem extensão) de uma subpasta, em ordem alfabética.
    fn ids(&self, sub: &str, ext: &str) -> Res<Vec<String>> {
        let mut ids = Vec::new();
        let dir = match fs::read_dir(self.raiz.join(sub)) {
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

    // ----- rubricas -----

    pub fn ler_rubrica(&self, id: &str) -> Res<Rubrica> {
        let caminho = self.caminho("rubricas", id, "toml")?;
        let texto = ler_opcional(&caminho)?.ok_or_else(|| Erro::NaoEncontrado(format!("Rubrica “{id}” não existe")))?;
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
        for id in self.ids("rubricas", "toml")? {
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
        let _g = self.trava.lock().unwrap();
        r.normalizar();
        r.validar().map_err(Erro::Invalido)?;
        let id = match id {
            Some(id) => id.to_string(),
            None => id_livre(&slug(&r.titulo), |c| self.raiz.join("rubricas").join(format!("{c}.toml")).exists()),
        };
        let caminho = self.caminho("rubricas", &id, "toml")?;
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
        let mut r: Rubrica = toml::from_str(texto).map_err(|e| {
            let onde = e.span().map(|s| {
                let antes = &texto[..s.start.min(texto.len())];
                let linha = antes.matches('\n').count() + 1;
                let coluna = antes.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                format!("linha {linha}, coluna {coluna}: ")
            });
            Erro::Invalido(format!("{}{}", onde.unwrap_or_default(), e.message().trim()))
        })?;
        match self.ler_rubrica(id) {
            Ok(anterior) => r.herdar_ids(&anterior),
            Err(Erro::NaoEncontrado(_) | Erro::Invalido(_)) => {}
            Err(e) => return Err(e),
        }
        self.salvar_rubrica(Some(id), r)
    }

    pub fn apagar_rubrica(&self, id: &str) -> Res<()> {
        let _g = self.trava.lock().unwrap();
        let caminho = self.caminho("rubricas", id, "toml")?;
        match fs::remove_file(caminho) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Erro::NaoEncontrado(format!("Rubrica “{id}” não existe"))),
            outro => Ok(outro?),
        }
    }

    // ----- turmas -----

    pub fn listar_turmas(&self) -> Res<Vec<Turma>> {
        let mut turmas = Vec::new();
        for id in self.ids("turmas", "csv")? {
            match self.ler_turma(&id) {
                Ok(t) => turmas.push(t),
                Err(Erro::Invalido(_) | Erro::Io(_)) => turmas.push(Turma { id, alunos: Vec::new() }),
                Err(e) => return Err(e),
            }
        }
        Ok(turmas)
    }

    pub fn ler_turma(&self, id: &str) -> Res<Turma> {
        let caminho = self.caminho("turmas", id, "csv")?;
        let bytes = match fs::read(&caminho) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(Erro::NaoEncontrado(format!("Turma “{id}” não existe")));
            }
            Err(e) => return Err(e.into()),
        };
        // Planilhas antigas exportam em Latin-1; cada byte vira o caractere de mesmo código.
        let texto = String::from_utf8(bytes).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect());
        let alunos = ler_alunos(&texto).map_err(|e| Erro::Invalido(format!("turmas/{id}.csv: {e}")))?;
        Ok(Turma { id: id.to_string(), alunos })
    }

    pub fn salvar_turma(&self, id: &str, csv: &str, nova: bool) -> Res<Turma> {
        let _g = self.trava.lock().unwrap();
        let caminho = self.caminho("turmas", id, "csv")?;
        if nova && caminho.exists() {
            return Err(Erro::Conflito(format!("Já existe uma turma chamada “{id}”")));
        }
        let alunos = ler_alunos(csv).map_err(Erro::Invalido)?;
        gravar(&caminho, escrever_alunos(&alunos).as_bytes())?;
        Ok(Turma { id: id.to_string(), alunos })
    }

    pub fn apagar_turma(&self, id: &str) -> Res<()> {
        let em_uso: Vec<String> = self.listar_avaliacoes()?.into_iter().filter(|a| a.turma == id).map(|a| a.titulo).collect();
        if !em_uso.is_empty() {
            return Err(Erro::Conflito(format!("A turma está em uso por: {}", em_uso.join(", "))));
        }
        let _g = self.trava.lock().unwrap();
        let caminho = self.caminho("turmas", id, "csv")?;
        match fs::remove_file(caminho) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Erro::NaoEncontrado(format!("Turma “{id}” não existe"))),
            outro => Ok(outro?),
        }
    }

    // ----- avaliações -----

    pub fn ler_avaliacao(&self, id: &str) -> Res<Avaliacao> {
        let caminho = self.caminho("avaliacoes", id, "json")?;
        let texto = ler_opcional(&caminho)?.ok_or_else(|| Erro::NaoEncontrado(format!("Avaliação “{id}” não existe")))?;
        let mut a: Avaliacao =
            serde_json::from_str(&texto).map_err(|e| Erro::Invalido(format!("avaliacoes/{id}.json: {e}")))?;
        a.id = id.to_string();
        Ok(a)
    }

    pub fn listar_avaliacoes(&self) -> Res<Vec<Avaliacao>> {
        let mut todas = Vec::new();
        for id in self.ids("avaliacoes", "json")? {
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
    pub fn salvar_avaliacao(
        &self,
        id: Option<&str>,
        titulo: String,
        turma: String,
        nota_maxima: f64,
        rubricas: Vec<(String, f64)>,
        atualizar: &[String],
    ) -> Res<Avaliacao> {
        self.ler_turma(&turma).map_err(|e| match e {
            Erro::NaoEncontrado(m) => Erro::Invalido(m),
            e => e,
        })?;
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
                None => self.ler_rubrica(&origem).map_err(|e| match e {
                    Erro::NaoEncontrado(m) => Erro::Invalido(m),
                    e => e,
                })?,
            };
            aplicadas.push(RubricaAplicada { origem, peso, rubrica });
        }
        let mut a = Avaliacao { id: String::new(), titulo, turma, nota_maxima, rubricas: aplicadas };
        a.validar().map_err(Erro::Invalido)?;

        let _g = self.trava.lock().unwrap();
        let id = match id {
            Some(id) => id.to_string(),
            None => id_livre(&slug(&a.titulo), |c| self.raiz.join("avaliacoes").join(format!("{c}.json")).exists()),
        };
        let caminho = self.caminho("avaliacoes", &id, "json")?;
        gravar(&caminho, serde_json::to_string_pretty(&a).unwrap().as_bytes())?;
        a.id = id;
        Ok(a)
    }

    pub fn apagar_avaliacao(&self, id: &str) -> Res<()> {
        let _g = self.trava.lock().unwrap();
        let caminho = self.caminho("avaliacoes", id, "json")?;
        match fs::remove_file(caminho) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(Erro::NaoEncontrado(format!("Avaliação “{id}” não existe")));
            }
            outro => outro?,
        }
        match fs::remove_dir_all(self.raiz.join("correcoes").join(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Origens cuja rubrica na pasta difere da cópia guardada na avaliação.
    pub fn rubricas_desatualizadas(&self, a: &Avaliacao) -> Vec<String> {
        a.rubricas
            .iter()
            .filter(|ra| matches!(self.ler_rubrica(&ra.origem), Ok(atual) if atual != ra.rubrica))
            .map(|ra| ra.origem.clone())
            .collect()
    }

    // ----- correções -----

    fn caminho_correcao(&self, avaliacao: &str, aluno: &str) -> Res<PathBuf> {
        checar_id(avaliacao)?;
        checar_id(aluno)?;
        Ok(self.raiz.join("correcoes").join(avaliacao).join(format!("{aluno}.json")))
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
        checar_id(avaliacao)?;
        let sub = format!("correcoes/{avaliacao}");
        let mut todas = BTreeMap::new();
        for aluno in self.ids(&sub, "json")? {
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
    pub fn alterar_correcao(
        &self,
        avaliacao: &str,
        aluno: &str,
        respostas: Vec<(String, String, Option<Resposta>)>,
        comentario_geral: Option<String>,
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

        let _g = self.trava.lock().unwrap();
        let mut c = self.ler_correcao(avaliacao, aluno)?.unwrap_or_default();
        if aceitas.is_empty() && comentario_geral.is_none() {
            return Ok((c, recusadas));
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

        d.salvar_turma("Turma D", "matricula;nome\n1;Ana\n2;Bruno\n", true).unwrap();
        assert!(matches!(d.salvar_turma("Turma D", "x", true), Err(Erro::Conflito(_))));
        assert_eq!(fs::read_to_string(raiz.join("turmas/Turma D.csv")).unwrap(), "matricula,nome\n1,Ana\n2,Bruno\n");

        let av = d
            .salvar_avaliacao(None, "TF".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &[])
            .unwrap();
        assert_eq!(av.id, "tf");
        assert!(matches!(d.apagar_turma("Turma D"), Err(Erro::Conflito(_))));

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
                "gustavo",
                2,
            )
            .unwrap();
        assert_eq!(c.respostas["codigo"].len(), 2);
        assert_eq!(calcular(&av, Some(&c)).nota, 10.0);
        // apagar uma marcação
        let (c, _) = d.alterar_correcao("tf", "1", vec![("codigo".into(), "compila".into(), None)], None, "vini", 3).unwrap();
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
                "vini",
                4,
            )
            .unwrap();
        assert_eq!(recusadas.iter().map(|r| r.criterio.as_str()).collect::<Vec<_>>(), ["compila", "sumiu"]);
        assert_eq!(c.respostas["codigo"]["clareza"].marcacao, Marcacao::Nivel { nivel: "ruim".into() });
        assert_eq!(d.ler_correcao("tf", "1").unwrap().unwrap(), c);
        assert!(matches!(d.alterar_correcao("tf", "99", vec![], None, "vini", 4), Err(Erro::NaoEncontrado(_))));
        assert!(matches!(d.ler_rubrica("../usuarios"), Err(Erro::Invalido(_))));

        // atualizar a cópia a pedido
        let av = d
            .salvar_avaliacao(Some("tf"), "TF".into(), "Turma D".into(), 10.0, vec![("codigo".into(), 1.0)], &["codigo".into()])
            .unwrap();
        assert_eq!(av.rubricas[0].rubrica.criterios.len(), 1);
        assert!(d.rubricas_desatualizadas(&av).is_empty());

        d.apagar_avaliacao("tf").unwrap();
        assert!(!raiz.join("correcoes/tf").exists());
        d.apagar_turma("Turma D").unwrap();
        fs::remove_dir_all(&raiz).unwrap();
    }
}
