//! Rotas HTTP. Tudo sob `/api` exige sessão (cookie) ou token de API
//! (`Authorization: Bearer …`), menos `/api/entrar`. Cada rota protegida diz em
//! `exigencia` o que é preciso poder para usá-la; rota fora da tabela é negada.

use crate::auth::{self, Auth, ErroLogin};
use crate::dados::{self, Arquivadas, Dados, Tipo};
use crate::ead;
use crate::estatico::Web;
use crate::modelo::*;
use crate::permissoes::{Permissoes, Quem, Recurso, Visao};
use crate::contas;
use axum::extract::{FromRequestParts, MatchedPath, Path, Query, RawPathParams, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub struct Estado {
    pub dados: Arc<Dados>,
    pub auth: Arc<Auth>,
    pub web: Arc<Web>,
    pub avisos: Arc<ead::Avisos>,
    pub permissoes: Arc<Permissoes>,
}

impl Estado {
    /// O que `quem` pode fazer, com os acessos lidos uma vez só.
    pub(crate) fn visao(&self, quem: &Quem) -> Visao<'_> {
        self.permissoes.visao(quem)
    }

    /// O item pede `acao`, ou 403 com uma frase que diz o que falta.
    pub(crate) fn exigir(&self, quem: &Quem, tipo: Tipo, id: &str, acao: &str) -> Res<()> {
        let v = self.visao(quem);
        if v.pode(Recurso::Item(tipo, id), acao) { Ok(()) } else { Err(sem_permissao(&v, tipo, id, acao)) }
    }

    /// Quem cria um item vira dono dele.
    pub(crate) fn tornar_dono(&self, quem: &Quem, tipo: Tipo, id: &str) -> Res<()> {
        self.permissoes
            .conceder(tipo, id, &quem.nome, "dono", &quem.nome, auth::agora())
            .map_err(|m| Erro(StatusCode::INTERNAL_SERVER_ERROR, m))
    }

    pub(crate) fn esquecer(&self, tipo: Tipo, id: &str) {
        if let Err(m) = self.permissoes.esquecer_item(tipo, id) {
            eprintln!("acessos: {m}");
        }
    }
}

fn nome_do_item(tipo: Tipo) -> &'static str {
    match tipo {
        Tipo::Avaliacoes => "esta avaliação",
        Tipo::Rubricas => "esta rubrica",
        Tipo::Turmas => "esta turma",
    }
}

fn sem_permissao(v: &Visao, tipo: Tipo, id: &str, acao: &str) -> Erro {
    let item = nome_do_item(tipo);
    let m = if !v.pode(Recurso::Item(tipo, id), "ver") {
        format!("Você não tem acesso a {item}. Peça a quem a criou para compartilhar com você.")
    } else {
        let acao = match acao {
            "exportar" => "baixar as notas",
            "corrigir" => "corrigir",
            "editar" => "alterar",
            "ler_checagens" => "ler as checagens",
            outra => outra,
        };
        format!("Seu acesso a {item} não permite {acao}.")
    };
    Erro(StatusCode::FORBIDDEN, m)
}

pub(crate) struct Erro(pub StatusCode, pub String);

impl IntoResponse for Erro {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "erro": self.1 }))).into_response()
    }
}

impl From<dados::Erro> for Erro {
    fn from(e: dados::Erro) -> Self {
        match e {
            dados::Erro::NaoEncontrado(m) => Erro(StatusCode::NOT_FOUND, m),
            dados::Erro::Invalido(m) => Erro(StatusCode::UNPROCESSABLE_ENTITY, m),
            dados::Erro::Conflito(m) => Erro(StatusCode::CONFLICT, m),
            dados::Erro::Io(e) => {
                eprintln!("erro de disco: {e}");
                Erro(StatusCode::INTERNAL_SERVER_ERROR, format!("Não foi possível acessar a pasta de dados: {e}"))
            }
        }
    }
}

pub(crate) type Res<T> = Result<T, Erro>;

pub fn rotas(estado: Estado) -> Router {
    let protegidas = Router::new()
        .route("/sessao", get(sessao))
        .route("/rubricas", get(listar_rubricas).post(criar_rubrica))
        .route("/rubricas.txt", get(listar_rubricas_texto))
        .route("/rubricas/{id}", get(ler_rubrica).put(salvar_rubrica).delete(apagar_rubrica))
        .route("/turmas", get(listar_turmas).post(criar_turma))
        .route("/turmas/{id}", get(ler_turma).put(salvar_turma).delete(apagar_turma))
        .route("/avaliacoes", get(listar_avaliacoes).post(criar_avaliacao))
        .route("/avaliacoes/{id}", get(pacote).put(salvar_avaliacao).delete(apagar_avaliacao))
        .route("/avaliacoes/{id}/correcoes/{aluno}", patch(alterar_correcao))
        .route("/avaliacoes/{id}/notas.csv", get(exportar))
        .route("/rubricas/{id}/arquivada", put(|e, p, j| arquivar(Tipo::Rubricas, e, p, j)))
        .route("/turmas/{id}/arquivada", put(|e, p, j| arquivar(Tipo::Turmas, e, p, j)))
        .route("/avaliacoes/{id}/arquivada", put(|e, p, j| arquivar(Tipo::Avaliacoes, e, p, j)))
        .merge(ead::rotas())
        .merge(contas::rotas())
        .route_layer(middleware::from_fn_with_state(estado.clone(), exigir_sessao));
    let api = Router::new()
        .route("/entrar", post(entrar))
        .route("/sair", post(sair))
        .merge(protegidas)
        .fallback(|| async { Erro(StatusCode::NOT_FOUND, "Rota desconhecida".into()) })
        .layer(middleware::from_fn(erros_em_json));
    Router::new().nest("/api", api).fallback(estatico).layer(middleware::from_fn(cabecalhos)).with_state(estado)
}

async fn estatico(State(e): State<Estado>, uri: Uri, headers: HeaderMap) -> Response {
    e.web.servir(uri.path(), &headers)
}

async fn cabecalhos(req: Request, next: Next) -> Response {
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    );
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    r
}

/// Recusas do próprio axum (JSON malformado, tipo de conteúdo errado) saem como
/// texto; aqui viram o mesmo `{ "erro": … }` das demais respostas.
async fn erros_em_json(req: Request, next: Next) -> Response {
    let r = next.run(req).await;
    let texto = r.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("text/plain"));
    if r.status().is_success() || !texto {
        return r;
    }
    let status = r.status();
    let detalhe = axum::body::to_bytes(r.into_body(), 2048).await.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    Erro(status, format!("Pedido inválido: {}", detalhe.trim())).into_response()
}

fn token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|par| par.trim().split_once('='))
        .find(|(nome, _)| *nome == auth::COOKIE)
        .map(|(_, valor)| valor.to_string())
}

fn cookie(headers: &HeaderMap, valor: &str, duracao: u64) -> HeaderValue {
    // Atrás de um proxy HTTPS o cookie só trafega cifrado.
    let https = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let seguro = if https { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{}={valor}; Path=/; HttpOnly; SameSite=Strict; Max-Age={duracao}{seguro}", auth::COOKIE))
        .unwrap()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ").map(str::trim)
}

/// O que uma rota exige, além de estar dentro.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Exigencia {
    /// Só estar dentro: a própria rota filtra o que mostra (listas) ou só mexe
    /// em coisas do próprio usuário (sessão, senha, foco).
    Dentro,
    /// Uma ação sobre o sistema todo: criar, gerir usuários, executar checagens.
    Sistema(&'static str),
    /// Uma ação sobre o item cujo id vem no parâmetro `{id}` do caminho.
    Item(Tipo, &'static str),
    /// O próprio handler confere, porque depende do corpo ou de o item existir.
    ConfereNoHandler,
}

/// A tabela de permissões das rotas. `rota` é o modelo registrado no axum, sem o
/// `/api`. Rota que não está aqui é negada: esquecer de pôr uma rota nova na
/// tabela dá 403, não acesso livre.
pub(crate) fn exigencia(metodo: &str, rota: &str) -> Option<Exigencia> {
    use Exigencia::*;
    use Tipo::*;
    let item = |tipo, acao| Some(Item(tipo, acao));
    let r = match (metodo, rota) {
        ("GET", "/sessao" | "/usuarios" | "/rubricas" | "/rubricas.txt" | "/turmas" | "/avaliacoes" | "/avaliacoes.txt" | "/pipelines.txt") => Dentro,
        ("PUT", "/senha" | "/foco") | ("GET", "/foco/eventos") => Dentro,
        ("POST", "/usuarios") | ("PATCH" | "DELETE", "/usuarios/{nome}") => Sistema("gerir_usuarios"),
        ("POST", "/rubricas" | "/turmas" | "/avaliacoes") => Sistema("criar"),
        ("GET", "/executor/fila" | "/executor/eventos") => Sistema("executar"),
        ("PUT", "/executor/relatorios/{id}/{aluno}" | "/executor/plagio/{id}" | "/executor/plagio/{id}/jplag") => Sistema("executar"),

        // Gravar uma rubrica pelo nome cria se ela não existe.
        ("PUT", "/rubricas/{id}") => ConfereNoHandler,
        ("GET", "/rubricas/{id}") => return item(Rubricas, "ver"),
        ("DELETE", "/rubricas/{id}") => return item(Rubricas, "apagar"),
        ("PUT", "/rubricas/{id}/arquivada") => return item(Rubricas, "arquivar"),
        ("GET" | "PUT" | "DELETE", "/rubricas/{id}/acessos" | "/rubricas/{id}/acessos/{usuario}") => return item(Rubricas, "compartilhar"),

        ("GET", "/turmas/{id}") => return item(Turmas, "ver"),
        ("PUT", "/turmas/{id}") => return item(Turmas, "editar"),
        ("DELETE", "/turmas/{id}") => return item(Turmas, "apagar"),
        ("PUT", "/turmas/{id}/arquivada") => return item(Turmas, "arquivar"),
        ("GET" | "PUT" | "DELETE", "/turmas/{id}/acessos" | "/turmas/{id}/acessos/{usuario}") => return item(Turmas, "compartilhar"),

        ("GET", "/avaliacoes/{id}" | "/avaliacoes/{id}/plagio" | "/avaliacoes/{id}/plagio.jplag") => return item(Avaliacoes, "ver"),
        ("GET", "/avaliacoes/{id}/notas.csv") | ("POST", "/avaliacoes/{id}/planilha-moodle") => return item(Avaliacoes, "exportar"),
        ("PATCH", "/avaliacoes/{id}/correcoes/{aluno}") => return item(Avaliacoes, "corrigir"),
        // Editar o código do aluno faz parte de corrigir. Ler submissões e pipeline é
        // o que o executor precisa (e quem vê a avaliação também pode).
        ("PUT" | "DELETE", "/avaliacoes/{id}/submissoes/{aluno}/{*caminho}") => return item(Avaliacoes, "corrigir"),
        (
            "GET",
            "/avaliacoes/{id}/submissoes"
            | "/avaliacoes/{id}/submissoes/{aluno}"
            | "/avaliacoes/{id}/submissoes/{aluno}/{*caminho}"
            | "/pipelines/{id}"
            | "/pipelines/{id}/{*caminho}",
        ) => return item(Avaliacoes, "ler_checagens"),
        ("PUT", "/avaliacoes/{id}") | ("POST", "/avaliacoes/{id}/submissoes" | "/avaliacoes/{id}/submissoes/sem-dono/{chave}") => {
            return item(Avaliacoes, "editar");
        }
        ("PUT" | "DELETE", "/pipelines/{id}/{*caminho}") => return item(Avaliacoes, "editar"),
        ("DELETE", "/avaliacoes/{id}") => return item(Avaliacoes, "apagar"),
        ("PUT", "/avaliacoes/{id}/arquivada") => return item(Avaliacoes, "arquivar"),
        ("GET" | "PUT" | "DELETE", "/avaliacoes/{id}/acessos" | "/avaliacoes/{id}/acessos/{usuario}") => return item(Avaliacoes, "compartilhar"),
        _ => return None,
    };
    Some(r)
}

/// `GET /rubricas/x.toml` fala da rubrica `x`.
fn id_do_item(id: &str) -> &str {
    id.strip_suffix(".toml").unwrap_or(id)
}

async fn exigir_sessao(State(e): State<Estado>, req: Request, next: Next) -> Response {
    let quem = match bearer(req.headers()) {
        Some(t) => e.auth.usuario_por_token(t),
        None => token(req.headers()).and_then(|t| e.auth.usuario(&t)),
    };
    let Some(quem) = quem else {
        return Erro(StatusCode::UNAUTHORIZED, "Entre para continuar".into()).into_response();
    };
    let (mut partes, corpo) = req.into_parts();
    let rota = partes.extensions.get::<MatchedPath>().map(|m| m.as_str().trim_start_matches("/api").to_string()).unwrap_or_default();
    let negado = || Erro(StatusCode::FORBIDDEN, "Você não tem permissão para isso.".into()).into_response();
    match exigencia(partes.method.as_str(), &rota) {
        None => {
            eprintln!("rota sem permissão definida: {} {rota}", partes.method);
            return negado();
        }
        Some(Exigencia::Dentro | Exigencia::ConfereNoHandler) => {}
        Some(Exigencia::Sistema(acao)) => {
            if !e.visao(&quem).pode(Recurso::Sistema, acao) {
                return negado();
            }
        }
        Some(Exigencia::Item(tipo, acao)) => {
            let params = RawPathParams::from_request_parts(&mut partes, &e).await;
            let id = params.ok().and_then(|p| p.iter().find(|(k, _)| *k == "id").map(|(_, v)| id_do_item(v).to_string()));
            let Some(id) = id else { return negado() };
            if let Err(erro) = e.exigir(&quem, tipo, &id, acao) {
                return erro.into_response();
            }
        }
    }
    let mut req = Request::from_parts(partes, corpo);
    req.extensions_mut().insert(quem);
    let mut r = next.run(req).await;
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

// ----- sessão -----

#[derive(Deserialize)]
struct Credenciais {
    usuario: String,
    senha: String,
}

async fn entrar(State(e): State<Estado>, headers: HeaderMap, Json(c): Json<Credenciais>) -> Response {
    let auth = e.auth.clone();
    let resultado = tokio::task::spawn_blocking(move || auth.entrar(&c.usuario, &c.senha)).await;
    match resultado {
        Ok(Ok((token, nome))) => {
            let mut r = Json(json!({ "usuario": nome })).into_response();
            r.headers_mut().insert(header::SET_COOKIE, cookie(&headers, &token, auth::DURACAO_SESSAO));
            r
        }
        Ok(Err(ErroLogin::Credenciais)) => Erro(StatusCode::UNAUTHORIZED, "Usuário ou senha incorretos".into()).into_response(),
        Ok(Err(ErroLogin::Bloqueado(s))) => Erro(
            StatusCode::TOO_MANY_REQUESTS,
            format!("Muitas tentativas. Tente de novo em {} min.", s.div_ceil(60)),
        )
        .into_response(),
        Err(_) => Erro(StatusCode::INTERNAL_SERVER_ERROR, "Falha ao conferir a senha".into()).into_response(),
    }
}

async fn sair(State(e): State<Estado>, headers: HeaderMap) -> Response {
    if let Some(t) = token(&headers) {
        e.auth.sair(&t);
    }
    let mut r = Json(json!({})).into_response();
    r.headers_mut().insert(header::SET_COOKIE, cookie(&headers, "", 0));
    r
}

/// Quem está dentro, o que pode no sistema e os papéis que a política conhece
/// (a tela de compartilhar e a de usuários usam).
async fn sessao(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Json<Value> {
    let acoes = e.visao(&u).acoes(Recurso::Sistema);
    Json(json!({
        "usuario": u.nome,
        "papel": u.papel,
        "acoes": acoes,
        "papeis": e.permissoes.papeis(),
        "papeis_globais": e.permissoes.papeis_globais(),
    }))
}

// ----- arquivadas -----

#[derive(Deserialize)]
struct Arquivar {
    arquivada: bool,
}

async fn arquivar(tipo: Tipo, State(e): State<Estado>, Path(id): Path<String>, Json(a): Json<Arquivar>) -> Res<Json<Value>> {
    e.dados.arquivar(tipo, &id, a.arquivada)?;
    if tipo == Tipo::Avaliacoes {
        e.avisos.fila_mudou();
    }
    Ok(Json(json!({ "arquivada": a.arquivada })))
}

/// O item (que tem `id`) como JSON, com `arquivada` e as `acoes` que quem pede
/// pode fazer nele ao lado dos campos dele.
fn com_estado(item: impl serde::Serialize, tipo: Tipo, arquivadas: &Arquivadas, visao: &Visao) -> Value {
    let mut v = json!(item);
    let id = v["id"].as_str().unwrap_or_default().to_string();
    v["arquivada"] = json!(arquivadas.contem(tipo, &id));
    v["acoes"] = json!(visao.acoes(Recurso::Item(tipo, &id)));
    v
}

fn visivel(visao: &Visao, tipo: Tipo, id: &str) -> bool {
    visao.pode(Recurso::Item(tipo, id), "ver")
}

/// O fim da linha das listas em texto.
pub(crate) fn marca_arquivada(arquivada: bool) -> &'static str {
    if arquivada { " | arquivada" } else { "" }
}

// ----- rubricas -----

async fn listar_rubricas(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Json<Value>> {
    let (rubricas, problemas) = e.dados.listar_rubricas()?;
    let arquivadas = e.dados.arquivadas();
    let visao = e.visao(&u);
    let rubricas: Vec<Value> = rubricas
        .into_iter()
        .filter(|r| visivel(&visao, Tipo::Rubricas, &r.id))
        .map(|r| com_estado(r, Tipo::Rubricas, &arquivadas, &visao))
        .collect();
    // Os problemas citam arquivos pelo nome: só o admin os vê todos.
    let problemas = if visao.pode(Recurso::Sistema, "ver_tudo") { problemas } else { Vec::new() };
    Ok(Json(json!({ "rubricas": rubricas, "problemas": problemas })))
}

/// Para quem gasta tokens lendo (o Claude): uma linha por rubrica.
async fn listar_rubricas_texto(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Response> {
    let (rubricas, problemas) = e.dados.listar_rubricas()?;
    let arquivadas = e.dados.arquivadas();
    let visao = e.visao(&u);
    let problemas = if visao.pode(Recurso::Sistema, "ver_tudo") { problemas } else { Vec::new() };
    let mut texto = String::new();
    for r in rubricas.iter().filter(|r| visivel(&visao, Tipo::Rubricas, &r.id)) {
        let (criterios, passos) = r.contagem();
        let arquivada = marca_arquivada(arquivadas.contem(Tipo::Rubricas, &r.id));
        texto.push_str(&format!("{} | {} | {criterios} critérios | {passos} passos{arquivada}\n", r.id, r.titulo));
    }
    for p in problemas {
        texto.push_str(&format!("! {p}\n"));
    }
    Ok(texto_puro(texto))
}

pub(crate) fn texto_puro(texto: String) -> Response {
    let mut r = texto.into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    r
}

/// `GET /rubricas/{id}` dá JSON; `GET /rubricas/{id}.toml`, o TOML enxuto.
async fn ler_rubrica(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>) -> Res<Response> {
    Ok(match id.strip_suffix(".toml") {
        Some(id) => texto_puro(e.dados.rubrica_toml(id)?),
        None => Json(com_estado(e.dados.ler_rubrica(&id)?, Tipo::Rubricas, &e.dados.arquivadas(), &e.visao(&u))).into_response(),
    })
}

async fn criar_rubrica(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(r): Json<Rubrica>) -> Res<Json<Rubrica>> {
    let r = e.dados.salvar_rubrica(None, r)?;
    e.tornar_dono(&u, Tipo::Rubricas, &r.id)?;
    Ok(Json(r))
}

/// Gravar sobre uma rubrica que existe pede `editar` nela; gravar uma nova pede
/// poder criar, e quem cria vira dono.
fn exigir_gravar_rubrica(e: &Estado, u: &Quem, id: &str) -> Res<bool> {
    match e.dados.ler_rubrica(id) {
        Err(dados::Erro::NaoEncontrado(_)) => {
            if e.visao(u).pode(Recurso::Sistema, "criar") {
                Ok(true)
            } else {
                Err(Erro(StatusCode::FORBIDDEN, "Seu usuário não pode criar rubricas.".into()))
            }
        }
        _ => e.exigir(u, Tipo::Rubricas, id, "editar").map(|_| false),
    }
}

/// `PUT /rubricas/{id}` recebe JSON (editor web); `PUT /rubricas/{id}.toml`, o
/// arquivo inteiro em TOML, e responde com uma linha só.
async fn salvar_rubrica(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>, corpo: String) -> Res<Response> {
    let nova = exigir_gravar_rubrica(&e, &u, id_do_item(&id))?;
    if let Some(id) = id.strip_suffix(".toml") {
        let r = e.dados.salvar_rubrica_toml(id, &corpo)?;
        if nova {
            e.tornar_dono(&u, Tipo::Rubricas, &r.id)?;
        }
        let (criterios, passos) = r.contagem();
        return Ok(texto_puro(format!("Gravada: {} ({criterios} critérios, {passos} passos)\n", r.id)));
    }
    let r: Rubrica =
        serde_json::from_str(&corpo).map_err(|err| Erro(StatusCode::UNPROCESSABLE_ENTITY, format!("Pedido inválido: {err}")))?;
    let r = e.dados.salvar_rubrica(Some(&id), r)?;
    if nova {
        e.tornar_dono(&u, Tipo::Rubricas, &r.id)?;
    }
    Ok(Json(r).into_response())
}

async fn apagar_rubrica(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.dados.apagar_rubrica(&id)?;
    e.esquecer(Tipo::Rubricas, &id);
    Ok(Json(json!({})))
}

// ----- turmas -----

#[derive(Deserialize)]
struct NovaTurma {
    nome: String,
    csv: String,
}

#[derive(Deserialize)]
struct ListaAlunos {
    csv: String,
}

async fn listar_turmas(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Json<Vec<Value>>> {
    let arquivadas = e.dados.arquivadas();
    let visao = e.visao(&u);
    Ok(Json(
        e.dados
            .listar_turmas()?
            .into_iter()
            .filter(|t| visivel(&visao, Tipo::Turmas, &t.id))
            .map(|t| com_estado(t, Tipo::Turmas, &arquivadas, &visao))
            .collect(),
    ))
}

async fn ler_turma(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>) -> Res<Json<Value>> {
    Ok(Json(com_estado(e.dados.ler_turma(&id)?, Tipo::Turmas, &e.dados.arquivadas(), &e.visao(&u))))
}

async fn criar_turma(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(t): Json<NovaTurma>) -> Res<Json<Turma>> {
    let t = e.dados.salvar_turma(t.nome.trim(), &t.csv, true)?;
    e.tornar_dono(&u, Tipo::Turmas, &t.id)?;
    Ok(Json(t))
}

async fn salvar_turma(State(e): State<Estado>, Path(id): Path<String>, Json(t): Json<ListaAlunos>) -> Res<Json<Turma>> {
    e.dados.ler_turma(&id).map(|_| ()).or_else(|err| match err {
        dados::Erro::Invalido(_) => Ok(()),
        outro => Err(outro),
    })?;
    Ok(Json(e.dados.salvar_turma(&id, &t.csv, false)?))
}

async fn apagar_turma(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.dados.apagar_turma(&id)?;
    e.esquecer(Tipo::Turmas, &id);
    Ok(Json(json!({})))
}

// ----- avaliações -----

#[derive(Deserialize)]
struct RubricaEscolhida {
    origem: String,
    #[serde(default = "um")]
    peso: f64,
}

fn um() -> f64 {
    1.0
}

#[derive(Deserialize)]
struct DadosAvaliacao {
    titulo: String,
    turma: String,
    #[serde(default = "dez")]
    nota_maxima: f64,
    rubricas: Vec<RubricaEscolhida>,
    /// Origens cuja cópia deve ser refeita a partir da pasta.
    #[serde(default)]
    atualizar: Vec<String>,
}

fn dez() -> f64 {
    10.0
}

async fn listar_avaliacoes(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Json<Value>> {
    let mut lista = Vec::new();
    let arquivadas = e.dados.arquivadas();
    let visao = e.visao(&u);
    for a in e.dados.listar_avaliacoes()? {
        if !visivel(&visao, Tipo::Avaliacoes, &a.id) {
            continue;
        }
        let alunos = e.dados.ler_turma(&a.turma).map(|t| t.alunos).unwrap_or_default();
        let correcoes = e.dados.ler_correcoes(&a.id).unwrap_or_default();
        let (mut concluidos, mut iniciados) = (0, 0);
        for aluno in &alunos {
            let r = calcular(&a, correcoes.get(&aluno.id));
            if r.concluida() {
                concluidos += 1;
            } else if r.respondidos > 0 {
                iniciados += 1;
            }
        }
        lista.push(json!({
            "id": a.id,
            "titulo": a.titulo,
            "turma": a.turma,
            "nota_maxima": a.nota_maxima,
            "rubricas": a.rubricas.iter().map(|r| json!({ "origem": r.origem, "titulo": r.rubrica.titulo, "peso": r.peso })).collect::<Vec<_>>(),
            "criterios": a.total_criterios(),
            "alunos": alunos.len(),
            "concluidos": concluidos,
            "iniciados": iniciados,
            "arquivada": arquivadas.contem(Tipo::Avaliacoes, &a.id),
            "acoes": visao.acoes(Recurso::Item(Tipo::Avaliacoes, &a.id)),
        }));
    }
    Ok(Json(json!(lista)))
}

/// Tudo que a tela de avaliação precisa, de uma vez, para funcionar sem rede depois.
async fn pacote(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>) -> Res<Json<Value>> {
    let avaliacao = e.dados.ler_avaliacao(&id)?;
    let turma = e.dados.ler_turma(&avaliacao.turma)?;
    let correcoes = e.dados.ler_correcoes(&id)?;
    let desatualizadas = e.dados.rubricas_desatualizadas(&avaliacao);
    let ead = ead::resumo(&e, &id)?;
    let arquivada = e.dados.arquivadas().contem(Tipo::Avaliacoes, &id);
    let acoes = e.visao(&u).acoes(Recurso::Item(Tipo::Avaliacoes, &id));
    Ok(Json(json!({ "avaliacao": avaliacao, "turma": turma, "correcoes": correcoes, "desatualizadas": desatualizadas, "ead": ead, "arquivada": arquivada, "acoes": acoes })))
}

/// Pôr uma rubrica ou turma numa avaliação copia o conteúdo delas: pede poder
/// vê-las. As que já estavam na avaliação continuam valendo sem isso, para quem
/// edita uma avaliação compartilhada não precisar de acesso à rubrica original.
fn exigir_usar(e: &Estado, u: &Quem, id: Option<&str>, d: &DadosAvaliacao) -> Res<()> {
    let atual = match id {
        Some(id) => Some(e.dados.ler_avaliacao(id)?),
        None => None,
    };
    let ja_tinha = |origem: &str| atual.as_ref().is_some_and(|a| a.rubricas.iter().any(|r| r.origem == origem));
    for r in &d.rubricas {
        if !ja_tinha(&r.origem) || d.atualizar.contains(&r.origem) {
            e.exigir(u, Tipo::Rubricas, &r.origem, "ver")?;
        }
    }
    if atual.is_none_or(|a| a.turma != d.turma) {
        e.exigir(u, Tipo::Turmas, &d.turma, "ver")?;
    }
    Ok(())
}

fn gravar_avaliacao(e: &Estado, id: Option<&str>, d: DadosAvaliacao) -> Res<Json<Avaliacao>> {
    let rubricas = d.rubricas.into_iter().map(|r| (r.origem, r.peso)).collect();
    Ok(Json(e.dados.salvar_avaliacao(id, d.titulo.trim().to_string(), d.turma, d.nota_maxima, rubricas, &d.atualizar)?))
}

async fn criar_avaliacao(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(d): Json<DadosAvaliacao>) -> Res<Json<Avaliacao>> {
    exigir_usar(&e, &u, None, &d)?;
    let a = gravar_avaliacao(&e, None, d)?;
    e.tornar_dono(&u, Tipo::Avaliacoes, &a.id)?;
    Ok(a)
}

async fn salvar_avaliacao(
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path(id): Path<String>,
    Json(d): Json<DadosAvaliacao>,
) -> Res<Json<Avaliacao>> {
    exigir_usar(&e, &u, Some(&id), &d)?;
    // Rubricas atualizadas podem mudar as sugestões do pipeline.
    e.avisos.fila_mudou();
    gravar_avaliacao(&e, Some(&id), d)
}

async fn apagar_avaliacao(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.dados.apagar_avaliacao(&id)?;
    e.esquecer(Tipo::Avaliacoes, &id);
    Ok(Json(json!({})))
}

// ----- correções -----

#[derive(Deserialize)]
struct AlteracaoResposta {
    rubrica: String,
    criterio: String,
    /// `null` apaga a marcação do critério.
    resposta: Option<Resposta>,
}

#[derive(Deserialize)]
struct Alteracoes {
    #[serde(default)]
    respostas: Vec<AlteracaoResposta>,
    #[serde(default)]
    comentario_geral: Option<String>,
}

async fn alterar_correcao(
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path((id, aluno)): Path<(String, String)>,
    Json(a): Json<Alteracoes>,
) -> Res<Json<Value>> {
    let respostas = a.respostas.into_iter().map(|r| (r.rubrica, r.criterio, r.resposta)).collect();
    let (correcao, recusadas) = e.dados.alterar_correcao(&id, &aluno, respostas, a.comentario_geral, &u.nome, auth::agora())?;
    Ok(Json(json!({ "correcao": correcao, "recusadas": recusadas })))
}

// ----- exportação -----

#[derive(Deserialize)]
struct OpcoesCsv {
    /// `br=1`: ponto e vírgula como separador e vírgula decimal, como o Excel em português espera.
    #[serde(default)]
    br: Option<String>,
}

pub(crate) fn numero(v: f64, br: bool) -> String {
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

pub fn notas_csv(av: &Avaliacao, turma: &Turma, correcoes: &std::collections::BTreeMap<String, Correcao>, br: bool) -> String {
    let mut w = csv::WriterBuilder::new().delimiter(if br { b';' } else { b',' }).from_writer(Vec::new());
    let varias = av.rubricas.len() > 1;
    let mut cab = vec!["matricula".to_string(), "nome".to_string()];
    for ra in &av.rubricas {
        for c in ra.rubrica.criterios.iter().filter(|c| !c.roteiro()) {
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
            for c in ra.rubrica.criterios.iter().filter(|c| !c.roteiro()) {
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
        linha.push(
            if r.concluida() {
                "concluída".to_string()
            } else if r.respondidos > 0 {
                format!("{} de {}", r.respondidos, r.total)
            } else {
                "não avaliado".to_string()
            },
        );
        linha.push(texto_csv(&comentarios.join(" | ")));
        w.write_record(&linha).unwrap();
    }
    String::from_utf8(w.into_inner().unwrap()).unwrap()
}

async fn exportar(State(e): State<Estado>, Path(id): Path<String>, Query(o): Query<OpcoesCsv>) -> Res<Response> {
    let av = e.dados.ler_avaliacao(&id)?;
    let turma = e.dados.ler_turma(&av.turma)?;
    let correcoes = e.dados.ler_correcoes(&id)?;
    let br = o.br.is_some_and(|v| v != "0");
    // O BOM faz o Excel reconhecer UTF-8.
    let corpo = format!("\u{feff}{}", notas_csv(&av, &turma, &correcoes, br));
    let nome: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let mut r = corpo.into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/csv; charset=utf-8"));
    r.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"notas-{nome}{}.csv\"", if br { "-excel" } else { "" })).unwrap(),
    );
    Ok(r)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tabela_de_permissoes_das_rotas() {
        use Exigencia::*;
        assert_eq!(exigencia("GET", "/sessao"), Some(Dentro));
        assert_eq!(exigencia("POST", "/avaliacoes"), Some(Sistema("criar")));
        assert_eq!(exigencia("PATCH", "/avaliacoes/{id}/correcoes/{aluno}"), Some(Item(Tipo::Avaliacoes, "corrigir")));
        assert_eq!(exigencia("GET", "/avaliacoes/{id}/notas.csv"), Some(Item(Tipo::Avaliacoes, "exportar")));
        assert_eq!(exigencia("PUT", "/avaliacoes/{id}/submissoes/{aluno}/{*caminho}"), Some(Item(Tipo::Avaliacoes, "corrigir")));
        assert_eq!(exigencia("GET", "/pipelines/{id}/{*caminho}"), Some(Item(Tipo::Avaliacoes, "ler_checagens")));
        assert_eq!(exigencia("PUT", "/pipelines/{id}/{*caminho}"), Some(Item(Tipo::Avaliacoes, "editar")));
        assert_eq!(exigencia("DELETE", "/turmas/{id}"), Some(Item(Tipo::Turmas, "apagar")));
        assert_eq!(exigencia("PUT", "/rubricas/{id}/acessos/{usuario}"), Some(Item(Tipo::Rubricas, "compartilhar")));
        assert_eq!(exigencia("PUT", "/executor/relatorios/{id}/{aluno}"), Some(Sistema("executar")));
        assert_eq!(exigencia("DELETE", "/usuarios/{nome}"), Some(Sistema("gerir_usuarios")));
        // fora da tabela: negado
        assert_eq!(exigencia("DELETE", "/sessao"), None);
        assert_eq!(exigencia("GET", "/rota-nova"), None);
        assert_eq!(id_do_item("laços.toml"), "laços");
    }

    /// Um pedido ao servidor de teste; devolve o status e o corpo como JSON (ou texto).
    fn pedir(base: &str, token: &str, metodo: &str, caminho: &str, corpo: Option<&str>) -> (u16, Value) {
        let agente: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        let url = format!("{base}/api{caminho}");
        let auth = format!("Bearer {token}");
        let corpo = corpo.unwrap_or("");
        let tipo = if corpo.starts_with(['{', '[']) { "application/json" } else { "text/plain" };
        let r = match metodo {
            "GET" => agente.get(&url).header("Authorization", &auth).call(),
            "DELETE" => agente.delete(&url).header("Authorization", &auth).call(),
            "POST" => agente.post(&url).header("Authorization", &auth).header("content-type", tipo).send(corpo),
            "PUT" => agente.put(&url).header("Authorization", &auth).header("content-type", tipo).send(corpo),
            "PATCH" => agente.patch(&url).header("Authorization", &auth).header("content-type", tipo).send(corpo),
            _ => unreachable!(),
        };
        let mut r = r.unwrap();
        let status = r.status().as_u16();
        let texto = r.body_mut().read_to_string().unwrap_or_default();
        (status, serde_json::from_str(&texto).unwrap_or(Value::String(texto)))
    }

    /// Professor cria e compartilha; o monitor só vê e corrige o que recebeu.
    #[tokio::test(flavor = "multi_thread")]
    async fn compartilhar_uma_avaliacao_com_um_monitor() {
        let raiz = std::env::temp_dir().join(format!("rubricas-api-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        let dados = Dados::abrir(&raiz).unwrap();
        for (nome, papel) in [("vini", "admin"), ("bia", "professor"), ("ana", "monitor"), ("maquina", "executor")] {
            auth::definir_senha(&raiz, nome, "senha-longa", papel).unwrap();
        }
        let [vini, bia, ana, maquina] = ["vini", "bia", "ana", "maquina"].map(|n| auth::criar_token(&raiz, n).unwrap());
        let estado = Estado {
            auth: Arc::new(Auth::abrir(&raiz)),
            permissoes: Arc::new(Permissoes::abrir(&raiz).unwrap()),
            dados: Arc::new(dados),
            web: Arc::new(Web::novo(None)),
            avisos: Arc::new(ead::Avisos::novo()),
        };
        let escuta = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", escuta.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(escuta, rotas(estado)).await });

        tokio::task::spawn_blocking(move || {
            let p = |token: &str, m: &str, c: &str, corpo: Option<&str>| pedir(&base, token, m, c, corpo);

            // a professora cria turma, rubrica e avaliação: vira dona das três
            assert_eq!(p(&bia, "POST", "/turmas", Some(r#"{"nome":"T1","csv":"matricula,nome\n1,Ana Aluna\n"}"#)).0, 200);
            let rubrica = "titulo = \"Prática\"\n[[criterios]]\nnome = \"Funciona\"\nescala = { tipo = \"binaria\", pontos = 1.0 }\n";
            assert_eq!(p(&bia, "PUT", "/rubricas/pratica.toml", Some(rubrica)).0, 200);
            let (st, av) = p(&bia, "POST", "/avaliacoes", Some(r#"{"titulo":"P1","turma":"T1","rubricas":[{"origem":"pratica"}]}"#));
            assert_eq!(st, 200, "{av}");
            assert_eq!(av["id"], "p1");

            // o monitor não vê nada, nem pode criar
            assert_eq!(p(&ana, "GET", "/avaliacoes", None).1, json!([]));
            let (st, erro) = p(&ana, "GET", "/avaliacoes/p1", None);
            assert_eq!(st, 403);
            assert!(erro["erro"].as_str().unwrap().contains("Peça a quem a criou"), "{erro}");
            assert_eq!(p(&ana, "POST", "/turmas", Some(r#"{"nome":"T2","csv":"matricula,nome\n"}"#)).0, 403);
            assert_eq!(p(&ana, "PUT", "/rubricas/outra.toml", Some(rubrica)).0, 403);
            assert_eq!(p(&ana, "GET", "/usuarios", None).0, 200);
            assert_eq!(p(&ana, "POST", "/usuarios", Some(r#"{"nome":"eu","senha":"senha-longa","papel":"admin"}"#)).0, 403);

            // compartilhada como avaliador: vê e corrige, mas não altera nem apaga
            assert_eq!(p(&ana, "PUT", "/avaliacoes/p1/acessos/bia", Some(r#"{"papel":"leitor"}"#)).0, 403);
            let (st, r) = p(&bia, "PUT", "/avaliacoes/p1/acessos/ana", Some(r#"{"papel":"avaliador"}"#));
            assert_eq!(st, 200, "{r}");
            let (_, lista) = p(&ana, "GET", "/avaliacoes", None);
            assert_eq!(lista[0]["id"], "p1");
            assert_eq!(lista[0]["acoes"], json!(["corrigir", "exportar", "ler_checagens", "ver"]));
            assert_eq!(p(&ana, "GET", "/avaliacoes/p1", None).0, 200);
            let marca = r#"{"respostas":[{"rubrica":"pratica","criterio":"funciona","resposta":{"tipo":"binaria","atende":true}}]}"#;
            let (st, r) = p(&ana, "PATCH", "/avaliacoes/p1/correcoes/1", Some(marca));
            assert_eq!(st, 200, "{r}");
            assert_eq!(r["correcao"]["avaliador"], "ana", "{r}");
            assert_eq!(p(&ana, "DELETE", "/avaliacoes/p1", None).0, 403);
            assert_eq!(p(&ana, "PUT", "/avaliacoes/p1", Some(r#"{"titulo":"X","turma":"T1","rubricas":[{"origem":"pratica"}]}"#)).0, 403);
            // a turma e a rubrica não vieram junto
            assert_eq!(p(&ana, "GET", "/turmas", None).1, json!([]));
            assert_eq!(p(&ana, "GET", "/rubricas/pratica", None).0, 403);

            // o executor baixa o que as checagens pedem, mas não vê notas nem corrige
            assert_eq!(p(&maquina, "GET", "/executor/fila", None).0, 200);
            assert_eq!(p(&maquina, "GET", "/avaliacoes/p1/submissoes", None).0, 200);
            assert_eq!(p(&maquina, "GET", "/avaliacoes/p1/submissoes/1", None).0, 200);
            assert_eq!(p(&maquina, "GET", "/avaliacoes/p1", None).0, 403);
            assert_eq!(p(&maquina, "GET", "/avaliacoes/p1/notas.csv", None).0, 403);
            assert_eq!(p(&maquina, "PATCH", "/avaliacoes/p1/correcoes/1", Some(marca)).0, 403);
            assert_eq!(p(&ana, "GET", "/executor/fila", None).0, 403);

            // a dona não pode deixar a avaliação sem dono
            assert_eq!(p(&bia, "DELETE", "/avaliacoes/p1/acessos/bia", None).0, 409);
            // o admin vê tudo, mesmo sem acesso concedido
            assert_eq!(p(&vini, "GET", "/turmas", None).1.as_array().unwrap().len(), 1);

            // revogado, volta a não ver
            assert_eq!(p(&bia, "DELETE", "/avaliacoes/p1/acessos/ana", None).0, 200);
            assert_eq!(p(&ana, "GET", "/avaliacoes/p1", None).0, 403);
            // apagar a avaliação leva os acessos junto
            assert_eq!(p(&bia, "DELETE", "/avaliacoes/p1", None).0, 200);
            assert_eq!(p(&vini, "GET", "/avaliacoes/p1/acessos", None).0, 404);
        })
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Toda rota registrada no código está na tabela: esquecer uma daria 403 até
    /// para o admin. Varre os `.route("…", get(…).put(…))` dos módulos de rotas.
    #[test]
    fn toda_rota_registrada_tem_permissao_definida() {
        let fontes = [include_str!("api.rs"), include_str!("ead.rs"), include_str!("contas.rs")];
        let mut rotas = Vec::new();
        for fonte in fontes {
            for (i, _) in fonte.match_indices(".route(") {
                let resto = &fonte[i + 7..];
                let fim = [resto.find("\n        ."), resto.find(";\n")].into_iter().flatten().min().unwrap_or(resto.len());
                let trecho = &resto[..fim];
                let Some(caminho) = trecho.trim_start().strip_prefix('"').and_then(|t| t.split('"').next()).filter(|c| c.starts_with('/')) else {
                    continue;
                };
                for (m, metodo) in [("get(", "GET"), ("post(", "POST"), ("put(", "PUT"), ("patch(", "PATCH"), ("delete(", "DELETE")] {
                    if trecho.contains(&format!(" {m}")) || trecho.contains(&format!("({m}")) || trecho.contains(&format!(".{m}")) {
                        rotas.push((metodo, caminho.to_string()));
                    }
                }
            }
        }
        // As de `contas.rs` montadas com format!, uma por tipo.
        for pasta in ["avaliacoes", "rubricas", "turmas"] {
            rotas.push(("GET", format!("/{pasta}/{{id}}/acessos")));
            rotas.push(("PUT", format!("/{pasta}/{{id}}/acessos/{{usuario}}")));
            rotas.push(("DELETE", format!("/{pasta}/{{id}}/acessos/{{usuario}}")));
        }
        assert!(rotas.len() > 40, "a varredura achou poucas rotas: {rotas:?}");
        let faltando: Vec<_> = rotas.iter().filter(|(m, r)| r != "/entrar" && r != "/sair" && exigencia(m, r).is_none()).collect();
        assert!(faltando.is_empty(), "rotas sem permissão definida: {faltando:?}");
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
