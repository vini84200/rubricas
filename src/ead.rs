//! Rotas da correção de EAD: submissões do Moodle, pipelines, executor e a tela
//! "Acompanhar" (SSE).

use crate::api::{Erro, Estado, Res, Usuario, texto_puro};
use crate::auth;
use crate::fila::{ResultadoPlagio, Tarefa};
use crate::pipeline::Relatorio;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Extension, Json, Router};
use futures_util::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::watch;

/// O Cloudflare corta conexões ociosas em ~100 s.
const BATIMENTO: Duration = Duration::from_secs(30);
const MAX_ZIP: usize = 300 << 20;
const MAX_ANEXO: usize = 20 << 20;

/// O aluno que um usuário está avaliando agora, em qualquer aparelho.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Foco {
    pub avaliacao: String,
    pub aluno: String,
}

/// Avisos em memória para quem está ouvindo: o executor (fila mudou) e a tela
/// "Acompanhar" (o mesmo usuário trocou de aluno).
pub struct Avisos {
    fila: watch::Sender<u64>,
    focos: Mutex<HashMap<String, watch::Sender<Option<Foco>>>>,
    executor_visto_em: AtomicU64,
}

impl Avisos {
    pub fn novo() -> Self {
        Avisos { fila: watch::Sender::new(0), focos: Mutex::new(HashMap::new()), executor_visto_em: AtomicU64::new(0) }
    }

    /// Algo que o executor pode ter de rodar mudou.
    pub fn fila_mudou(&self) {
        self.fila.send_modify(|n| *n += 1);
    }

    fn foco(&self, usuario: &str) -> watch::Sender<Option<Foco>> {
        self.focos.lock().unwrap().entry(usuario.to_string()).or_insert_with(|| watch::Sender::new(None)).clone()
    }
}

pub fn rotas() -> Router<Estado> {
    Router::new()
        .route("/avaliacoes.txt", get(avaliacoes_texto))
        .route(
            "/avaliacoes/{id}/submissoes",
            get(listar_submissoes).post(importar).layer(DefaultBodyLimit::max(MAX_ZIP)),
        )
        .route("/avaliacoes/{id}/submissoes/sem-dono/{chave}", post(associar))
        .route("/avaliacoes/{id}/submissoes/{aluno}", get(submissao))
        .route("/avaliacoes/{id}/submissoes/{aluno}/{*caminho}", get(arquivo_submissao))
        .route("/avaliacoes/{id}/plagio", get(plagio))
        .route("/avaliacoes/{id}/plagio.jplag", get(baixar_jplag))
        .route("/pipelines.txt", get(pipelines_texto))
        .route("/pipelines/{id}", get(ler_pipeline))
        .route(
            "/pipelines/{id}/{*caminho}",
            get(ler_anexo).put(gravar_anexo).delete(apagar_anexo).layer(DefaultBodyLimit::max(MAX_ANEXO)),
        )
        .route("/executor/fila", get(fila))
        .route("/executor/eventos", get(eventos_executor))
        .route("/executor/relatorios/{id}/{aluno}", put(gravar_relatorio))
        .route("/executor/plagio/{id}", put(gravar_plagio))
        .route("/executor/plagio/{id}/jplag", put(gravar_jplag).layer(DefaultBodyLimit::max(MAX_ZIP)))
        .route("/foco", put(definir_foco))
        .route("/foco/eventos", get(eventos_foco))
}

/// O que a tela de avaliar precisa da parte de EAD, junto com o pacote (e
/// guardado para uso sem rede). O código e os relatórios inteiros vêm aluno a
/// aluno, por `GET /avaliacoes/{id}/submissoes/{aluno}`. `null` se a avaliação não usa nada disso.
pub fn resumo(e: &Estado, id: &str) -> Res<Value> {
    let d = &e.dados;
    let subs = d.submissoes(id)?;
    let pipeline = d.ler_pipeline(id);
    if subs.alunos.is_empty() && subs.sem_dono.is_empty() && matches!(pipeline, Ok(None)) {
        return Ok(Value::Null);
    }
    let submissoes: serde_json::Map<String, Value> = subs
        .alunos
        .iter()
        .map(|(a, s)| {
            let v = json!({ "alterada_em": s.alterada_em, "mudou": s.mudou_depois_da_correcao, "arquivos": s.arquivos.len() });
            (a.clone(), v)
        })
        .collect();
    let sem_dono: Vec<Value> =
        subs.sem_dono.iter().map(|s| json!({ "chave": s.chave, "nome": s.nome_moodle, "arquivos": s.arquivos.len() })).collect();
    let relatorios: serde_json::Map<String, Value> = d
        .relatorios(id)?
        .into_iter()
        .map(|(a, (r, atual))| {
            let checagens: Vec<Value> =
                r.checagens.iter().map(|c| json!({ "nome": c.nome, "situacao": c.situacao, "resumo": c.resumo })).collect();
            (a, json!({ "atual": atual, "checagens": checagens, "sugestoes": if atual { json!(r.sugestoes) } else { json!([]) } }))
        })
        .collect();
    // O par mais parecido de cada aluno, só acima do limiar.
    let tem_pipeline = matches!(pipeline, Ok(Some(_)));
    let mut plagio = serde_json::Map::new();
    if let (Ok(Some(p)), Some(r)) = (&pipeline, d.plagio(id)?)
        && let Some(cfg) = &p.plagio
    {
        for par in r.pares.iter().filter(|x| x.similaridade >= cfg.limiar) {
            for (eu, outro) in [(&par.a, &par.b), (&par.b, &par.a)] {
                if !eu.starts_with("referencias/") && !plagio.contains_key(eu.as_str()) {
                    plagio.insert(eu.clone(), json!({ "com": outro, "similaridade": par.similaridade }));
                }
            }
        }
    }
    let (fila, erro_pipeline) = match pipeline {
        Ok(Some(ref p)) => (d.fila_da_avaliacao(id, p)?, None),
        Ok(None) => (vec![], None),
        Err(e) => (vec![], Some(Erro::from(e).1)),
    };
    let aguardando = fila.iter().filter(|t| matches!(t, Tarefa::Checagens { .. })).count();
    let plagio_pendente = fila.iter().any(|t| matches!(t, Tarefa::Plagio { .. }));
    let visto = e.avisos.executor_visto_em.load(Ordering::Relaxed);
    Ok(json!({
        "submissoes": submissoes,
        "sem_dono": sem_dono,
        "relatorios": relatorios,
        "plagio": plagio,
        "pipeline": tem_pipeline,
        "erro_pipeline": erro_pipeline,
        "aguardando": aguardando,
        "plagio_pendente": plagio_pendente,
        "executor_visto_em": (visto > 0).then_some(visto),
    }))
}

/// Trabalho pesado de disco (ZIP, hashes) fora do executor assíncrono.
async fn bloqueante<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tokio::task::spawn_blocking(f).await.map_err(|_| Erro(StatusCode::INTERNAL_SERVER_ERROR, "Falha interna".into()))?
}

// ----- avaliações em texto (para o Claude) -----

async fn avaliacoes_texto(State(e): State<Estado>) -> Res<Response> {
    let mut texto = String::new();
    for a in e.dados.listar_avaliacoes()? {
        let rubricas: Vec<&str> = a.rubricas.iter().map(|r| r.origem.as_str()).collect();
        let pipeline = if e.dados.pipeline_toml(&a.id)?.is_some() { "com pipeline" } else { "sem pipeline" };
        texto.push_str(&format!("{} | {} | turma {} | rubricas: {} | {pipeline}\n", a.id, a.titulo, a.turma, rubricas.join(", ")));
    }
    Ok(texto_puro(texto))
}

// ----- submissões -----

async fn importar(State(e): State<Estado>, Path(id): Path<String>, corpo: Bytes) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    let r = bloqueante(move || Ok(dados.importar_submissoes(&id, &corpo, auth::agora())?)).await?;
    e.avisos.fila_mudou();
    Ok(Json(json!(r)))
}

async fn listar_submissoes(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    Ok(Json(json!(bloqueante(move || Ok(dados.submissoes(&id)?)).await?)))
}

#[derive(Deserialize)]
struct Associacao {
    aluno: String,
}

async fn associar(State(e): State<Estado>, Path((id, chave)): Path<(String, String)>, Json(a): Json<Associacao>) -> Res<Json<Value>> {
    e.dados.associar_sem_dono(&id, &chave, &a.aluno, auth::agora())?;
    e.avisos.fila_mudou();
    Ok(Json(json!({})))
}

/// Texto de um arquivo para o visualizador; `None` se é binário ou grande demais.
fn como_texto(bytes: &[u8]) -> Option<String> {
    if bytes.len() > 512 << 10 || bytes.contains(&0) {
        return None;
    }
    // Quem programa no Windows às vezes grava em Latin-1.
    Some(String::from_utf8(bytes.to_vec()).unwrap_or_else(|e| e.into_bytes().iter().map(|&b| b as char).collect()))
}

/// Código e relatório de um aluno, de uma vez: é o que o visualizador mostra.
async fn submissao(State(e): State<Estado>, Path((id, aluno)): Path<(String, String)>) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    bloqueante(move || {
        let sub = dados.submissoes(&id)?.alunos.remove(&aluno);
        let mut arquivos = Vec::new();
        for a in sub.as_ref().map(|s| s.arquivos.as_slice()).unwrap_or_default() {
            let texto = como_texto(&dados.arquivo_submissao(&id, &aluno, &a.caminho)?);
            arquivos.push(json!({ "caminho": a.caminho, "tamanho": a.tamanho, "texto": texto }));
        }
        let relatorio = dados.relatorio(&id, &aluno)?.map(|(r, atual)| json!({ "atual": atual, "relatorio": r }));
        Ok(Json(json!({ "submissao": sub, "arquivos": arquivos, "relatorio": relatorio })))
    })
    .await
}

fn baixar(bytes: Vec<u8>, nome: &str) -> Response {
    let mut r = bytes.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    let seguro: String = nome.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '_' }).collect();
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{seguro}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    r
}

async fn arquivo_submissao(State(e): State<Estado>, Path((id, aluno, caminho)): Path<(String, String, String)>) -> Res<Response> {
    let nome = caminho.rsplit('/').next().unwrap_or("arquivo").to_string();
    Ok(baixar(e.dados.arquivo_submissao(&id, &aluno, &caminho)?, &nome))
}

async fn plagio(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    let limiar = e.dados.ler_pipeline(&id).ok().flatten().and_then(|p| p.plagio).map(|p| p.limiar);
    Ok(Json(json!({ "resultado": e.dados.plagio(&id)?, "limiar": limiar })))
}

async fn baixar_jplag(State(e): State<Estado>, Path(id): Path<String>) -> Res<Response> {
    Ok(baixar(e.dados.jplag(&id)?, &format!("{id}.jplag")))
}

// ----- pipelines -----

/// Uma linha por avaliação com pipeline: id | checagens | anexos | pendentes.
async fn pipelines_texto(State(e): State<Estado>) -> Res<Response> {
    let dados = e.dados.clone();
    let texto = bloqueante(move || {
        let mut texto = String::new();
        for a in dados.listar_avaliacoes()? {
            if dados.pipeline_toml(&a.id)?.is_none() {
                continue;
            }
            let anexos: Vec<String> =
                dados.anexos_pipeline(&a.id)?.into_iter().map(|x| x.caminho).filter(|c| c != "pipeline.toml").collect();
            let linha = match dados.ler_pipeline(&a.id) {
                Ok(Some(p)) => {
                    let pendentes = dados.fila_da_avaliacao(&a.id, &p)?.len();
                    format!("{} checagens | anexos: {} | {pendentes} pendentes", p.checagens.len(), anexos.join(", "))
                }
                Ok(None) => continue,
                Err(err) => format!("! {}", Erro::from(err).1),
            };
            texto.push_str(&format!("{} | {linha}\n", a.id));
        }
        Ok(texto)
    })
    .await?;
    Ok(texto_puro(texto))
}

/// Para a tela: o texto, o pipeline lido (ou o erro), os anexos e quantos faltam rodar.
async fn ler_pipeline(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    let visto = e.avisos.executor_visto_em.load(Ordering::Relaxed);
    bloqueante(move || {
        dados.ler_avaliacao(&id)?;
        let toml = dados.pipeline_toml(&id)?;
        let (pipeline, erro, aguardando) = match dados.ler_pipeline(&id) {
            Ok(Some(p)) => {
                let n = dados.fila_da_avaliacao(&id, &p)?.len();
                (Some(p), None, n)
            }
            Ok(None) => (None, None, 0),
            Err(err) => (None, Some(Erro::from(err).1), 0),
        };
        let anexos: Vec<_> = dados.anexos_pipeline(&id)?.into_iter().filter(|a| a.caminho != "pipeline.toml").collect();
        Ok(Json(json!({
            "toml": toml, "pipeline": pipeline, "erro": erro, "anexos": anexos,
            "aguardando": aguardando, "executor_visto_em": (visto > 0).then_some(visto),
        })))
    })
    .await
}

async fn ler_anexo(State(e): State<Estado>, Path((id, caminho)): Path<(String, String)>) -> Res<Response> {
    let bytes = e.dados.anexo_pipeline(&id, &caminho)?;
    if caminho == "pipeline.toml" || caminho.ends_with(".py") || caminho.ends_with(".txt") {
        return Ok(texto_puro(String::from_utf8_lossy(&bytes).into_owned()));
    }
    Ok(baixar(bytes, caminho.rsplit('/').next().unwrap_or("anexo")))
}

/// `pipeline.toml` é validado contra a avaliação; os outros arquivos são anexos.
async fn gravar_anexo(State(e): State<Estado>, Path((id, caminho)): Path<(String, String)>, corpo: Bytes) -> Res<Response> {
    let resposta = if caminho == "pipeline.toml" {
        let texto = String::from_utf8(corpo.to_vec()).map_err(|_| Erro(StatusCode::UNPROCESSABLE_ENTITY, "O arquivo não está em UTF-8".into()))?;
        let (p, avisos) = e.dados.salvar_pipeline_toml(&id, &texto)?;
        let mut linha = format!("Gravado: pipelines/{id}/pipeline.toml ({} checagens)", p.checagens.len());
        for a in avisos {
            linha.push_str(&format!("\nAtenção: {a}"));
        }
        linha
    } else {
        e.dados.salvar_anexo_pipeline(&id, &caminho, &corpo)?;
        format!("Gravado: pipelines/{id}/{caminho} ({} bytes)", corpo.len())
    };
    e.avisos.fila_mudou();
    Ok(texto_puro(resposta + "\n"))
}

async fn apagar_anexo(State(e): State<Estado>, Path((id, caminho)): Path<(String, String)>) -> Res<Response> {
    e.dados.apagar_anexo_pipeline(&id, &caminho)?;
    e.avisos.fila_mudou();
    Ok(texto_puro(format!("Apagado: pipelines/{id}/{caminho}\n")))
}

// ----- executor -----

async fn fila(State(e): State<Estado>) -> Res<Json<Value>> {
    e.avisos.executor_visto_em.store(auth::agora(), Ordering::Relaxed);
    let dados = e.dados.clone();
    let tarefas: Vec<Tarefa> = bloqueante(move || Ok(dados.fila()?)).await?;
    Ok(Json(json!({ "tarefas": tarefas })))
}

/// Um evento `fila` a cada mudança que pode dar trabalho ao executor.
async fn eventos_executor(State(e): State<Estado>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    e.avisos.executor_visto_em.store(auth::agora(), Ordering::Relaxed);
    let rx = e.avisos.fila.subscribe();
    let mudancas = stream::unfold(rx, |mut rx| async move {
        rx.changed().await.ok()?;
        let n = *rx.borrow_and_update();
        Some((Ok(Event::default().event("fila").data(n.to_string())), rx))
    });
    Sse::new(mudancas).keep_alive(KeepAlive::new().interval(BATIMENTO))
}

async fn gravar_relatorio(
    State(e): State<Estado>,
    Path((id, aluno)): Path<(String, String)>,
    Json(r): Json<Relatorio>,
) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    let r = bloqueante(move || Ok(dados.salvar_relatorio(&id, &aluno, r, auth::agora())?)).await?;
    Ok(Json(json!({ "sugestoes": r.sugestoes.len() })))
}

async fn gravar_plagio(State(e): State<Estado>, Path(id): Path<String>, Json(r): Json<ResultadoPlagio>) -> Res<Json<Value>> {
    e.dados.salvar_plagio(&id, r, auth::agora())?;
    Ok(Json(json!({})))
}

async fn gravar_jplag(State(e): State<Estado>, Path(id): Path<String>, corpo: Bytes) -> Res<Json<Value>> {
    e.dados.salvar_jplag(&id, &corpo)?;
    Ok(Json(json!({})))
}

// ----- acompanhar -----

async fn definir_foco(State(e): State<Estado>, Extension(u): Extension<Usuario>, Json(f): Json<Foco>) -> Json<Value> {
    e.avisos.foco(&u.0).send_if_modified(|atual| {
        let mudou = atual.as_ref() != Some(&f);
        *atual = Some(f);
        mudou
    });
    Json(json!({}))
}

/// O foco atual na hora de conectar e um evento a cada troca de aluno.
async fn eventos_foco(State(e): State<Estado>, Extension(u): Extension<Usuario>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = e.avisos.foco(&u.0).subscribe();
    let evento = |f: &Option<Foco>| Ok(Event::default().event("foco").json_data(f).unwrap_or_default());
    let primeiro = evento(&rx.borrow_and_update());
    let mudancas = stream::unfold(rx, move |mut rx| async move {
        rx.changed().await.ok()?;
        let ev = evento(&rx.borrow_and_update());
        Some((ev, rx))
    });
    Sse::new(stream::once(async move { primeiro }).chain(mudancas)).keep_alive(KeepAlive::new().interval(BATIMENTO))
}
