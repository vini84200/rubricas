//! Pipelines, relatórios e a fila do executor, na pasta de dados.
//!
//! ```text
//! dados/pipelines/<avaliacao>/pipeline.toml     checagens da avaliação
//! dados/pipelines/<avaliacao>/...               anexos (testes.py, esqueleto.py, referencias/)
//! dados/relatorios/<avaliacao>/<aluno>.json     resultado das checagens de um aluno
//! dados/relatorios/<avaliacao>/.plagio.json     pares parecidos (JPlag)
//! dados/relatorios/<avaliacao>/.plagio.jplag    relatório para o visualizador oficial
//! ```
//!
//! Cada trabalho tem uma versão: o resumo do pipeline mais o da submissão. Um
//! trabalho está pendente enquanto não há relatório dessa versão, ou enquanto o
//! relatório tem erro de execução.

use crate::dados::{Dados, Erro, Res, Tipo, checar_id, gravar, ler_json, ler_opcional, ler_ou};
use crate::executor::MOTOR;
use crate::modelo::Avaliacao;
use crate::pipeline::{self, Pipeline, Relatorio, Situacao, Sugestao};
use crate::submissoes::{Arquivos, InfoArquivo, Submissao, Submissoes, listar, resumo};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Tarefa {
    Checagens { avaliacao: String, aluno: String, versao: String },
    Plagio { avaliacao: String, versao: String },
}

/// Dois trabalhos parecidos. `a` e `b` são ids de aluno ou `referencias/<nome>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Par {
    pub a: String,
    pub b: String,
    pub similaridade: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultadoPlagio {
    pub versao: String,
    #[serde(default)]
    pub gerado_em: u64,
    #[serde(default)]
    pub pares: Vec<Par>,
    /// Erro de execução (JPlag não rodou): não conta como feito.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub erro: String,
}

fn versao(partes: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in partes {
        h.update(p.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// O trabalho de checar um aluno: pipeline, código e a regra de julgar do executor.
/// O plágio não depende dela e não entra aqui.
fn versao_checagens(pipeline: &str, codigo: &str) -> String {
    versao(&[MOTOR, pipeline, codigo])
}

/// O pipeline de uma avaliação, como está no disco.
pub enum EstadoPipeline {
    Ausente,
    /// Não vale contra a avaliação (a rubrica mudou depois dele); espera ser corrigido.
    Invalido(String),
    Valido(Pipeline),
}

/// O trabalho de um aluno com submissão.
pub enum EstadoAluno<'a> {
    SemRelatorio,
    Atual(&'a Relatorio),
    /// O código ou o pipeline mudaram depois do relatório.
    Desatualizado(&'a Relatorio),
}

/// Tudo o que decide a fila de uma avaliação, lido do disco uma vez: a fila do
/// executor, os relatórios da tela e as sugestões saem daqui.
pub struct Panorama {
    avaliacao: Avaliacao,
    pub pipeline: EstadoPipeline,
    /// Resumo dos arquivos do pipeline; `None` sem `pipeline.toml`.
    versao_pipeline: Option<String>,
    pub submissoes: Submissoes,
    relatorios: BTreeMap<String, Relatorio>,
    plagio: Option<ResultadoPlagio>,
    arquivada: bool,
}

impl Panorama {
    /// Versão atual do trabalho do aluno; `None` sem pipeline.
    fn versao(&self, s: &Submissao) -> Option<String> {
        Some(versao_checagens(self.versao_pipeline.as_deref()?, &s.hash_checagem))
    }

    /// `None` se o aluno não tem submissão.
    pub fn estado(&self, aluno: &str) -> Option<EstadoAluno<'_>> {
        let s = self.submissoes.alunos.get(aluno)?;
        Some(match self.relatorios.get(aluno) {
            None => EstadoAluno::SemRelatorio,
            Some(r) if self.versao(s).as_deref() == Some(r.versao.as_str()) => EstadoAluno::Atual(r),
            Some(r) => EstadoAluno::Desatualizado(r),
        })
    }

    /// Os relatórios de quem tem submissão, cada um com "é da versão atual?".
    pub fn relatorios(&self) -> impl Iterator<Item = (&str, &Relatorio, bool)> {
        self.submissoes.alunos.keys().filter_map(|a| match self.estado(a)? {
            EstadoAluno::SemRelatorio => None,
            EstadoAluno::Atual(r) => Some((a.as_str(), r, true)),
            EstadoAluno::Desatualizado(r) => Some((a.as_str(), r, false)),
        })
    }

    /// Recalculadas a cada leitura: a cópia da rubrica pode ter mudado depois do
    /// relatório sem que nada precise rodar de novo. Vazias se ele não é o atual.
    pub fn sugestoes(&self, aluno: &str) -> Vec<Sugestao> {
        match (&self.pipeline, self.estado(aluno)) {
            (EstadoPipeline::Valido(p), Some(EstadoAluno::Atual(r))) => p.sugestoes(&self.avaliacao, &r.checagens),
            _ => Vec::new(),
        }
    }

    pub fn plagio(&self) -> Option<&ResultadoPlagio> {
        self.plagio.as_ref()
    }

    /// O que falta rodar. Vazia sem pipeline válido ou com a avaliação arquivada.
    pub fn fila(&self) -> Vec<Tarefa> {
        let (EstadoPipeline::Valido(p), Some(vp), false) = (&self.pipeline, &self.versao_pipeline, self.arquivada) else {
            return Vec::new();
        };
        let id = &self.avaliacao.id;
        let mut fila = Vec::new();
        for (aluno, s) in &self.submissoes.alunos {
            // Erro de execução não conta como feito.
            let feito = matches!(self.estado(aluno), Some(EstadoAluno::Atual(r)) if !r.checagens.iter().any(|c| c.situacao == Situacao::Erro));
            if !feito {
                fila.push(Tarefa::Checagens { avaliacao: id.clone(), aluno: aluno.clone(), versao: versao_checagens(vp, &s.hash_checagem) });
            }
        }
        if p.plagio.is_some() && self.submissoes.alunos.len() >= 2 {
            // O plágio compara o que os alunos enviaram, não as edições do professor.
            let partes: Vec<String> = self.submissoes.alunos.iter().map(|(a, s)| format!("{a}:{}", s.hash)).collect();
            let mut todas = vec![vp.as_str()];
            todas.extend(partes.iter().map(String::as_str));
            let v = versao(&todas);
            if !self.plagio.as_ref().is_some_and(|r| r.versao == v && r.erro.is_empty()) {
                fila.push(Tarefa::Plagio { avaliacao: id.clone(), versao: v });
            }
        }
        fila
    }
}

impl Dados {
    pub(crate) fn pasta_pipeline(&self, avaliacao: &str) -> Res<PathBuf> {
        checar_id(avaliacao)?;
        Ok(self.raiz().join("pipelines").join(avaliacao))
    }

    pub(crate) fn pasta_relatorios(&self, avaliacao: &str) -> Res<PathBuf> {
        checar_id(avaliacao)?;
        Ok(self.raiz().join("relatorios").join(avaliacao))
    }

    /// O texto do pipeline, como foi gravado (com comentários).
    pub fn pipeline_toml(&self, avaliacao: &str) -> Res<Option<String>> {
        ler_opcional(&self.pasta_pipeline(avaliacao)?.join("pipeline.toml"))
    }

    /// O pipeline da avaliação, validado contra as rubricas dela. `None` se não há.
    pub fn ler_pipeline(&self, avaliacao: &str) -> Res<Option<Pipeline>> {
        let Some(texto) = self.pipeline_toml(avaliacao)? else { return Ok(None) };
        let av = self.ler_avaliacao(avaliacao)?;
        pipeline::ler(&texto, &av).map(Some).map_err(|e| Erro::Invalido(format!("pipelines/{avaliacao}/pipeline.toml: {e}")))
    }

    pub fn estado_pipeline(&self, avaliacao: &str) -> EstadoPipeline {
        match self.ler_pipeline(avaliacao) {
            Ok(None) => EstadoPipeline::Ausente,
            Ok(Some(p)) => EstadoPipeline::Valido(p),
            Err(e) => EstadoPipeline::Invalido(e.to_string()),
        }
    }

    /// Valida e grava o pipeline. Devolve avisos sobre anexos que faltam.
    pub fn salvar_pipeline_toml(&self, avaliacao: &str, texto: &str) -> Res<(Pipeline, Vec<String>)> {
        let pasta = self.pasta_pipeline(avaliacao)?;
        let p = {
            // Validado sob a trava: a avaliação não muda entre conferir e gravar.
            let _g = self.travar();
            let p = pipeline::ler(texto, &self.ler_avaliacao(avaliacao)?).map_err(Erro::Invalido)?;
            gravar(&pasta.join("pipeline.toml"), texto.as_bytes())?;
            p
        };
        self.fila_mudou();
        let mut anexos: Vec<&str> = p.checagens.iter().map(|c| c.testes.as_str()).collect();
        if let Some(pl) = &p.plagio {
            anexos.extend([pl.base.as_str(), pl.referencias.as_str()]);
        }
        let avisos = anexos
            .into_iter()
            .filter(|a| !a.is_empty() && !pasta.join(a.trim_end_matches('/')).exists())
            .map(|a| format!("falta o anexo {a}"))
            .collect();
        Ok((p, avisos))
    }

    /// Arquivos da pasta do pipeline, inclusive `pipeline.toml`.
    pub fn anexos_pipeline(&self, avaliacao: &str) -> Res<Vec<InfoArquivo>> {
        Ok(listar(&self.pasta_pipeline(avaliacao)?))
    }

    fn caminho_anexo(&self, avaliacao: &str, caminho: &str) -> Res<PathBuf> {
        if !pipeline::caminho_seguro(caminho) {
            return Err(Erro::Invalido(format!("Caminho inválido: {caminho:?}")));
        }
        Ok(self.pasta_pipeline(avaliacao)?.join(caminho))
    }

    pub fn anexo_pipeline(&self, avaliacao: &str, caminho: &str) -> Res<Vec<u8>> {
        ler_ou(&self.caminho_anexo(avaliacao, caminho)?, || format!("pipelines/{avaliacao}/{caminho} não existe"))
    }

    /// Grava um anexo (testes, esqueleto, referências). O `pipeline.toml` passa
    /// por `salvar_pipeline_toml`, que o valida.
    pub fn salvar_anexo_pipeline(&self, avaliacao: &str, caminho: &str, bytes: &[u8]) -> Res<()> {
        self.ler_avaliacao(avaliacao)?;
        if caminho == "pipeline.toml" {
            return Err(Erro::Invalido("pipeline.toml é gravado com validação, não como anexo".into()));
        }
        let destino = self.caminho_anexo(avaliacao, caminho)?;
        let _g = self.travar();
        gravar(&destino, bytes)?;
        self.fila_mudou();
        Ok(())
    }

    pub fn apagar_anexo_pipeline(&self, avaliacao: &str, caminho: &str) -> Res<()> {
        let alvo = self.caminho_anexo(avaliacao, caminho)?;
        let _g = self.travar();
        let r = if alvo.is_dir() { fs::remove_dir_all(&alvo) } else { fs::remove_file(&alvo) };
        match r {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(Erro::NaoEncontrado(format!("pipelines/{avaliacao}/{caminho} não existe")))
            }
            outro => {
                outro?;
                self.fila_mudou();
                Ok(())
            }
        }
    }

    /// Resumo de todos os arquivos do pipeline: mudar qualquer anexo muda a versão.
    fn versao_pipeline(&self, avaliacao: &str) -> Res<String> {
        let pasta = self.pasta_pipeline(avaliacao)?;
        let mut arquivos = Arquivos::new();
        for a in listar(&pasta) {
            arquivos.insert(a.caminho.clone(), fs::read(pasta.join(&a.caminho))?);
        }
        Ok(resumo(&arquivos))
    }

    pub fn panorama(&self, avaliacao: &str) -> Res<Panorama> {
        let pasta = self.pasta_relatorios(avaliacao)?;
        let submissoes = self.submissoes(avaliacao)?;
        let mut relatorios = BTreeMap::new();
        for aluno in submissoes.alunos.keys() {
            if let Some(r) = ler_json(&pasta.join(format!("{aluno}.json")))? {
                relatorios.insert(aluno.clone(), r);
            }
        }
        let versao_pipeline = match self.pipeline_toml(avaliacao)? {
            Some(_) => Some(self.versao_pipeline(avaliacao)?),
            None => None,
        };
        Ok(Panorama {
            avaliacao: self.ler_avaliacao(avaliacao)?,
            pipeline: self.estado_pipeline(avaliacao),
            versao_pipeline,
            submissoes,
            relatorios,
            plagio: ler_json(&pasta.join(".plagio.json"))?,
            arquivada: self.arquivadas().contem(Tipo::Avaliacoes, avaliacao),
        })
    }

    /// Tudo que falta rodar, em todas as avaliações com pipeline.
    pub fn fila(&self) -> Res<Vec<Tarefa>> {
        let mut fila = Vec::new();
        for av in self.listar_avaliacoes()? {
            if self.pipeline_toml(&av.id)?.is_some() {
                fila.extend(self.panorama(&av.id)?.fila());
            }
        }
        Ok(fila)
    }

    /// Grava o relatório do executor e devolve as sugestões dele. Só aceita o da
    /// versão atual do trabalho: um executor lento não apaga um relatório mais novo.
    pub fn salvar_relatorio(&self, avaliacao: &str, aluno: &str, mut relatorio: Relatorio, agora: u64) -> Res<Vec<Sugestao>> {
        checar_id(aluno)?;
        if relatorio.motor != MOTOR {
            return Err(Erro::Conflito(format!(
                "O executor julga pela regra {:?} e o servidor, pela {MOTOR:?}: atualize o rubricas executor",
                relatorio.motor
            )));
        }
        let _g = self.travar();
        let panorama = self.panorama(avaliacao)?;
        let p = match &panorama.pipeline {
            EstadoPipeline::Valido(p) => p,
            EstadoPipeline::Ausente => return Err(Erro::NaoEncontrado(format!("A avaliação “{avaliacao}” não tem pipeline"))),
            EstadoPipeline::Invalido(m) => return Err(Erro::Invalido(m.clone())),
        };
        let atual = panorama.submissoes.alunos.get(aluno).and_then(|s| panorama.versao(s));
        if atual.as_deref() != Some(relatorio.versao.as_str()) {
            return Err(Erro::Conflito("O trabalho mudou depois que este relatório começou; ele volta para a fila".into()));
        }
        if relatorio.checagens.len() != p.checagens.len() {
            return Err(Erro::Invalido(format!(
                "O relatório tem {} checagens; o pipeline tem {}",
                relatorio.checagens.len(),
                p.checagens.len()
            )));
        }
        relatorio.gerado_em = agora;
        gravar(&self.pasta_relatorios(avaliacao)?.join(format!("{aluno}.json")), serde_json::to_string_pretty(&relatorio).unwrap().as_bytes())?;
        Ok(p.sugestoes(&panorama.avaliacao, &relatorio.checagens))
    }

    pub fn salvar_plagio(&self, avaliacao: &str, mut r: ResultadoPlagio, agora: u64) -> Res<()> {
        self.ler_avaliacao(avaliacao)?;
        r.gerado_em = agora;
        r.pares.retain(|p| p.similaridade.is_finite());
        // No empate, pares entre alunos antes de pares com uma referência.
        let refs = |p: &Par| [&p.a, &p.b].iter().filter(|x| x.starts_with("referencias/")).count();
        r.pares.sort_by(|x, y| y.similaridade.total_cmp(&x.similaridade).then(refs(x).cmp(&refs(y))));
        let caminho = self.pasta_relatorios(avaliacao)?.join(".plagio.json");
        let _g = self.travar();
        Ok(gravar(&caminho, serde_json::to_string_pretty(&r).unwrap().as_bytes())?)
    }

    /// O `.jplag` para abrir no visualizador oficial.
    pub fn salvar_jplag(&self, avaliacao: &str, bytes: &[u8]) -> Res<()> {
        self.ler_avaliacao(avaliacao)?;
        let caminho = self.pasta_relatorios(avaliacao)?.join(".plagio.jplag");
        let _g = self.travar();
        Ok(gravar(&caminho, bytes)?)
    }

    pub fn jplag(&self, avaliacao: &str) -> Res<Vec<u8>> {
        ler_ou(&self.pasta_relatorios(avaliacao)?.join(".plagio.jplag"), || "O JPlag ainda não rodou nesta avaliação".into())
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::pipeline::{ResultadoChecagem, TipoChecagem};
    use crate::submissoes::testes::{montar, zip};

    const PIPELINE: &str = r#"
plagio = { limiar = 0.7 }
[[checagens]]
tipo = "casos"
criterio = "funciona"
[[checagens.casos]]
entrada = "1"
saida = "1"
"#;

    fn pendentes(d: &Dados) -> Vec<String> {
        d.fila()
            .unwrap()
            .into_iter()
            .map(|t| match t {
                Tarefa::Checagens { aluno, .. } => aluno,
                Tarefa::Plagio { .. } => "plágio".into(),
            })
            .collect()
    }

    fn relatorio(d: &Dados, aluno: &str, situacao: Situacao) -> Relatorio {
        let versao = d
            .fila()
            .unwrap()
            .into_iter()
            .find_map(|t| match t {
                Tarefa::Checagens { aluno: a, versao, .. } if a == aluno => Some(versao),
                _ => None,
            })
            .unwrap();
        let passaram = u32::from(situacao == Situacao::Passou);
        let r = ResultadoChecagem { nome: "casos".into(), tipo: TipoChecagem::Casos, situacao, passaram, total: 1, resumo: String::new(), casos: vec![], saida: String::new() };
        Relatorio { versao, motor: crate::executor::MOTOR.into(), gerado_em: 0, checagens: vec![r] }
    }

    /// O relatório do aluno e se ele é o da versão atual.
    fn atual(d: &Dados, aluno: &str) -> Option<bool> {
        match d.panorama("ead").unwrap().estado(aluno)? {
            EstadoAluno::SemRelatorio => None,
            EstadoAluno::Atual(_) => Some(true),
            EstadoAluno::Desatualizado(_) => Some(false),
        }
    }

    #[test]
    fn fila_tem_quem_ainda_nao_foi_checado_na_versao_atual() {
        let (raiz, d) = montar("fila");
        d.importar_submissoes(
            "ead",
            &zip(&[("Ana Souza_1_assignsubmission_file_/main.py", "a"), ("Bruno Lima_2_assignsubmission_file_/main.py", "b")]),
            10,
        )
        .unwrap();
        assert!(pendentes(&d).is_empty(), "sem pipeline não há o que rodar");

        let (_, avisos) = d.salvar_pipeline_toml("ead", PIPELINE).unwrap();
        assert!(avisos.is_empty());
        assert_eq!(pendentes(&d), ["1", "2", "plágio"]);

        // o relatório tira o aluno da fila e já vem com a sugestão
        let sugestoes = d.salvar_relatorio("ead", "1", relatorio(&d, "1", Situacao::Passou), 20).unwrap();
        assert_eq!(sugestoes.len(), 1);
        assert_eq!(pendentes(&d), ["2", "plágio"]);
        assert_eq!(atual(&d, "1"), Some(true), "relatório atual");
        let gravado: Relatorio = ler_json(&d.pasta_relatorios("ead").unwrap().join("1.json")).unwrap().unwrap();
        assert_eq!(gravado.gerado_em, 20);

        // a cópia da rubrica muda (binária → nota livre): a sugestão acompanha sem rodar de novo
        std::fs::write(
            raiz.join("rubricas/codigo.toml"),
            "titulo = \"Código\"\n[[criterios]]\nnome = \"Funciona\"\nescala = { tipo = \"livre\", max = 2.0 }\n",
        )
        .unwrap();
        d.salvar_avaliacao(Some("ead"), "EAD".into(), "d".into(), 10.0, vec![("codigo".into(), 1.0)], &["codigo".into()]).unwrap();
        assert_eq!(atual(&d, "1"), Some(true));
        assert_eq!(d.panorama("ead").unwrap().sugestoes("1")[0].marcacao, crate::modelo::Marcacao::Nota { nota: 2.0 });

        // um executor com a regra de julgar antiga é recusado, e o aluno segue na fila
        let mut velho = relatorio(&d, "2", Situacao::Passou);
        velho.motor = "1".into();
        let Err(Erro::Conflito(m)) = d.salvar_relatorio("ead", "2", velho, 20) else { panic!("aceitou o motor antigo") };
        assert!(m.contains("atualize"), "{m}");
        assert_eq!(pendentes(&d), ["2", "plágio"]);

        // erro de execução não conta como feito
        d.salvar_relatorio("ead", "2", relatorio(&d, "2", Situacao::Erro), 20).unwrap();
        assert_eq!(pendentes(&d), ["2", "plágio"]);

        // submissão nova volta para a fila; o relatório antigo fica, marcado como desatualizado
        let velho: Relatorio = ler_json(&d.pasta_relatorios("ead").unwrap().join("1.json")).unwrap().unwrap();
        d.importar_submissoes("ead", &zip(&[("Ana Souza_1_assignsubmission_file_/main.py", "a2")]), 30).unwrap();
        assert_eq!(pendentes(&d), ["1", "2", "plágio"]);
        assert_eq!(atual(&d, "1"), Some(false));
        assert!(d.panorama("ead").unwrap().sugestoes("1").is_empty(), "sem sugestão de um relatório velho");
        // o relatório de quem rodou sobre o código antigo chega tarde: é recusado
        assert!(matches!(d.salvar_relatorio("ead", "1", velho, 31), Err(Erro::Conflito(_))));

        // editar o pipeline também
        d.salvar_relatorio("ead", "1", relatorio(&d, "1", Situacao::Passou), 40).unwrap();
        d.salvar_relatorio("ead", "2", relatorio(&d, "2", Situacao::Passou), 40).unwrap();
        assert_eq!(pendentes(&d), ["plágio"]);
        // a edição do professor põe o aluno de volta na fila; o plágio olha o original e não muda
        let plagio = |d: &Dados| d.fila().unwrap().into_iter().find_map(|t| match t {
            Tarefa::Plagio { versao, .. } => Some(versao),
            _ => None,
        });
        let antes = plagio(&d);
        d.editar_arquivo("ead", "1", "main.py", b"a3", "vini", 45).unwrap();
        assert_eq!(pendentes(&d), ["1", "plágio"]);
        assert_eq!(plagio(&d), antes);
        assert_eq!(atual(&d, "1"), Some(false));
        d.salvar_relatorio("ead", "1", relatorio(&d, "1", Situacao::Passou), 46).unwrap();
        assert_eq!(atual(&d, "1"), Some(true), "relatório da versão editada é o atual");

        d.salvar_pipeline_toml("ead", &PIPELINE.replace("saida = \"1\"", "saida = \"2\"")).unwrap();
        assert_eq!(pendentes(&d), ["1", "2", "plágio"]);

        // avaliação arquivada não roda mais nada; desarquivada, volta
        d.arquivar(Tipo::Avaliacoes, "ead", true).unwrap();
        assert!(pendentes(&d).is_empty());
        assert!(d.panorama("ead").unwrap().fila().is_empty(), "nada aguardando o executor");
        d.arquivar(Tipo::Avaliacoes, "ead", false).unwrap();
        assert_eq!(pendentes(&d), ["1", "2", "plágio"]);
        std::fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn plagio_guarda_os_pares_e_sai_da_fila_so_sem_erro() {
        let (raiz, d) = montar("plagio");
        d.importar_submissoes(
            "ead",
            &zip(&[("Ana Souza_1_assignsubmission_file_/main.py", "a"), ("Bruno Lima_2_assignsubmission_file_/main.py", "b")]),
            10,
        )
        .unwrap();
        d.salvar_pipeline_toml("ead", PIPELINE).unwrap();
        let versao = d.fila().unwrap().into_iter().find_map(|t| match t {
            Tarefa::Plagio { versao, .. } => Some(versao),
            _ => None,
        });
        let versao = versao.unwrap();
        let com_erro = ResultadoPlagio { versao: versao.clone(), gerado_em: 0, pares: vec![], erro: "JPlag falhou".into() };
        d.salvar_plagio("ead", com_erro, 5).unwrap();
        assert!(pendentes(&d).contains(&"plágio".to_string()));

        let par = Par { a: "1".into(), b: "2".into(), similaridade: 0.87 };
        d.salvar_plagio("ead", ResultadoPlagio { versao, gerado_em: 0, pares: vec![par.clone()], erro: String::new() }, 6).unwrap();
        assert!(!pendentes(&d).contains(&"plágio".to_string()));
        assert_eq!(d.panorama("ead").unwrap().plagio().unwrap().pares, [par]);
        d.salvar_jplag("ead", b"zip").unwrap();
        assert_eq!(d.jplag("ead").unwrap(), b"zip");
        std::fs::remove_dir_all(&raiz).unwrap();
    }
}
