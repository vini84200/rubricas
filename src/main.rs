mod api;
mod auth;
mod conferir;
mod dados;
mod ead;
mod estatico;
mod executor;
mod fila;
mod modelo;
mod moodle;
mod pipeline;
mod submissoes;

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
    /// Cria (ou troca) o token de API de um usuário, para scripts e para o Claude
    Token {
        nome: String,
        #[arg(long, default_value = "dados")]
        dados: PathBuf,
        /// Remove o token em vez de criar um
        #[arg(long)]
        remover: bool,
    },
    /// Roda as checagens pendentes (pipelines) em contêineres podman e envia os resultados
    Executor {
        /// Endereço do servidor, por exemplo https://rubrica.vini84200.cc
        #[arg(long, env = "RUBRICAS_URL")]
        servidor: String,
        /// Token de API (rubricas token <nome>, no servidor)
        #[arg(long, env = "RUBRICAS_TOKEN", hide_env_values = true)]
        token: String,
        /// Continua rodando e espera novas submissões em vez de terminar com a fila vazia
        #[arg(long)]
        continuo: bool,
    },
    /// Instala a skill do Claude Code em ~/.claude/skills/rubricas
    Skill {
        #[command(subcommand)]
        acao: AcaoSkill,
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

#[derive(Subcommand)]
enum AcaoSkill {
    /// Copia a skill para ~/.claude/skills/rubricas/SKILL.md
    Instalar,
}

const SKILL: &str = include_str!("../claude/skills/rubricas/SKILL.md");

fn instalar_skill() -> Result<String, String> {
    let casa = std::env::var_os("HOME").ok_or("HOME não definido")?;
    let destino = PathBuf::from(casa).join(".claude/skills/rubricas/SKILL.md");
    dados::gravar(&destino, SKILL.as_bytes()).map_err(|e| format!("{}: {e}", destino.display()))?;
    Ok(format!(
        "Skill instalada em {}.\nDefina RUBRICAS_URL (ex.: https://rubrica.vini84200.cc) e RUBRICAS_TOKEN (rubricas token <nome>).",
        destino.display()
    ))
}

fn token(nome: &str, dados: &std::path::Path, remover: bool) -> Result<String, String> {
    if remover {
        return match auth::remover_token(dados, nome)? {
            true => Ok(format!("Token de {nome} removido.")),
            false => Err(format!("{nome} não tem token.")),
        };
    }
    let t = auth::criar_token(dados, nome)?;
    Ok(format!("{t}\n\nToken de {nome} criado (o anterior, se havia, deixou de valer). Ele não será mostrado de novo."))
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
    let estado = api::Estado {
        dados: Arc::new(pasta),
        auth: Arc::new(auth),
        web: Arc::new(estatico::Web::novo(web)),
        avisos: Arc::new(ead::Avisos::novo()),
    };
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
        Comando::Token { nome, dados, remover } => token(&nome, &dados, remover).map(|msg| println!("{msg}")),
        Comando::Executor { servidor, token, continuo } => {
            let op = executor::Opcoes { servidor, token, continuo };
            tokio::task::spawn_blocking(move || executor::rodar(op)).await.unwrap_or_else(|e| Err(e.to_string()))
        }
        Comando::Skill { acao: AcaoSkill::Instalar } => instalar_skill().map(|msg| println!("{msg}")),
    };
    match resultado {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Erro: {e}");
            ExitCode::FAILURE
        }
    }
}
