//! Usuários, a própria senha e o compartilhamento de avaliações, rubricas e
//! turmas. Cada rota diz o que exige ao ser registrada.

use crate::api::{Erro, Estado, Exigencia, Res, Rotas, bloqueante, metodos};
use crate::auth::{self, Usuarios};
use crate::dados::{self, Tipo};
use crate::permissoes::{Acao, Permissoes, Quem, Recurso};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn rotas() -> Rotas {
    use Exigencia::*;
    let gerir = Sistema(Acao::GerirUsuarios);
    let mut r = Rotas::new()
        .rota("/usuarios", metodos().get(Dentro, listar_usuarios).post(gerir, criar_usuario))
        .rota("/usuarios/{nome}", metodos().patch(gerir, alterar_usuario).delete(gerir, remover_usuario))
        .rota("/senha", metodos().put(Dentro, trocar_senha));
    for tipo in [Tipo::Avaliacoes, Tipo::Rubricas, Tipo::Turmas] {
        let compartilhar = Item(tipo, Acao::Compartilhar);
        r = r
            .rota(&format!("/{}/{{id}}/acessos", tipo.pasta()), metodos().get(compartilhar, move |e, p| listar_acessos(tipo, e, p)))
            .rota(
                &format!("/{}/{{id}}/acessos/{{usuario}}", tipo.pasta()),
                metodos()
                    .put(compartilhar, move |e, u, p, j| conceder(tipo, e, u, p, j))
                    .delete(compartilhar, move |e, p| revogar(tipo, e, p)),
            );
    }
    r
}

fn invalido(m: impl Into<String>) -> Erro {
    Erro(StatusCode::UNPROCESSABLE_ENTITY, m.into())
}

/// Remove o usuário e os acessos dele, que não devem valer para outro criado
/// depois com o mesmo nome. Recusa remover o último que gere usuários.
pub(crate) fn remover(usuarios: &Usuarios, permissoes: &Permissoes, nome: &str) -> dados::Res<()> {
    usuarios.remover(nome, |papel| permissoes.gere_usuarios(papel))?;
    permissoes.esquecer_usuario(nome)
}

// ----- usuários -----

/// Todos veem a lista de nomes (para escolher com quem compartilhar); só quem
/// gere usuários vê quem tem token de API.
async fn listar_usuarios(State(e): State<Estado>, Extension(u): Extension<Quem>) -> Res<Json<Value>> {
    let gere = e.visao(&u).pode(Recurso::Sistema, Acao::GerirUsuarios);
    let lista: Vec<Value> = e
        .auth
        .usuarios
        .listar()?
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

#[derive(Deserialize)]
struct NovoUsuario {
    nome: String,
    senha: String,
    papel: String,
}

async fn criar_usuario(State(e): State<Estado>, Json(n): Json<NovoUsuario>) -> Res<Json<Value>> {
    checar_papel_global(&e, &n.papel)?;
    let nome = n.nome.trim().to_string();
    let (auth, n2) = (e.auth.clone(), nome.clone());
    bloqueante(move || Ok(auth.usuarios.criar(&n2, &n.senha, &n.papel)?)).await?;
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
    let nome = e.auth.usuarios.nome_gravado(&nome).ok_or(Erro(StatusCode::NOT_FOUND, format!("Não existe usuário {nome}.")))?;
    if let Some(papel) = &a.papel {
        checar_papel_global(&e, papel)?;
        e.auth.usuarios.definir_papel(&nome, papel, |p| e.permissoes.gere_usuarios(p))?;
    }
    if let Some(senha) = a.senha {
        let (auth, n) = (e.auth.clone(), nome.clone());
        bloqueante(move || Ok(auth.usuarios.trocar_senha(&n, &senha)?)).await?;
    }
    Ok(Json(json!({ "nome": nome })))
}

async fn remover_usuario(State(e): State<Estado>, Extension(u): Extension<Quem>, Path(nome): Path<String>) -> Res<Json<Value>> {
    if nome.eq_ignore_ascii_case(&u.nome) {
        return Err(Erro(StatusCode::CONFLICT, "Você não pode remover a si mesmo.".into()));
    }
    remover(&e.auth.usuarios, &e.permissoes, &nome)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct TrocaSenha {
    atual: String,
    nova: String,
}

async fn trocar_senha(State(e): State<Estado>, Extension(u): Extension<Quem>, Json(t): Json<TrocaSenha>) -> Res<Json<Value>> {
    let auth = e.auth.clone();
    bloqueante(move || {
        if !auth.usuarios.senha_confere(&u.nome, &t.atual) {
            return Err(Erro(StatusCode::FORBIDDEN, "A senha atual não confere.".into()));
        }
        Ok(auth.usuarios.trocar_senha(&u.nome, &t.nova)?)
    })
    .await?;
    // Trocar a senha derruba todas as sessões, inclusive esta: a tela pede para entrar de novo.
    Ok(Json(json!({})))
}

// ----- compartilhar -----

fn existe(e: &Estado, tipo: Tipo, id: &str) -> Res<()> {
    if e.dados.existe(tipo, id)? { Ok(()) } else { Err(Erro(StatusCode::NOT_FOUND, format!("“{id}” não existe em {}/", tipo.pasta()))) }
}

async fn listar_acessos(tipo: Tipo, State(e): State<Estado>, Path(id): Path<String>) -> Res<Json<Value>> {
    existe(&e, tipo, &id)?;
    Ok(Json(json!({
        "acessos": e.permissoes.acessos(tipo, &id),
        "papeis": e.permissoes.papeis()[tipo.nome()],
    })))
}

#[derive(Deserialize)]
struct Concessao {
    papel: String,
}

async fn conceder(
    tipo: Tipo,
    State(e): State<Estado>,
    Extension(u): Extension<Quem>,
    Path((id, usuario)): Path<(String, String)>,
    Json(c): Json<Concessao>,
) -> Res<Json<Value>> {
    existe(&e, tipo, &id)?;
    let usuario = e.auth.usuarios.nome_gravado(&usuario).ok_or(Erro(StatusCode::NOT_FOUND, format!("Não existe usuário {usuario}.")))?;
    e.permissoes.conceder(tipo, &id, &usuario, &c.papel, &u.nome, auth::agora())?;
    Ok(Json(json!({ "acessos": e.permissoes.acessos(tipo, &id) })))
}

async fn revogar(tipo: Tipo, State(e): State<Estado>, Path((id, usuario)): Path<(String, String)>) -> Res<Json<Value>> {
    if !e.permissoes.revogar(tipo, &id, &usuario)? {
        return Err(Erro(StatusCode::NOT_FOUND, format!("{usuario} não tinha acesso.")));
    }
    Ok(Json(json!({ "acessos": e.permissoes.acessos(tipo, &id) })))
}
