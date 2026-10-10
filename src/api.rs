//! Rotas HTTP. Tudo sob `/api` exige sessão (cookie) ou token de API
//! (`Authorization: Bearer …`), menos `/api/entrar`.

use crate::auth::{self, Auth, ErroLogin};
use crate::dados::{self, Arquivadas, Dados, Tipo};
use crate::ead;
use crate::estatico::Web;
use crate::modelo::*;
use axum::extract::{Path, Query, Request, State};
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
}

#[derive(Clone)]
pub(crate) struct Usuario(pub String);

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

async fn exigir_sessao(State(e): State<Estado>, mut req: Request, next: Next) -> Response {
    let usuario = match bearer(req.headers()) {
        Some(t) => e.auth.usuario_por_token(t),
        None => token(req.headers()).and_then(|t| e.auth.usuario(&t)),
    };
    match usuario {
        Some(nome) => {
            req.extensions_mut().insert(Usuario(nome));
            let mut r = next.run(req).await;
            r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            r
        }
        None => Erro(StatusCode::UNAUTHORIZED, "Entre para continuar".into()).into_response(),
    }
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

async fn sessao(Extension(u): Extension<Usuario>) -> Json<Value> {
    Json(json!({ "usuario": u.0 }))
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

/// O item (que tem `id`) como JSON, com `arquivada` ao lado dos campos dele.
fn com_arquivada(item: impl serde::Serialize, tipo: Tipo, arquivadas: &Arquivadas) -> Value {
    let mut v = json!(item);
    let arquivada = v["id"].as_str().is_some_and(|id| arquivadas.contem(tipo, id));
    v["arquivada"] = json!(arquivada);
    v
}

/// O fim da linha das listas em texto.
pub(crate) fn marca_arquivada(arquivada: bool) -> &'static str {
    if arquivada { " | arquivada" } else { "" }
}

// ----- rubricas -----

async fn listar_rubricas(State(e): State<Estado>) -> Res<Json<Value>> {
    let (rubricas, problemas) = e.dados.listar_rubricas()?;
    let arquivadas = e.dados.arquivadas();
    let rubricas: Vec<Value> = rubricas.into_iter().map(|r| com_arquivada(r, Tipo::Rubricas, &arquivadas)).collect();
    Ok(Json(json!({ "rubricas": rubricas, "problemas": problemas })))
}

/// Para quem gasta tokens lendo (o Claude): uma linha por rubrica.
async fn listar_rubricas_texto(State(e): State<Estado>) -> Res<Response> {
    let (rubricas, problemas) = e.dados.listar_rubricas()?;
    let arquivadas = e.dados.arquivadas();
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
async fn ler_rubrica(State(e): State<Estado>, Path(id): Path<String>) -> Res<Response> {
    Ok(match id.strip_suffix(".toml") {
        Some(id) => texto_puro(e.dados.rubrica_toml(id)?),
        None => Json(com_arquivada(e.dados.ler_rubrica(&id)?, Tipo::Rubricas, &e.dados.arquivadas())).into_response(),
    })
}

async fn criar_rubrica(State(e): State<Estado>, Json(r): Json<Rubrica>) -> Res<Json<Rubrica>> {
    Ok(Json(e.dados.salvar_rubrica(None, r)?))
}

/// `PUT /rubricas/{id}` recebe JSON (editor web); `PUT /rubricas/{id}.toml`, o
/// arquivo inteiro em TOML, e responde com uma linha só.
async fn salvar_rubrica(State(e): State<Estado>, Path(id): Path<String>, corpo: String) -> Res<Response> {
    if let Some(id) = id.strip_suffix(".toml") {
        let r = e.dados.salvar_rubrica_toml(id, &corpo)?;
        let (criterios, passos) = r.contagem();
        return Ok(texto_puro(format!("Gravada: {} ({criterios} critérios, {passos} passos)\n", r.id)));
    }
    let r: Rubrica =
        serde_json::from_str(&corpo).map_err(|err| Erro(StatusCode::UNPROCESSABLE_ENTITY, format!("Pedido inválido: {err}")))?;
    Ok(Json(e.dados.salvar_rubrica(Some(&id), r)?).into_response())
}

async fn apagar_rubrica(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.dados.apagar_rubrica(&id)?;
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

async fn listar_turmas(State(e): State<Estado>) -> Res<Json<Vec<Value>>> {
    let arquivadas = e.dados.arquivadas();
    Ok(Json(e.dados.listar_turmas()?.into_iter().map(|t| com_arquivada(t, Tipo::Turmas, &arquivadas)).collect()))
}

async fn ler_turma(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    Ok(Json(com_arquivada(e.dados.ler_turma(&id)?, Tipo::Turmas, &e.dados.arquivadas())))
}

async fn criar_turma(State(e): State<Estado>, Json(t): Json<NovaTurma>) -> Res<Json<Turma>> {
    Ok(Json(e.dados.salvar_turma(t.nome.trim(), &t.csv, true)?))
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

async fn listar_avaliacoes(State(e): State<Estado>) -> Res<Json<Value>> {
    let mut lista = Vec::new();
    let arquivadas = e.dados.arquivadas();
    for a in e.dados.listar_avaliacoes()? {
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
        }));
    }
    Ok(Json(json!(lista)))
}

/// Tudo que a tela de avaliação precisa, de uma vez, para funcionar sem rede depois.
async fn pacote(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    let avaliacao = e.dados.ler_avaliacao(&id)?;
    let turma = e.dados.ler_turma(&avaliacao.turma)?;
    let correcoes = e.dados.ler_correcoes(&id)?;
    let desatualizadas = e.dados.rubricas_desatualizadas(&avaliacao);
    let ead = ead::resumo(&e, &id)?;
    let arquivada = e.dados.arquivadas().contem(Tipo::Avaliacoes, &id);
    Ok(Json(json!({ "avaliacao": avaliacao, "turma": turma, "correcoes": correcoes, "desatualizadas": desatualizadas, "ead": ead, "arquivada": arquivada })))
}

fn gravar_avaliacao(e: &Estado, id: Option<&str>, d: DadosAvaliacao) -> Res<Json<Avaliacao>> {
    let rubricas = d.rubricas.into_iter().map(|r| (r.origem, r.peso)).collect();
    Ok(Json(e.dados.salvar_avaliacao(id, d.titulo.trim().to_string(), d.turma, d.nota_maxima, rubricas, &d.atualizar)?))
}

async fn criar_avaliacao(State(e): State<Estado>, Json(d): Json<DadosAvaliacao>) -> Res<Json<Avaliacao>> {
    gravar_avaliacao(&e, None, d)
}

async fn salvar_avaliacao(State(e): State<Estado>, Path(id): Path<String>, Json(d): Json<DadosAvaliacao>) -> Res<Json<Avaliacao>> {
    // Rubricas atualizadas podem mudar as sugestões do pipeline.
    e.avisos.fila_mudou();
    gravar_avaliacao(&e, Some(&id), d)
}

async fn apagar_avaliacao(State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    e.dados.apagar_avaliacao(&id)?;
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
    Extension(u): Extension<Usuario>,
    Path((id, aluno)): Path<(String, String)>,
    Json(a): Json<Alteracoes>,
) -> Res<Json<Value>> {
    let respostas = a.respostas.into_iter().map(|r| (r.rubrica, r.criterio, r.resposta)).collect();
    let (correcao, recusadas) = e.dados.alterar_correcao(&id, &aluno, respostas, a.comentario_geral, &u.0, auth::agora())?;
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
