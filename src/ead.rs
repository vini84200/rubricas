//! Rotas da correção de EAD: submissões do Moodle, pipelines, executor e a tela
//! "Acompanhar" (SSE).

use crate::api::{Erro, Estado, Exigencia, Res, Rotas, bloqueante, marca_arquivada, metodos, texto_puro};
use crate::permissoes::{Acao, Quem, Recurso};
use crate::auth;
use crate::dados::{Dados, Tipo};
use crate::efeitos;
use crate::fila::{EstadoAluno, EstadoPipeline, ResultadoPlagio, Tarefa};
use crate::moodle;
use crate::pipeline::{Pipeline, Relatorio};
use crate::submissoes::{InfoArquivo, Submissao};
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
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

/// Avisos em memória para a tela "Acompanhar" (o mesmo usuário trocou de
/// aluno) e sobre o executor. As mudanças na fila vêm de `Dados::ouvir_fila`.
pub struct Avisos {
    focos: Mutex<HashMap<String, watch::Sender<Option<Foco>>>>,
    executor_visto_em: AtomicU64,
    /// Vira `true` quando o servidor vai parar: as conexões de eventos terminam.
    encerrando: watch::Sender<bool>,
}

impl Avisos {
    pub fn novo() -> Self {
        Avisos { focos: Mutex::new(HashMap::new()), executor_visto_em: AtomicU64::new(0), encerrando: watch::Sender::new(false) }
    }

    pub fn encerrar(&self) {
        self.encerrando.send_replace(true);
    }

    /// Termina quando o servidor vai parar.
    pub fn encerramento(&self) -> impl Future<Output = ()> + use<> {
        let mut rx = self.encerrando.subscribe();
        async move {
            let _ = rx.wait_for(|e| *e).await;
        }
    }

    fn foco(&self, usuario: &str) -> watch::Sender<Option<Foco>> {
        self.focos.lock().unwrap().entry(usuario.to_string()).or_insert_with(|| watch::Sender::new(None)).clone()
    }
}

pub fn rotas() -> Rotas {
    use Acao::*;
    use Exigencia::*;
    use Tipo::Avaliacoes;
    // Editar o código do aluno faz parte de corrigir. Ler submissões e pipeline é
    // o que o executor precisa (e quem vê a avaliação também pode).
    let ler = Item(Avaliacoes, LerChecagens);
    let executar = Sistema(Executar);
    Rotas::new()
        .rota("/avaliacoes.txt", metodos().get(Dentro, avaliacoes_texto))
        .rota("/avaliacoes/{id}/submissoes", metodos().get(ler, listar_submissoes).post(Item(Avaliacoes, Editar), importar).limite(MAX_ZIP))
        .rota("/avaliacoes/{id}/submissoes/sem-dono/{chave}", metodos().post(Item(Avaliacoes, Editar), associar))
        .rota("/avaliacoes/{id}/planilha-moodle", metodos().post(Item(Avaliacoes, Exportar), planilha_moodle))
        .rota("/avaliacoes/{id}/envios", metodos().post(Item(Avaliacoes, Editar), envios))
        .rota("/avaliacoes/{id}/submissoes/{aluno}", metodos().get(ler, submissao))
        .rota(
            "/avaliacoes/{id}/submissoes/{aluno}/{*caminho}",
            metodos()
                .get(ler, arquivo_submissao)
                .put(Item(Avaliacoes, Corrigir), editar_arquivo)
                .delete(Item(Avaliacoes, Corrigir), desfazer_edicao)
                .limite(MAX_ANEXO),
        )
        .rota("/avaliacoes/{id}/plagio", metodos().get(Item(Avaliacoes, Ver), plagio))
        .rota("/avaliacoes/{id}/plagio.jplag", metodos().get(Item(Avaliacoes, Ver), baixar_jplag))
        .rota("/pipelines.txt", metodos().get(Dentro, pipelines_texto))
        .rota("/pipelines/{id}", metodos().get(ler, ler_pipeline))
        .rota(
            "/pipelines/{id}/{*caminho}",
            metodos()
                .get(ler, ler_anexo)
                .put(Item(Avaliacoes, Editar), gravar_anexo)
                .delete(Item(Avaliacoes, Editar), apagar_anexo)
                .limite(MAX_ANEXO),
        )
        .rota("/executor/fila", metodos().get(executar, fila))
        .rota("/executor/eventos", metodos().get(executar, eventos_executor))
        .rota("/executor/relatorios/{id}/{aluno}", metodos().put(executar, gravar_relatorio))
        .rota("/executor/plagio/{id}", metodos().put(executar, gravar_plagio))
        .rota("/executor/plagio/{id}/jplag", metodos().put(executar, gravar_jplag).limite(MAX_ZIP))
        .rota("/foco", metodos().put(Dentro, definir_foco))
        .rota("/foco/eventos", metodos().get(Dentro, eventos_foco))
}

/// O que a tela de avaliar precisa da parte de EAD, junto com o pacote (e
/// guardado para uso sem rede). O código e os relatórios inteiros vêm aluno a
/// aluno, por `GET /avaliacoes/{id}/submissoes/{aluno}`. `null` se a avaliação não usa nada disso.
pub fn resumo(e: &Estado, id: &str) -> Res<Value> {
    let panorama = e.dados.panorama(id)?;
    let subs = &panorama.submissoes;
    if subs.alunos.is_empty() && subs.sem_dono.is_empty() && matches!(panorama.pipeline, EstadoPipeline::Ausente) {
        return Ok(Value::Null);
    }
    let submissoes: serde_json::Map<String, Value> = subs
        .alunos
        .iter()
        .map(|(a, s)| {
            let v = json!({
                "alterada_em": s.alterada_em,
                "mudou": s.mudou_depois_da_correcao,
                "arquivos": s.arquivos.len(),
                "edicao": s.edicao,
            });
            (a.clone(), v)
        })
        .collect();
    let sem_dono: Vec<Value> =
        subs.sem_dono.iter().map(|s| json!({ "chave": s.chave, "nome": s.nome_moodle, "arquivos": s.arquivos.len() })).collect();
    let relatorios: serde_json::Map<String, Value> = panorama
        .relatorios()
        .map(|(a, r, atual)| {
            let checagens: Vec<Value> =
                r.checagens.iter().map(|c| json!({ "nome": c.nome, "situacao": c.situacao, "resumo": c.resumo })).collect();
            (a.to_string(), json!({ "atual": atual, "checagens": checagens, "sugestoes": panorama.sugestoes(a) }))
        })
        .collect();
    // O par mais parecido de cada aluno, só acima do limiar.
    let mut plagio = serde_json::Map::new();
    if let (EstadoPipeline::Valido(p), Some(r)) = (&panorama.pipeline, panorama.plagio())
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
    let erro_pipeline = match &panorama.pipeline {
        EstadoPipeline::Invalido(m) => Some(m),
        _ => None,
    };
    let fila = panorama.fila();
    let aguardando = fila.iter().filter(|t| matches!(t, Tarefa::Checagens { .. })).count();
    let plagio_pendente = fila.iter().any(|t| matches!(t, Tarefa::Plagio { .. }));
    let visto = e.avisos.executor_visto_em.load(Ordering::Relaxed);
    Ok(json!({
        "submissoes": submissoes,
        "sem_dono": sem_dono,
        "relatorios": relatorios,
        "plagio": plagio,
        "pipeline": matches!(panorama.pipeline, EstadoPipeline::Valido(_)),
        "erro_pipeline": erro_pipeline,
        "aguardando": aguardando,
        "plagio_pendente": plagio_pendente,
        "executor_visto_em": (visto > 0).then_some(visto),
    }))
}

// ----- avaliações em texto (para o Claude) -----

async fn avaliacoes_texto(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Response> {
    let mut texto = String::new();
    let arquivadas = e.dados.arquivadas();
    let visao = e.visao(&u);
    for a in e.dados.listar_avaliacoes()? {
        if !visao.pode(Recurso::Item(Tipo::Avaliacoes, &a.id), Acao::Ver) {
            continue;
        }
        let rubricas: Vec<&str> = a.rubricas.iter().map(|r| r.origem.as_str()).collect();
        let pipeline = if e.dados.pipeline_toml(&a.id)?.is_some() { "com pipeline" } else { "sem pipeline" };
        let arquivada = marca_arquivada(arquivadas.contem(Tipo::Avaliacoes, &a.id));
        texto.push_str(&format!("{} | {} | turma {} | rubricas: {} | {pipeline}{arquivada}\n", a.id, a.titulo, a.turma, rubricas.join(", ")));
    }
    Ok(texto_puro(texto))
}

// ----- submissões -----

async fn importar(State(e): State<Estado>, Path(id): Path<String>, corpo: Bytes) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    let r = bloqueante(move || Ok(dados.importar_submissoes(&id, &corpo, auth::agora())?)).await?;
    Ok(Json(json!(r)))
}

fn texto_da_planilha(corpo: &[u8]) -> Res<String> {
    como_texto(corpo).ok_or(Erro(StatusCode::UNPROCESSABLE_ENTITY, "Isto não parece a planilha de notas do Moodle".into()))
}

/// Lê as datas de envio da planilha e grava; devolve o resumo, com quantos
/// enviaram depois do prazo (mais a tolerância).
fn registrar_envios(dados: &Dados, id: &str, texto: &str) -> Res<Value> {
    let av = dados.ler_avaliacao(id)?;
    let turma = dados.ler_turma(&av.turma)?;
    let apelidos = dados.apelidos(&turma.id)?;
    let r = moodle::datas_de_envio(texto, &turma, &apelidos).map_err(|m| Erro(StatusCode::UNPROCESSABLE_ENTITY, m))?;
    dados.gravar_envios(id, &r.datas)?;
    let prazo = av.prazo.as_deref().and_then(efeitos::minutos);
    let atrasados = prazo.map(|p| {
        let limite = p + i64::from(av.tolerancia);
        r.datas.values().flatten().filter(|d| efeitos::minutos(d).is_some_and(|m| m > limite)).count()
    });
    let mut v = json!(r);
    v["atrasados"] = json!(atrasados);
    Ok(v)
}

/// `POST /avaliacoes/{id}/envios`: a planilha de notas do Moodle, só pelas datas.
async fn envios(State(e): State<Estado>, Path(id): Path<String>, corpo: Bytes) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    Ok(Json(bloqueante(move || registrar_envios(&dados, &id, &texto_da_planilha(&corpo)?)).await?))
}

/// Recebe a planilha de notas da tarefa do Moodle e a devolve preenchida, com o
/// resumo do que ficou de fora; a tela baixa o `csv` como arquivo. Para quem
/// pode alterar a avaliação, as datas de envio da planilha também são gravadas.
async fn planilha_moodle(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>, corpo: Bytes) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    let gravar_datas = e.visao(&u).pode(Recurso::Item(Tipo::Avaliacoes, &id), Acao::Editar);
    let p = bloqueante(move || {
        let av = dados.ler_avaliacao(&id)?;
        let turma = dados.ler_turma(&av.turma)?;
        let correcoes = dados.ler_correcoes(&id)?;
        let apelidos = dados.apelidos(&turma.id)?;
        let texto = texto_da_planilha(&corpo)?;
        let p = moodle::preencher_planilha(&texto, &av, &turma, &correcoes, &apelidos).map_err(|m| Erro(StatusCode::UNPROCESSABLE_ENTITY, m))?;
        // Uma planilha sem a coluna de envio, ou ilegível, não impede o preenchimento.
        if gravar_datas && let Err(Erro(_, m)) = registrar_envios(&dados, &id, &texto) {
            eprintln!("datas de envio de {id} não gravadas: {m}");
        }
        Ok(p)
    })
    .await?;
    Ok(Json(json!(p)))
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

/// `GET /avaliacoes/{id}/submissoes/{aluno}`: código e relatório de um aluno, de
/// uma vez. É o que o visualizador mostra e de onde o executor tira os arquivos.
#[derive(Serialize, Deserialize)]
pub struct RespostaSubmissao {
    pub submissao: Option<Submissao>,
    pub arquivos: Vec<ArquivoDaSubmissao>,
    pub relatorio: Option<RelatorioDoAluno>,
}

#[derive(Serialize, Deserialize)]
pub struct ArquivoDaSubmissao {
    pub caminho: String,
    pub tamanho: u64,
    /// `None` se é binário ou grande demais.
    pub texto: Option<String>,
    pub editado: bool,
    /// Como o aluno enviou, quando o professor editou.
    pub original: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RelatorioDoAluno {
    pub atual: bool,
    pub relatorio: Relatorio,
}

fn json_submissao(dados: &Dados, id: &str, aluno: &str) -> Res<Json<RespostaSubmissao>> {
    let panorama = dados.panorama(id)?;
    let sub = panorama.submissoes.alunos.get(aluno).cloned();
    let editados = sub.as_ref().and_then(|s| s.edicao.as_ref()).map(|e| e.arquivos.clone()).unwrap_or_default();
    let mut arquivos = Vec::new();
    for a in sub.as_ref().map(|s| s.arquivos.as_slice()).unwrap_or_default() {
        let editado = editados.contains(&a.caminho);
        arquivos.push(ArquivoDaSubmissao {
            caminho: a.caminho.clone(),
            tamanho: a.tamanho,
            texto: como_texto(&dados.arquivo_submissao(id, aluno, &a.caminho)?),
            editado,
            original: if editado { como_texto(&dados.arquivo_original(id, aluno, &a.caminho)?) } else { None },
        });
    }
    let relatorio = match panorama.estado(aluno) {
        Some(EstadoAluno::Atual(r)) => Some(RelatorioDoAluno { atual: true, relatorio: r.clone() }),
        Some(EstadoAluno::Desatualizado(r)) => Some(RelatorioDoAluno { atual: false, relatorio: r.clone() }),
        Some(EstadoAluno::SemRelatorio) | None => None,
    };
    Ok(Json(RespostaSubmissao { submissao: sub, arquivos, relatorio }))
}

async fn submissao(State(e): State<Estado>, Path((id, aluno)): Path<(String, String)>) -> Res<Json<RespostaSubmissao>> {
    let dados = e.dados.clone();
    bloqueante(move || json_submissao(&dados, &id, &aluno)).await
}

/// Mudança pontual do professor no código do aluno; as checagens rodam de novo.
async fn editar_arquivo(
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path((id, aluno, caminho)): Path<(String, String, String)>,
    corpo: Bytes,
) -> Res<Json<RespostaSubmissao>> {
    let dados = e.dados.clone();
    bloqueante(move || {
        dados.editar_arquivo(&id, &aluno, &caminho, &corpo, &u.nome, auth::agora())?;
        json_submissao(&dados, &id, &aluno)
    })
    .await
}

/// Volta o arquivo ao que o aluno enviou.
async fn desfazer_edicao(
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path((id, aluno, caminho)): Path<(String, String, String)>,
) -> Res<Json<RespostaSubmissao>> {
    let dados = e.dados.clone();
    bloqueante(move || {
        let original = dados.arquivo_original(&id, &aluno, &caminho)?;
        dados.editar_arquivo(&id, &aluno, &caminho, &original, &u.nome, auth::agora())?;
        json_submissao(&dados, &id, &aluno)
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

#[derive(Deserialize)]
struct QualVersao {
    /// `original=1`: como o aluno enviou, sem as edições do professor.
    #[serde(default)]
    original: Option<String>,
}

async fn arquivo_submissao(
    State(e): State<Estado>,
    Path((id, aluno, caminho)): Path<(String, String, String)>,
    Query(q): Query<QualVersao>,
) -> Res<Response> {
    let nome = caminho.rsplit('/').next().unwrap_or("arquivo").to_string();
    let bytes = match q.original.is_some_and(|v| v != "0") {
        true => e.dados.arquivo_original(&id, &aluno, &caminho)?,
        false => e.dados.arquivo_submissao(&id, &aluno, &caminho)?,
    };
    Ok(baixar(bytes, &nome))
}

async fn plagio(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    let panorama = e.dados.panorama(&id)?;
    let limiar = match &panorama.pipeline {
        EstadoPipeline::Valido(p) => p.plagio.as_ref().map(|p| p.limiar),
        _ => None,
    };
    Ok(Json(json!({ "resultado": panorama.plagio(), "limiar": limiar })))
}

async fn baixar_jplag(State(e): State<Estado>, Path(id): Path<String>) -> Res<Response> {
    Ok(baixar(e.dados.jplag(&id)?, &format!("{id}.jplag")))
}

// ----- pipelines -----

/// Uma linha por avaliação com pipeline: id | checagens | anexos | pendentes.
async fn pipelines_texto(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Response> {
    let dados = e.dados.clone();
    let permissoes = e.permissoes.clone();
    let texto = bloqueante(move || {
        let visao = permissoes.visao(&u);
        let mut texto = String::new();
        for a in dados.listar_avaliacoes()?.into_iter().filter(|a| visao.pode(Recurso::Item(Tipo::Avaliacoes, &a.id), Acao::Ver)) {
            if dados.pipeline_toml(&a.id)?.is_none() {
                continue;
            }
            let anexos: Vec<String> =
                dados.anexos_pipeline(&a.id)?.into_iter().map(|x| x.caminho).filter(|c| c != "pipeline.toml").collect();
            let panorama = dados.panorama(&a.id)?;
            let linha = match &panorama.pipeline {
                EstadoPipeline::Valido(p) => {
                    format!("{} checagens | anexos: {} | {} pendentes", p.checagens.len(), anexos.join(", "), panorama.fila().len())
                }
                EstadoPipeline::Ausente => continue,
                EstadoPipeline::Invalido(m) => format!("! {m}"),
            };
            texto.push_str(&format!("{} | {linha}\n", a.id));
        }
        Ok(texto)
    })
    .await?;
    Ok(texto_puro(texto))
}

/// `GET /pipelines/{id}`, para a tela e para o executor: o texto, o pipeline
/// lido (ou o erro), os anexos e quantos faltam rodar.
#[derive(Serialize, Deserialize)]
pub struct RespostaPipeline {
    pub toml: Option<String>,
    pub pipeline: Option<Pipeline>,
    pub erro: Option<String>,
    /// Sem o `pipeline.toml`.
    pub anexos: Vec<InfoArquivo>,
    pub aguardando: usize,
    pub executor_visto_em: Option<u64>,
}

async fn ler_pipeline(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<RespostaPipeline>> {
    let dados = e.dados.clone();
    let visto = e.avisos.executor_visto_em.load(Ordering::Relaxed);
    bloqueante(move || {
        let toml = dados.pipeline_toml(&id)?;
        let panorama = dados.panorama(&id)?;
        let aguardando = panorama.fila().len();
        let (pipeline, erro) = match panorama.pipeline {
            EstadoPipeline::Valido(p) => (Some(p), None),
            EstadoPipeline::Ausente => (None, None),
            EstadoPipeline::Invalido(m) => (None, Some(m)),
        };
        let anexos = dados.anexos_pipeline(&id)?.into_iter().filter(|a| a.caminho != "pipeline.toml").collect();
        Ok(Json(RespostaPipeline { toml, pipeline, erro, anexos, aguardando, executor_visto_em: (visto > 0).then_some(visto) }))
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
    Ok(texto_puro(resposta + "\n"))
}

async fn apagar_anexo(State(e): State<Estado>, Path((id, caminho)): Path<(String, String)>) -> Res<Response> {
    e.dados.apagar_anexo_pipeline(&id, &caminho)?;
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
    let rx = e.dados.ouvir_fila();
    let mudancas = stream::unfold(rx, |mut rx| async move {
        rx.changed().await.ok()?;
        let n = *rx.borrow_and_update();
        Some((Ok(Event::default().event("fila").data(n.to_string())), rx))
    });
    Sse::new(mudancas.take_until(e.avisos.encerramento())).keep_alive(KeepAlive::new().interval(BATIMENTO))
}

async fn gravar_relatorio(
    State(e): State<Estado>,
    Path((id, aluno)): Path<(String, String)>,
    Json(r): Json<Relatorio>,
) -> Res<Json<Value>> {
    let dados = e.dados.clone();
    let sugestoes = bloqueante(move || Ok(dados.salvar_relatorio(&id, &aluno, r, auth::agora())?)).await?;
    Ok(Json(json!({ "sugestoes": sugestoes.len() })))
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

async fn definir_foco(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(f): Json<Foco>) -> Json<Value> {
    e.avisos.foco(&u.nome).send_if_modified(|atual| {
        let mudou = atual.as_ref() != Some(&f);
        *atual = Some(f);
        mudou
    });
    Json(json!({}))
}

/// O foco atual na hora de conectar e um evento a cada troca de aluno.
async fn eventos_foco(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = e.avisos.foco(&u.nome).subscribe();
    let evento = |f: &Option<Foco>| Ok(Event::default().event("foco").json_data(f).unwrap_or_default());
    let primeiro = evento(&rx.borrow_and_update());
    let mudancas = stream::unfold(rx, move |mut rx| async move {
        rx.changed().await.ok()?;
        let ev = evento(&rx.borrow_and_update());
        Some((ev, rx))
    });
    let eventos = stream::once(async move { primeiro }).chain(mudancas).take_until(e.avisos.encerramento());
    Sse::new(eventos).keep_alive(KeepAlive::new().interval(BATIMENTO))
}
