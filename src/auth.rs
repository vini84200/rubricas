//! Usuários locais (senha com Argon2id em `usuarios.toml`) e sessões por cookie.

use crate::dados::gravar;
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

pub const COOKIE: &str = "rubricas_sessao";
pub const DURACAO_SESSAO: u64 = 30 * 24 * 3600;
const MAX_FALHAS: u32 = 8;
const JANELA_FALHAS: u64 = 10 * 60;

#[derive(Debug, Default, Serialize, Deserialize)]
struct ArquivoUsuarios {
    #[serde(default)]
    usuarios: Vec<Usuario>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Usuario {
    nome: String,
    hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sessao {
    usuario: String,
    expira: u64,
    /// Resumo do hash de senha no momento do login: trocar a senha ou recriar o
    /// usuário muda o hash e derruba as sessões antigas.
    #[serde(default)]
    marca: String,
}

#[derive(Debug, PartialEq)]
pub enum ErroLogin {
    Credenciais,
    /// Muitas tentativas; segundos até poder tentar de novo.
    Bloqueado(u64),
}

pub struct Auth {
    usuarios: PathBuf,
    arquivo_sessoes: PathBuf,
    /// SHA-256 do token → sessão. O token em si só existe no cookie.
    sessoes: Mutex<HashMap<String, Sessao>>,
    /// usuário → (falhas, início da janela)
    falhas: Mutex<HashMap<String, (u32, u64)>>,
}

pub fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn resumo(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn gerar_hash(senha: &str) -> Result<String, String> {
    Argon2::default().hash_password(senha.as_bytes()).map(|h| h.to_string()).map_err(|e| e.to_string())
}

fn confere(senha: &str, hash: &str) -> bool {
    PasswordHash::new(hash).map(|h| Argon2::default().verify_password(senha.as_bytes(), &h).is_ok()).unwrap_or(false)
}

fn ler_usuarios(caminho: &Path) -> io::Result<ArquivoUsuarios> {
    match fs::read_to_string(caminho) {
        Ok(t) => toml::from_str(&t).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(ArquivoUsuarios::default()),
        Err(e) => Err(e),
    }
}

pub fn nome_valido(nome: &str) -> bool {
    !nome.is_empty() && nome.len() <= 40 && nome.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Cria o usuário ou troca a senha de um existente.
pub fn definir_senha(raiz: &Path, nome: &str, senha: &str) -> Result<bool, String> {
    if !nome_valido(nome) {
        return Err("Use só letras sem acento, números, ponto, hífen ou sublinhado no nome".into());
    }
    if senha.chars().count() < 8 {
        return Err("A senha precisa ter pelo menos 8 caracteres".into());
    }
    let caminho = raiz.join("usuarios.toml");
    let mut arq = ler_usuarios(&caminho).map_err(|e| e.to_string())?;
    let hash = gerar_hash(senha)?;
    let existente = arq.usuarios.iter_mut().find(|u| u.nome.eq_ignore_ascii_case(nome));
    let novo = existente.is_none();
    match existente {
        Some(u) => u.hash = hash,
        None => arq.usuarios.push(Usuario { nome: nome.to_string(), hash }),
    }
    gravar(&caminho, toml::to_string_pretty(&arq).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    restringir(&caminho);
    Ok(novo)
}

pub fn remover_usuario(raiz: &Path, nome: &str) -> Result<bool, String> {
    let caminho = raiz.join("usuarios.toml");
    let mut arq = ler_usuarios(&caminho).map_err(|e| e.to_string())?;
    let antes = arq.usuarios.len();
    arq.usuarios.retain(|u| !u.nome.eq_ignore_ascii_case(nome));
    if arq.usuarios.len() == antes {
        return Ok(false);
    }
    gravar(&caminho, toml::to_string_pretty(&arq).map_err(|e| e.to_string())?.as_bytes()).map_err(|e| e.to_string())?;
    restringir(&caminho);
    Ok(true)
}

/// Só o dono lê arquivos com hashes de senha ou sessões.
fn restringir(caminho: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(caminho, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = caminho;
}

impl Auth {
    pub fn abrir(raiz: &Path) -> Self {
        let arquivo_sessoes = raiz.join(".sessoes.json");
        let mut sessoes: HashMap<String, Sessao> =
            fs::read_to_string(&arquivo_sessoes).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let t = agora();
        sessoes.retain(|_, s| s.expira > t);
        Auth {
            usuarios: raiz.join("usuarios.toml"),
            arquivo_sessoes,
            sessoes: Mutex::new(sessoes),
            falhas: Mutex::new(HashMap::new()),
        }
    }

    pub fn total_usuarios(&self) -> usize {
        ler_usuarios(&self.usuarios).map(|a| a.usuarios.len()).unwrap_or(0)
    }

    fn persistir(&self, sessoes: &HashMap<String, Sessao>) {
        if gravar(&self.arquivo_sessoes, serde_json::to_string(sessoes).unwrap().as_bytes()).is_ok() {
            restringir(&self.arquivo_sessoes);
        }
    }

    /// Confere a senha e abre uma sessão. Devolve (token, nome do usuário).
    /// É lento de propósito (Argon2): chame fora do executor assíncrono.
    pub fn entrar(&self, usuario: &str, senha: &str) -> Result<(String, String), ErroLogin> {
        let chave = usuario.trim().to_ascii_lowercase();
        let t = agora();
        {
            let mut falhas = self.falhas.lock().unwrap();
            falhas.retain(|_, (_, inicio)| *inicio + JANELA_FALHAS > t);
            if let Some((n, inicio)) = falhas.get(&chave)
                && *n >= MAX_FALHAS {
                    return Err(ErroLogin::Bloqueado(inicio + JANELA_FALHAS - t));
                }
        }
        let arq = ler_usuarios(&self.usuarios).unwrap_or_default();
        let conta = arq.usuarios.iter().find(|u| u.nome.to_ascii_lowercase() == chave);
        // Sem conta, confere contra um hash qualquer para o tempo de resposta não
        // revelar se o usuário existe.
        static FALSO: OnceLock<String> = OnceLock::new();
        let hash = match conta {
            Some(u) => u.hash.as_str(),
            None => FALSO.get_or_init(|| gerar_hash("senha de ninguém").unwrap_or_default()).as_str(),
        };
        let ok = confere(senha, hash) && conta.is_some();
        let mut falhas = self.falhas.lock().unwrap();
        if !ok {
            if falhas.len() < 10_000 || falhas.contains_key(&chave) {
                falhas.entry(chave).or_insert((0, t)).0 += 1;
            }
            return Err(ErroLogin::Credenciais);
        }
        falhas.remove(&chave);
        drop(falhas);

        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("sem fonte de aleatoriedade");
        let token = hex(&bytes);
        let nome = conta.unwrap().nome.clone();
        let marca = resumo(&conta.unwrap().hash);
        let mut sessoes = self.sessoes.lock().unwrap();
        sessoes.retain(|_, s| s.expira > t);
        sessoes.insert(resumo(&token), Sessao { usuario: nome.clone(), expira: t + DURACAO_SESSAO, marca });
        self.persistir(&sessoes);
        Ok((token, nome))
    }

    pub fn usuario(&self, token: &str) -> Option<String> {
        let sessoes = self.sessoes.lock().unwrap();
        let s = sessoes.get(&resumo(token))?;
        if s.expira <= agora() {
            return None;
        }
        // Usuário removido, recriado ou com senha trocada pelo terminal perde o acesso na hora.
        let vale = ler_usuarios(&self.usuarios)
            .map(|a| a.usuarios.iter().any(|u| u.nome == s.usuario && resumo(&u.hash) == s.marca))
            .unwrap_or(false);
        vale.then(|| s.usuario.clone())
    }

    pub fn sair(&self, token: &str) {
        let mut sessoes = self.sessoes.lock().unwrap();
        if sessoes.remove(&resumo(token)).is_some() {
            self.persistir(&sessoes);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn login_sessao_e_bloqueio() {
        let raiz = std::env::temp_dir().join(format!("rubricas-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        fs::create_dir_all(&raiz).unwrap();

        assert!(definir_senha(&raiz, "vini", "curta").is_err());
        assert!(definir_senha(&raiz, "nome com espaço", "senha-longa").is_err());
        assert_eq!(definir_senha(&raiz, "vini", "senha-longa"), Ok(true));
        assert_eq!(definir_senha(&raiz, "Vini", "outra-senha-longa"), Ok(false));
        let texto = fs::read_to_string(raiz.join("usuarios.toml")).unwrap();
        assert!(texto.contains("$argon2id$") && !texto.contains("outra-senha-longa"));

        let auth = Auth::abrir(&raiz);
        assert_eq!(auth.entrar("vini", "senha-longa"), Err(ErroLogin::Credenciais));
        assert_eq!(auth.entrar("ninguem", "outra-senha-longa"), Err(ErroLogin::Credenciais));
        let (token, nome) = auth.entrar("VINI", "outra-senha-longa").unwrap();
        assert_eq!(nome, "vini");
        assert_eq!(auth.usuario(&token).as_deref(), Some("vini"));
        assert_eq!(auth.usuario("token-inventado"), None);

        // a sessão sobrevive a reiniciar o servidor; o arquivo não guarda o token
        assert!(!fs::read_to_string(raiz.join(".sessoes.json")).unwrap().contains(&token));
        assert_eq!(Auth::abrir(&raiz).usuario(&token).as_deref(), Some("vini"));

        auth.sair(&token);
        assert_eq!(auth.usuario(&token), None);

        for _ in 0..MAX_FALHAS {
            assert_eq!(auth.entrar("vini", "errada"), Err(ErroLogin::Credenciais));
        }
        assert!(matches!(auth.entrar("vini", "outra-senha-longa"), Err(ErroLogin::Bloqueado(_))));

        // trocar a senha derruba as sessões abertas com a antiga
        let outro = Auth::abrir(&raiz);
        let (token, _) = outro.entrar("vini", "outra-senha-longa").unwrap();
        assert_eq!(definir_senha(&raiz, "vini", "terceira-senha-longa"), Ok(false));
        assert_eq!(outro.usuario(&token), None);
        // remover o usuário também, e recriá-lo não as ressuscita
        let (token, _) = outro.entrar("vini", "terceira-senha-longa").unwrap();
        assert_eq!(remover_usuario(&raiz, "vini"), Ok(true));
        assert_eq!(outro.usuario(&token), None);
        assert_eq!(definir_senha(&raiz, "vini", "terceira-senha-longa"), Ok(true));
        assert_eq!(outro.usuario(&token), None);
        fs::remove_dir_all(&raiz).unwrap();
    }
}
