//! Usuários, a própria senha e o compartilhamento de avaliações, rubricas e
//! turmas. Quem pode chamar cada rota está em `api::exigencia`.

use crate::api::{Erro, Estado, Res};
use crate::auth;
use crate::dados::Tipo;
use crate::ead::bloqueante;
use crate::permissoes::{Quem, Recurso, nome_tipo};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, put};
use axum::{Extension, Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn rotas() -> Router<Estado> {
    let mut r = Router::new()
        .route("/usuarios", get(listar_usuarios).post(criar_usuario))
        .route("/usuarios/{nome}", patch(alterar_usuario).delete(remover_usuario))
        .route("/senha", put(trocar_senha));
    for (pasta, tipo) in [("avaliacoes", Tipo::Avaliacoes), ("rubricas", Tipo::Rubricas), ("turmas", Tipo::Turmas)] {
        r = r
            .route(&format!("/{pasta}/{{id}}/acessos"), get(move |e, p| listar_acessos(tipo, e, p)))
            .route(
                &format!("/{pasta}/{{id}}/acessos/{{usuario}}"),
                put(move |e, u, p, j| conceder(tipo, e, u, p, j)).delete(move |e, u, p| revogar(tipo, e, u, p)),
            );
    }
    r
}

fn invalido(m: impl Into<String>) -> Erro {
    Erro(StatusCode::UNPROCESSABLE_ENTITY, m.into())
}

fn interno(m: String) -> Erro {
    Erro(StatusCode::INTERNAL_SERVER_ERROR, m)
}

// ----- usuários -----

/// Todos veem a lista de nomes (para escolher com quem compartilhar); só quem
/// gere usuários vê quem tem token de API.
async fn listar_usuarios(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Json<Value>> {
    let gere = e.visao(&u).pode(Recurso::Sistema, "gerir_usuarios");
    let lista: Vec<Value> = auth::listar(e.dados.raiz())
        .map_err(interno)?
        .into_iter()
        .map(|r| if gere { json!(r) } else { json!({ "nome": r.nome, "papel": r.papel }) })
        .collect();
    Ok(Json(json!(lista)))
}

fn checar_papel_global(e: &Estado, papel: &str) -> Res<()> {
    if e.permissoes.papeis_globais().iter().any(|p| p == papel) {
        Ok(())
    } else {
        Err(invalido(format!("Papel desconhecido: {papel}")))
    }
}

/// Recusa deixar o sistema sem nenhum admin.
fn checar_resta_admin(e: &Estado, sem: &str) -> Res<()> {
    let outros = auth::listar(e.dados.raiz()).map_err(interno)?.into_iter().filter(|r| r.papel == "admin" && !r.nome.eq_ignore_ascii_case(sem)).count();
    if outros == 0 {
        return Err(Erro(StatusCode::CONFLICT, "É preciso ficar pelo menos um admin.".into()));
    }
    Ok(())
}

#[derive(Deserialize)]
struct NovoUsuario {
    nome: String,
    senha: String,
    papel: String,
}

async fn criar_usuario(State(e): State<Estado>, Json(n): Json<NovoUsuario>) -> Res<Json<Value>> {
    checar_papel_global(&e, &n.papel)?;
    let nome = n.nome.trim().to_string();
    if auth::nome_gravado(e.dados.raiz(), &nome).is_some() {
        return Err(Erro(StatusCode::CONFLICT, format!("Já existe o usuário {nome}.")));
    }
    let raiz = e.dados.raiz().to_path_buf();
    let n2 = nome.clone();
    bloqueante(move || auth::definir_senha(&raiz, &n2, &n.senha, &n.papel).map_err(invalido)).await?;
    Ok(Json(json!({ "nome": nome })))
}

#[derive(Deserialize)]
struct AlteracaoUsuario {
    #[serde(default)]
    papel: Option<String>,
    /// Senha nova definida por quem gere usuários (a pessoa esqueceu a dela).
    #[serde(default)]
    senha: Option<String>,
}

async fn alterar_usuario(State(e): State<Estado>, Path(nome): Path<String>, Json(a): Json<AlteracaoUsuario>) -> Res<Json<Value>> {
    let raiz = e.dados.raiz().to_path_buf();
    let nome = auth::nome_gravado(&raiz, &nome).ok_or(Erro(StatusCode::NOT_FOUND, format!("Não existe usuário {nome}.")))?;
    if let Some(papel) = &a.papel {
        checar_papel_global(&e, papel)?;
        if papel != "admin" {
            checar_resta_admin(&e, &nome)?;
        }
    }
    if let Some(senha) = a.senha {
        let (r, n) = (raiz.clone(), nome.clone());
        bloqueante(move || auth::definir_senha(&r, &n, &senha, "").map(|_| ()).map_err(invalido)).await?;
    }
    if let Some(papel) = &a.papel {
        auth::definir_papel(&raiz, &nome, papel).map_err(interno)?;
    }
    Ok(Json(json!({ "nome": nome })))
}

async fn remover_usuario(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(nome): Path<String>) -> Res<Json<Value>> {
    if nome.eq_ignore_ascii_case(&u.nome) {
        return Err(Erro(StatusCode::CONFLICT, "Você não pode remover a si mesmo.".into()));
    }
    checar_resta_admin(&e, &nome)?;
    if !auth::remover_usuario(e.dados.raiz(), &nome).map_err(interno)? {
        return Err(Erro(StatusCode::NOT_FOUND, format!("Não existe usuário {nome}.")));
    }
    e.permissoes.esquecer_usuario(&nome).map_err(interno)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct TrocaSenha {
    atual: String,
    nova: String,
}

async fn trocar_senha(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(t): Json<TrocaSenha>) -> Res<Json<Value>> {
    let raiz = e.dados.raiz().to_path_buf();
    bloqueante(move || {
        if !auth::senha_confere(&raiz, &u.nome, &t.atual) {
            return Err(Erro(StatusCode::FORBIDDEN, "A senha atual não confere.".into()));
        }
        auth::definir_senha(&raiz, &u.nome, &t.nova, "").map_err(invalido)
    })
    .await?;
    // Trocar a senha derruba todas as sessões, inclusive esta: a tela pede para entrar de novo.
    Ok(Json(json!({})))
}

// ----- compartilhar -----

fn existe(e: &Estado, tipo: Tipo, id: &str) -> Res<()> {
    match tipo {
        Tipo::Avaliacoes => e.dados.ler_avaliacao(id).map(|_| ()),
        Tipo::Rubricas => e.dados.ler_rubrica(id).map(|_| ()),
        Tipo::Turmas => e.dados.ler_turma(id).map(|_| ()),
    }
    .map_err(Erro::from)
}

async fn listar_acessos(tipo: Tipo, State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    existe(&e, tipo, &id)?;
    Ok(Json(json!({
        "acessos": e.permissoes.acessos(tipo, &id),
        "papeis": e.permissoes.papeis()[nome_tipo(tipo)],
    })))
}

#[derive(Deserialize)]
struct Concessao {
    papel: String,
}

/// Recusa tirar o último dono: o item ficaria só para o admin.
fn checar_resta_dono(e: &Estado, tipo: Tipo, id: &str, sem: &str) -> Res<()> {
    let acessos = e.permissoes.acessos(tipo, id);
    let era_dono = acessos.iter().any(|a| a.papel == "dono" && a.usuario.eq_ignore_ascii_case(sem));
    let outros = acessos.iter().filter(|a| a.papel == "dono" && !a.usuario.eq_ignore_ascii_case(sem)).count();
    if era_dono && outros == 0 {
        return Err(Erro(StatusCode::CONFLICT, "É preciso ficar pelo menos um dono. Dê o papel de dono a outra pessoa antes.".into()));
    }
    Ok(())
}

async fn conceder(
    tipo: Tipo,
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path((id, usuario)): Path<(String, String)>,
    Json(c): Json<Concessao>,
) -> Res<Json<Value>> {
    existe(&e, tipo, &id)?;
    let usuario = auth::nome_gravado(e.dados.raiz(), &usuario).ok_or(Erro(StatusCode::NOT_FOUND, format!("Não existe usuário {usuario}.")))?;
    if !e.permissoes.papel_valido(tipo, &c.papel) {
        return Err(invalido(format!("Papel desconhecido: {}", c.papel)));
    }
    if c.papel != "dono" {
        checar_resta_dono(&e, tipo, &id, &usuario)?;
    }
    e.permissoes.conceder(tipo, &id, &usuario, &c.papel, &u.nome, auth::agora()).map_err(interno)?;
    Ok(Json(json!({ "acessos": e.permissoes.acessos(tipo, &id) })))
}

async fn revogar(tipo: Tipo, State(e): State<Estado>, Extension(_u): Extension<Quem>, Path((id, usuario)): Path<(String, String)>) -> Res<Json<Value>> {
    checar_resta_dono(&e, tipo, &id, &usuario)?;
    if !e.permissoes.revogar(tipo, &id, &usuario).map_err(interno)? {
        return Err(Erro(StatusCode::NOT_FOUND, format!("{usuario} não tinha acesso.")));
    }
    Ok(Json(json!({ "acessos": e.permissoes.acessos(tipo, &id) })))
}
