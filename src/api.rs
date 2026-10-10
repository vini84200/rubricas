//! Rotas HTTP. Tudo sob `/api` exige sessão (cookie) ou token de API
//! (`Authorization: Bearer …`), menos `/api/entrar` e `/api/sair`. Cada rota
//! protegida é registrada com o que é preciso poder para usá-la (`Metodos`).

use crate::auth::{self, Auth, ErroLogin};
use crate::dados::{self, Arquivadas, Dados, Tipo};
use crate::ead;
use crate::estatico::Web;
use crate::modelo::*;
use crate::permissoes::{Acao, Permissoes, Quem, Recurso, Visao};
use crate::contas;
use crate::notas::notas_csv;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, RawPathParams, Request, State};
use axum::handler::Handler;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, MethodRouter, on, post};
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
    pub(crate) fn exigir(&self, quem: &Quem, tipo: Tipo, id: &str, acao: Acao) -> Res<()> {
        let v = self.visao(quem);
        if v.pode(Recurso::Item(tipo, id), acao) { Ok(()) } else { Err(sem_permissao(&v, tipo, id, acao)) }
    }

    /// Cria um item e dá a quem criou o maior papel nele. Se o papel não puder
    /// ser gravado, o item é desfeito: nada fica no disco sem dono.
    fn criar<T>(&self, tipo: Tipo, quem: &Quem, id: impl Fn(&T) -> &str, criar: impl FnOnce(&Dados) -> dados::Res<T>) -> Res<T> {
        let item = criar(&self.dados)?;
        if let Err(e) = self.permissoes.nascer(tipo, id(&item), &quem.nome, auth::agora()) {
            if let Err(desfazer) = self.dados.apagar(tipo, id(&item)) {
                eprintln!("{}/{} ficou sem dono: {desfazer}", tipo.pasta(), id(&item));
            }
            return Err(e.into());
        }
        Ok(item)
    }

    /// Apaga o item e os acessos a ele. Se os acessos não puderem ser gravados,
    /// `nascer` os limpa quando outro item tomar o mesmo id.
    fn apagar(&self, tipo: Tipo, id: &str) -> Res<Json<Value>> {
        self.dados.apagar(tipo, id)?;
        if let Err(e) = self.permissoes.esquecer_item(tipo, id) {
            eprintln!("acessos de {}/{id} ficaram em acessos.toml: {e}", tipo.pasta());
        }
        Ok(Json(json!({})))
    }
}

fn sem_permissao(v: &Visao, tipo: Tipo, id: &str, acao: Acao) -> Erro {
    let item = tipo.este();
    let m = if !v.pode(Recurso::Item(tipo, id), Acao::Ver) {
        format!("Você não tem acesso a {item}. Peça a quem a criou para compartilhar com você.")
    } else {
        let acao = match acao {
            Acao::Exportar => "baixar as notas",
            Acao::Editar => "alterar",
            Acao::LerChecagens => "ler as checagens",
            outra => outra.nome(),
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

/// Trabalho pesado de disco (ZIP, hashes, Argon2) fora do executor assíncrono.
pub(crate) async fn bloqueante<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tokio::task::spawn_blocking(f).await.map_err(|_| Erro(StatusCode::INTERNAL_SERVER_ERROR, "Falha interna".into()))?
}

// ----- rotas e o que cada uma exige -----

/// O que um método de rota exige, além de estar dentro.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Exigencia {
    /// Só estar dentro: a própria rota filtra o que mostra (listas) ou só mexe
    /// em coisas do próprio usuário (sessão, senha, foco).
    Dentro,
    /// Uma ação sobre o sistema todo: criar, gerir usuários, executar checagens.
    Sistema(Acao),
    /// Uma ação sobre o item cujo id vem no parâmetro `{id}` do caminho.
    Item(Tipo, Acao),
    /// O próprio handler confere, porque depende do corpo ou de o item existir.
    ConfereNoHandler,
}

/// As rotas protegidas. O `Router` fica aqui dentro: só se registra uma rota
/// por `rota`, com `Metodos`, e cada método diz o que exige.
pub(crate) struct Rotas(Router<Estado>);

impl Rotas {
    pub(crate) fn new() -> Self {
        Rotas(Router::new())
    }

    pub(crate) fn rota(self, caminho: &str, m: Metodos) -> Self {
        Rotas(self.0.route(caminho, m.0))
    }

    fn juntar(self, outras: Rotas) -> Self {
        Rotas(self.0.merge(outras.0))
    }
}

/// Os métodos de uma rota, cada um com o seu handler e a sua exigência.
pub(crate) struct Metodos(MethodRouter<Estado>);

macro_rules! metodo {
    ($nome:ident, $filtro:ident) => {
        pub(crate) fn $nome<H: Handler<T, Estado>, T: 'static>(self, exigencia: Exigencia, h: H) -> Self {
            self.com(MethodFilter::$filtro, exigencia, h)
        }
    };
}

impl Metodos {
    metodo!(get, GET);
    metodo!(post, POST);
    metodo!(put, PUT);
    metodo!(patch, PATCH);
    metodo!(delete, DELETE);

    fn com<H: Handler<T, Estado>, T: 'static>(self, filtro: MethodFilter, exigencia: Exigencia, h: H) -> Self {
        let conferir = middleware::from_fn(move |req: Request, next: Next| exigir(exigencia, req, next));
        Metodos(self.0.merge(on(filtro, h).route_layer(conferir)))
    }

    /// Corpo maior que o padrão do axum (2 MB), para ZIPs e anexos.
    pub(crate) fn limite(self, bytes: usize) -> Self {
        Metodos(self.0.layer(DefaultBodyLimit::max(bytes)))
    }
}

pub(crate) fn metodos() -> Metodos {
    Metodos(MethodRouter::new())
}

pub fn rotas(estado: Estado) -> Router {
    use Acao::*;
    use Exigencia::*;
    use Tipo::*;
    let protegidas = Rotas::new()
        .rota("/sessao", metodos().get(Dentro, sessao))
        .rota("/rubricas", metodos().get(Dentro, listar_rubricas).post(Sistema(Criar), criar_rubrica))
        .rota("/rubricas.txt", metodos().get(Dentro, listar_rubricas_texto))
        .rota(
            "/rubricas/{id}",
            // Gravar uma rubrica pelo nome cria se ela não existe.
            metodos().get(Item(Rubricas, Ver), ler_rubrica).put(ConfereNoHandler, salvar_rubrica).delete(Item(Rubricas, Apagar), apagar_rubrica),
        )
        .rota("/turmas", metodos().get(Dentro, listar_turmas).post(Sistema(Criar), criar_turma))
        .rota(
            "/turmas/{id}",
            metodos().get(Item(Turmas, Ver), ler_turma).put(Item(Turmas, Editar), salvar_turma).delete(Item(Turmas, Apagar), apagar_turma),
        )
        .rota("/avaliacoes", metodos().get(Dentro, listar_avaliacoes).post(Sistema(Criar), criar_avaliacao))
        .rota(
            "/avaliacoes/{id}",
            metodos()
                .get(Item(Avaliacoes, Ver), pacote)
                .put(Item(Avaliacoes, Editar), salvar_avaliacao)
                .delete(Item(Avaliacoes, Apagar), apagar_avaliacao),
        )
        .rota("/avaliacoes/{id}/correcoes/{aluno}", metodos().patch(Item(Avaliacoes, Corrigir), alterar_correcao))
        .rota("/avaliacoes/{id}/notas.csv", metodos().get(Item(Avaliacoes, Exportar), exportar))
        .rota("/rubricas/{id}/arquivada", metodos().put(Item(Rubricas, Arquivar), |e, p, j| arquivar(Rubricas, e, p, j)))
        .rota("/turmas/{id}/arquivada", metodos().put(Item(Turmas, Arquivar), |e, p, j| arquivar(Turmas, e, p, j)))
        .rota("/avaliacoes/{id}/arquivada", metodos().put(Item(Avaliacoes, Arquivar), |e, p, j| arquivar(Avaliacoes, e, p, j)))
        .juntar(ead::rotas())
        .juntar(contas::rotas());
    let api = Router::new()
        .route("/entrar", post(entrar))
        .route("/sair", post(sair))
        .merge(protegidas.0.route_layer(middleware::from_fn_with_state(estado.clone(), autenticar)))
        .fallback(|| async { Erro(StatusCode::NOT_FOUND, "Rota desconhecida".into()) })
        .layer(middleware::from_fn(erros_em_json));
    Router::new().nest("/api", api).fallback(estatico).layer(middleware::from_fn(cabecalhos)).with_state(estado)
}

async fn estatico(State(e): State<Estado>, uri: Uri, headers: HeaderMap) -> Response {
    e.web.servir(uri.path(), &headers)
}

/// O editor de código precisa de duas folgas: WebAssembly (o Ruff) e CSS inline
/// (o CodeMirror injeta o seu). Scripts continuam só do próprio servidor.
async fn cabecalhos(req: Request, next: Next) -> Response {
    let mut r = next.run(req).await;
    let h = r.headers_mut();
    h.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
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

/// `GET /rubricas/x.toml` fala da rubrica `x`.
fn id_do_item(id: &str) -> &str {
    id.strip_suffix(".toml").unwrap_or(id)
}

/// Quem pede, para os handlers e para `exigir`, que roda depois.
async fn autenticar(State(e): State<Estado>, mut req: Request, next: Next) -> Response {
    let quem = match bearer(req.headers()) {
        Some(t) => e.auth.usuario_por_token(t),
        None => token(req.headers()).and_then(|t| e.auth.usuario(&t)),
    };
    let Some(quem) = quem else {
        return Erro(StatusCode::UNAUTHORIZED, "Entre para continuar".into()).into_response();
    };
    req.extensions_mut().insert(quem);
    req.extensions_mut().insert(e);
    let mut r = next.run(req).await;
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

async fn exigir(exigencia: Exigencia, req: Request, next: Next) -> Response {
    let negado = || Erro(StatusCode::FORBIDDEN, "Você não tem permissão para isso.".into()).into_response();
    let (mut partes, corpo) = req.into_parts();
    let (Some(e), Some(quem)) = (partes.extensions.get::<Estado>().cloned(), partes.extensions.get::<Quem>().cloned()) else {
        return negado();
    };
    match exigencia {
        Exigencia::Dentro | Exigencia::ConfereNoHandler => {}
        Exigencia::Sistema(acao) => {
            if !e.visao(&quem).pode(Recurso::Sistema, acao) {
                return negado();
            }
        }
        Exigencia::Item(tipo, acao) => {
            let params = RawPathParams::from_request_parts(&mut partes, &()).await;
            let id = params.ok().and_then(|p| p.iter().find(|(k, _)| *k == "id").map(|(_, v)| id_do_item(v).to_string()));
            let Some(id) = id else { return negado() };
            if let Err(erro) = e.exigir(&quem, tipo, &id, acao) {
                return erro.into_response();
            }
        }
    }
    next.run(Request::from_parts(partes, corpo)).await
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
    Ok(Json(json!({ "arquivada": a.arquivada })))
}

/// O item como JSON, com `arquivada` e as `acoes` que quem pede pode fazer nele
/// ao lado dos campos dele.
fn com_estado(item: impl serde::Serialize, tipo: Tipo, id: &str, arquivadas: &Arquivadas, visao: &Visao) -> Value {
    let mut v = json!(item);
    v["arquivada"] = json!(arquivadas.contem(tipo, id));
    v["acoes"] = json!(visao.acoes(Recurso::Item(tipo, id)));
    v
}

fn visivel(visao: &Visao, tipo: Tipo, id: &str) -> bool {
    visao.pode(Recurso::Item(tipo, id), Acao::Ver)
}

/// O fim da linha das listas em texto.
pub(crate) fn marca_arquivada(arquivada: bool) -> &'static str {
    if arquivada { " | arquivada" } else { "" }
}

// ----- rubricas -----

/// As rubricas que `visao` vê e os arquivos com problema. Os problemas citam
/// arquivos pelo nome: só quem vê tudo os vê.
fn rubricas_visiveis(e: &Estado, visao: &Visao) -> Res<(Vec<Rubrica>, Vec<String>)> {
    let (mut rubricas, problemas) = e.dados.listar_rubricas()?;
    rubricas.retain(|r| visivel(visao, Tipo::Rubricas, &r.id));
    let problemas = if visao.pode(Recurso::Sistema, Acao::VerTudo) { problemas } else { Vec::new() };
    Ok((rubricas, problemas))
}

async fn listar_rubricas(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Json<Value>> {
    let arquivadas = e.dados.arquivadas();
    let visao = e.visao(&u);
    let (rubricas, problemas) = rubricas_visiveis(&e, &visao)?;
    let rubricas: Vec<Value> = rubricas.iter().map(|r| com_estado(r, Tipo::Rubricas, &r.id, &arquivadas, &visao)).collect();
    Ok(Json(json!({ "rubricas": rubricas, "problemas": problemas })))
}

/// Para quem gasta tokens lendo (o Claude): uma linha por rubrica.
async fn listar_rubricas_texto(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Response> {
    let arquivadas = e.dados.arquivadas();
    let (rubricas, problemas) = rubricas_visiveis(&e, &e.visao(&u))?;
    let mut texto = String::new();
    for r in &rubricas {
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
        None => Json(com_estado(e.dados.ler_rubrica(&id)?, Tipo::Rubricas, &id, &e.dados.arquivadas(), &e.visao(&u))).into_response(),
    })
}

async fn criar_rubrica(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(r): Json<Rubrica>) -> Res<Json<Rubrica>> {
    Ok(Json(e.criar(Tipo::Rubricas, &u, |r: &Rubrica| &r.id, |d| d.salvar_rubrica(None, r))?))
}

/// `PUT /rubricas/{id}` recebe JSON (editor web); `PUT /rubricas/{id}.toml`, o
/// arquivo inteiro em TOML, e responde com uma linha só. Gravar sobre uma
/// rubrica que existe pede `editar` nela; gravar uma nova pede poder criar, e
/// quem cria vira dono.
async fn salvar_rubrica(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>, corpo: String) -> Res<Response> {
    let toml = id.ends_with(".toml");
    let id = id_do_item(&id);
    let gravar = |d: &Dados| -> dados::Res<Rubrica> {
        if toml {
            return d.salvar_rubrica_toml(id, &corpo);
        }
        let r = serde_json::from_str(&corpo).map_err(|err| dados::Erro::Invalido(format!("Pedido inválido: {err}")))?;
        d.salvar_rubrica(Some(id), r)
    };
    let r = if e.dados.existe(Tipo::Rubricas, id)? {
        e.exigir(&u, Tipo::Rubricas, id, Acao::Editar)?;
        gravar(&e.dados)?
    } else if e.visao(&u).pode(Recurso::Sistema, Acao::Criar) {
        e.criar(Tipo::Rubricas, &u, |r: &Rubrica| &r.id, gravar)?
    } else {
        return Err(Erro(StatusCode::FORBIDDEN, "Seu usuário não pode criar rubricas.".into()));
    };
    if toml {
        let (criterios, passos) = r.contagem();
        return Ok(texto_puro(format!("Gravada: {} ({criterios} critérios, {passos} passos)\n", r.id)));
    }
    Ok(Json(r).into_response())
}

async fn apagar_rubrica(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.apagar(Tipo::Rubricas, &id)
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
            .map(|t| com_estado(&t, Tipo::Turmas, &t.id, &arquivadas, &visao))
            .collect(),
    ))
}

async fn ler_turma(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(id): Path<String>) -> Res<Json<Value>> {
    Ok(Json(com_estado(e.dados.ler_turma(&id)?, Tipo::Turmas, &id, &e.dados.arquivadas(), &e.visao(&u))))
}

async fn criar_turma(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(t): Json<NovaTurma>) -> Res<Json<Turma>> {
    Ok(Json(e.criar(Tipo::Turmas, &u, |t: &Turma| &t.id, |d| d.criar_turma(t.nome.trim(), &t.csv))?))
}

async fn salvar_turma(State(e): State<Estado>, Path(id): Path<String>, Json(t): Json<ListaAlunos>) -> Res<Json<Turma>> {
    Ok(Json(e.dados.substituir_turma(&id, &t.csv)?))
}

async fn apagar_turma(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.apagar(Tipo::Turmas, &id)
}

// ----- avaliações -----

#[derive(Deserialize)]
struct RubricaEscolhida {
    origem: String,
    #[serde(default = "um")]
    peso: f64,
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
        let andamentos: Vec<Andamento> = alunos.iter().map(|aluno| calcular(&a, correcoes.get(&aluno.id)).andamento()).collect();
        let quantos = |x| andamentos.iter().filter(|a| **a == x).count();
        let (concluidos, iniciados) = (quantos(Andamento::Concluida), quantos(Andamento::Parcial));
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
            e.exigir(u, Tipo::Rubricas, &r.origem, Acao::Ver)?;
        }
    }
    if atual.is_none_or(|a| a.turma != d.turma) {
        e.exigir(u, Tipo::Turmas, &d.turma, Acao::Ver)?;
    }
    Ok(())
}

fn gravar_avaliacao(d: &Dados, id: Option<&str>, a: DadosAvaliacao) -> dados::Res<Avaliacao> {
    let rubricas = a.rubricas.into_iter().map(|r| (r.origem, r.peso)).collect();
    d.salvar_avaliacao(id, a.titulo.trim().to_string(), a.turma, a.nota_maxima, rubricas, &a.atualizar)
}

async fn criar_avaliacao(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(d): Json<DadosAvaliacao>) -> Res<Json<Avaliacao>> {
    exigir_usar(&e, &u, None, &d)?;
    Ok(Json(e.criar(Tipo::Avaliacoes, &u, |a: &Avaliacao| &a.id, |dados| gravar_avaliacao(dados, None, d))?))
}

async fn salvar_avaliacao(
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path(id): Path<String>,
    Json(d): Json<DadosAvaliacao>,
) -> Res<Json<Avaliacao>> {
    exigir_usar(&e, &u, Some(&id), &d)?;
    Ok(Json(gravar_avaliacao(&e.dados, Some(&id), d)?))
}

async fn apagar_avaliacao(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.apagar(Tipo::Avaliacoes, &id)
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
        let usuarios = auth::Usuarios::abrir(&raiz);
        for (nome, papel) in [("vini", "admin"), ("bia", "professor"), ("ana", "monitor"), ("maquina", "executor")] {
            usuarios.criar(nome, "senha-longa", papel).unwrap();
        }
        let [vini, bia, ana, maquina] = ["vini", "bia", "ana", "maquina"].map(|n| usuarios.criar_token(n).unwrap());
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
}
