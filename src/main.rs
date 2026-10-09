mod api;
mod auth;
mod dados;
mod estatico;
mod modelo;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "rubricas", version, about = "Avaliação por rubricas: pasta de arquivos + aplicação web")]
struct Cli {
    #[command(subcommand)]
    comando: Comando,
}

#[derive(Subcommand)]
enum Comando {
    /// Sobe o servidor web
    Servir {
        /// Pasta com rubricas, turmas, avaliações e correções
        #[arg(long, default_value = "dados")]
        dados: PathBuf,
        /// Endereço e porta de escuta
        #[arg(long, default_value = "127.0.0.1:8080")]
        endereco: String,
        /// Serve a aplicação web desta pasta em vez da embutida (para desenvolver)
        #[arg(long)]
        web: Option<PathBuf>,
    },
    /// Cria um usuário ou troca a senha de um existente
    Usuario {
        nome: String,
        #[arg(long, default_value = "dados")]
        dados: PathBuf,
        /// Remove o usuário em vez de definir a senha
        #[arg(long)]
        remover: bool,
    },
}

fn pedir_senha() -> Result<String, String> {
    // Para scripts: RUBRICAS_SENHA evita o prompt.
    if let Ok(s) = std::env::var("RUBRICAS_SENHA") {
        return Ok(s);
    }
    let a = rpassword::prompt_password("Senha: ").map_err(|e| e.to_string())?;
    let b = rpassword::prompt_password("Repita a senha: ").map_err(|e| e.to_string())?;
    if a == b { Ok(a) } else { Err("As senhas não conferem".into()) }
}

fn usuario(nome: &str, dados: &PathBuf, remover: bool) -> Result<String, String> {
    std::fs::create_dir_all(dados).map_err(|e| format!("{}: {e}", dados.display()))?;
    if remover {
        return match auth::remover_usuario(dados, nome)? {
            true => Ok(format!("Usuário {nome} removido.")),
            false => Err(format!("Não existe usuário {nome}.")),
        };
    }
    let senha = pedir_senha()?;
    Ok(match auth::definir_senha(dados, nome, &senha)? {
        true => format!("Usuário {nome} criado."),
        false => format!("Senha de {nome} atualizada."),
    })
}

async fn servir(dados: PathBuf, endereco: String, web: Option<PathBuf>) -> Result<(), String> {
    let pasta = dados::Dados::abrir(&dados).map_err(|e| format!("{}: {e}", dados.display()))?;
    let auth = auth::Auth::abrir(pasta.raiz());
    if auth.total_usuarios() == 0 {
        eprintln!("Nenhum usuário cadastrado. Crie um com:\n  rubricas usuario <nome> --dados {}\n", dados.display());
    }
    let estado = api::Estado { dados: Arc::new(pasta), auth: Arc::new(auth), web: Arc::new(estatico::Web::novo(web)) };
    let escuta = tokio::net::TcpListener::bind(&endereco).await.map_err(|e| format!("{endereco}: {e}"))?;
    println!("Dados em {}", dados.display());
    println!("Servindo em http://{}", escuta.local_addr().map_err(|e| e.to_string())?);
    axum::serve(escuta, api::rotas(estado))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> ExitCode {
    let resultado = match Cli::parse().comando {
        Comando::Servir { dados, endereco, web } => servir(dados, endereco, web).await,
        Comando::Usuario { nome, dados, remover } => usuario(&nome, &dados, remover).map(|msg| println!("{msg}")),
    };
    match resultado {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Erro: {e}");
            ExitCode::FAILURE
        }
    }
}
