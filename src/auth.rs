//! Usuários locais (senha com Argon2id em `usuarios.toml`), sessões por cookie e
//! tokens de API (para scripts e para o Claude: `Authorization: Bearer …`).
//! Cada usuário tem um papel global; o que ele pode fazer fica em `permissoes`.

use crate::dados::{Erro, Res, gravar, ler_opcional};
use crate::permissoes::Quem;
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
use std::fs;

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
    /// SHA-256 do token de API; o token em si só é mostrado ao ser criado.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    token: String,
    /// Antes das permissões todos podiam tudo: quem não tem papel gravado é admin.
    #[serde(default = "admin")]
    papel: String,
}

fn admin() -> String {
    "admin".into()
}

/// O que se mostra de um usuário: nunca o hash nem o token.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resumo {
    pub nome: String,
    pub papel: String,
    pub tem_token: bool,
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
    pub usuarios: Usuarios,
    arquivo_sessoes: PathBuf,
    /// SHA-256 do token → sessão. O token em si só existe no cookie.
    sessoes: Mutex<HashMap<String, Sessao>>,
    /// usuário → (falhas, início da janela)
    falhas: Mutex<HashMap<String, (u32, u64)>>,
}

pub fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Milissegundos desde a época, para nomes únicos.
pub fn agora_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn resumo(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// 32 bytes aleatórios em hexadecimal.
fn aleatorio() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("sem fonte de aleatoriedade");
    hex(&bytes)
}

fn gerar_hash(senha: &str) -> Result<String, String> {
    Argon2::default().hash_password(senha.as_bytes()).map(|h| h.to_string()).map_err(|e| e.to_string())
}

fn confere(senha: &str, hash: &str) -> bool {
    PasswordHash::new(hash).map(|h| Argon2::default().verify_password(senha.as_bytes(), &h).is_ok()).unwrap_or(false)
}

pub fn nome_valido(nome: &str) -> bool {
    !nome.is_empty() && nome.len() <= 40 && nome.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn hash_de_senha(senha: &str) -> Res<String> {
    if senha.chars().count() < 8 {
        return Err(Erro::Invalido("A senha precisa ter pelo menos 8 caracteres".into()));
    }
    gerar_hash(senha).map_err(Erro::Invalido)
}

fn nao_existe(nome: &str) -> Erro {
    Erro::NaoEncontrado(format!("Não existe usuário {nome}."))
}

impl ArquivoUsuarios {
    /// Nomes valem sem distinguir maiúsculas: `Vini` é `vini`.
    fn achar(&mut self, nome: &str) -> Res<&mut Usuario> {
        self.usuarios.iter_mut().find(|u| u.nome.eq_ignore_ascii_case(nome)).ok_or_else(|| nao_existe(nome))
    }
}

/// O arquivo `usuarios.toml`. Toda mudança passa por `alterar`: ler, mudar e
/// gravar sob a trava, para duas mudanças ao mesmo tempo não se sobrescreverem.
pub struct Usuarios {
    arquivo: PathBuf,
    trava: Mutex<()>,
}

impl Usuarios {
    pub fn abrir(raiz: &Path) -> Self {
        Usuarios { arquivo: raiz.join("usuarios.toml"), trava: Mutex::new(()) }
    }

    fn ler(&self) -> Res<ArquivoUsuarios> {
        match ler_opcional(&self.arquivo)? {
            None => Ok(ArquivoUsuarios::default()),
            Some(t) => toml::from_str(&t).map_err(|e| Erro::Invalido(format!("usuarios.toml: {}", e.message()))),
        }
    }

    /// `gere` diz se um papel gere usuários: a mudança é recusada se deixaria o
    /// sistema sem ninguém que gere (havendo alguém antes).
    fn alterar<T>(&self, gere: impl Fn(&str) -> bool, f: impl FnOnce(&mut ArquivoUsuarios) -> Res<T>) -> Res<T> {
        let _g = self.trava.lock().unwrap_or_else(|e| e.into_inner());
        let mut arq = self.ler()?;
        let alguem_gere = |a: &ArquivoUsuarios| a.usuarios.iter().any(|u| gere(&u.papel));
        let antes = alguem_gere(&arq);
        let r = f(&mut arq)?;
        if antes && !alguem_gere(&arq) {
            return Err(Erro::Conflito("É preciso ficar pelo menos um usuário que gere os demais (admin).".into()));
        }
        gravar(&self.arquivo, toml::to_string_pretty(&arq).map_err(|e| Erro::Invalido(e.to_string()))?.as_bytes())?;
        restringir(&self.arquivo);
        Ok(r)
    }

    pub fn listar(&self) -> Res<Vec<Resumo>> {
        let mut v: Vec<Resumo> =
            self.ler()?.usuarios.into_iter().map(|u| Resumo { tem_token: !u.token.is_empty(), nome: u.nome, papel: u.papel }).collect();
        v.sort_by_key(|u| u.nome.to_lowercase());
        Ok(v)
    }

    /// O nome como está gravado (`Vini` → `vini`), se o usuário existe.
    pub fn nome_gravado(&self, nome: &str) -> Option<String> {
        self.ler().ok()?.achar(nome).ok().map(|u| u.nome.clone())
    }

    /// Cria o usuário; recusa se o nome já existe. Lento (Argon2).
    pub fn criar(&self, nome: &str, senha: &str, papel: &str) -> Res<()> {
        if !nome_valido(nome) {
            return Err(Erro::Invalido("Use só letras sem acento, números, ponto, hífen ou sublinhado no nome".into()));
        }
        let hash = hash_de_senha(senha)?;
        self.alterar(|_| false, |arq| {
            if arq.achar(nome).is_ok() {
                return Err(Erro::Conflito(format!("Já existe o usuário {nome}.")));
            }
            arq.usuarios.push(Usuario { nome: nome.to_string(), hash, token: String::new(), papel: papel.into() });
            Ok(())
        })
    }

    /// Troca a senha de quem existe. A senha nova derruba as sessões e o token de API. Lento (Argon2).
    pub fn trocar_senha(&self, nome: &str, senha: &str) -> Res<()> {
        let hash = hash_de_senha(senha)?;
        self.alterar(|_| false, |arq| {
            let u = arq.achar(nome)?;
            u.hash = hash;
            u.token.clear();
            Ok(())
        })
    }

    /// Troca o papel global. Quem valida o papel é a política.
    pub fn definir_papel(&self, nome: &str, papel: &str, gere: impl Fn(&str) -> bool) -> Res<()> {
        self.alterar(gere, |arq| {
            arq.achar(nome)?.papel = papel.into();
            Ok(())
        })
    }

    /// Confere a senha atual de alguém (para trocar a própria senha). Lento (Argon2).
    pub fn senha_confere(&self, nome: &str, senha: &str) -> bool {
        self.ler().ok().and_then(|mut a| a.achar(nome).ok().map(|u| confere(senha, &u.hash))).unwrap_or(false)
    }

    pub fn remover(&self, nome: &str, gere: impl Fn(&str) -> bool) -> Res<()> {
        self.alterar(gere, |arq| {
            let antes = arq.usuarios.len();
            arq.usuarios.retain(|u| !u.nome.eq_ignore_ascii_case(nome));
            if arq.usuarios.len() == antes { Err(nao_existe(nome)) } else { Ok(()) }
        })
    }

    /// Cria o token de API do usuário, substituindo o anterior. Devolve o token, que
    /// não fica gravado em lugar nenhum.
    pub fn criar_token(&self, nome: &str) -> Res<String> {
        let token = aleatorio();
        self.alterar(|_| false, |arq| {
            arq.achar(nome)?.token = resumo(&token);
            Ok(())
        })?;
        Ok(token)
    }

    /// `false` se o usuário não tinha token.
    pub fn remover_token(&self, nome: &str) -> Res<bool> {
        self.alterar(|_| false, |arq| Ok(!std::mem::take(&mut arq.achar(nome)?.token).is_empty()))
    }

    fn todos(&self) -> Vec<Usuario> {
        self.ler().map(|a| a.usuarios).unwrap_or_default()
    }
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
            usuarios: Usuarios::abrir(raiz),
            arquivo_sessoes,
            sessoes: Mutex::new(sessoes),
            falhas: Mutex::new(HashMap::new()),
        }
    }

    pub fn total_usuarios(&self) -> usize {
        self.usuarios.todos().len()
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
        let todos = self.usuarios.todos();
        let conta = todos.iter().find(|u| u.nome.eq_ignore_ascii_case(&chave));
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

        let token = aleatorio();
        let nome = conta.unwrap().nome.clone();
        let marca = resumo(&conta.unwrap().hash);
        let mut sessoes = self.sessoes.lock().unwrap();
        sessoes.retain(|_, s| s.expira > t);
        sessoes.insert(resumo(&token), Sessao { usuario: nome.clone(), expira: t + DURACAO_SESSAO, marca });
        self.persistir(&sessoes);
        Ok((token, nome))
    }

    pub fn usuario(&self, token: &str) -> Option<Quem> {
        let sessoes = self.sessoes.lock().unwrap();
        let s = sessoes.get(&resumo(token))?;
        if s.expira <= agora() {
            return None;
        }
        // Usuário removido, recriado ou com senha trocada pelo terminal perde o acesso na hora.
        self.usuarios
            .todos()
            .into_iter()
            .find(|u| u.nome == s.usuario && resumo(&u.hash) == s.marca)
            .map(|u| Quem { nome: u.nome, papel: u.papel })
    }

    /// Dono de um token de API. Lido do disco a cada pedido, então criar ou
    /// remover o token pelo terminal vale na hora.
    pub fn usuario_por_token(&self, token: &str) -> Option<Quem> {
        if token.is_empty() {
            return None;
        }
        let r = resumo(token);
        self.usuarios.todos().into_iter().find(|u| u.token == r).map(|u| Quem { nome: u.nome, papel: u.papel })
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

    fn pasta(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("rubricas-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        fs::create_dir_all(&raiz).unwrap();
        raiz
    }

    fn gere(papel: &str) -> bool {
        papel == "admin"
    }

    #[test]
    fn login_sessao_e_bloqueio() {
        let raiz = pasta("auth");
        let u = Usuarios::abrir(&raiz);

        assert!(matches!(u.criar("vini", "curta", "admin"), Err(Erro::Invalido(_))));
        assert!(matches!(u.criar("nome com espaço", "senha-longa", "admin"), Err(Erro::Invalido(_))));
        u.criar("vini", "senha-longa", "admin").unwrap();
        assert!(matches!(u.criar("Vini", "senha-longa", "admin"), Err(Erro::Conflito(_))));
        u.trocar_senha("Vini", "outra-senha-longa").unwrap();
        assert!(matches!(u.trocar_senha("ninguem", "outra-senha-longa"), Err(Erro::NaoEncontrado(_))));
        let texto = fs::read_to_string(raiz.join("usuarios.toml")).unwrap();
        assert!(texto.contains("$argon2id$") && !texto.contains("outra-senha-longa"));

        let auth = Auth::abrir(&raiz);
        assert_eq!(auth.entrar("vini", "senha-longa"), Err(ErroLogin::Credenciais));
        assert_eq!(auth.entrar("ninguem", "outra-senha-longa"), Err(ErroLogin::Credenciais));
        let (token, nome) = auth.entrar("VINI", "outra-senha-longa").unwrap();
        assert_eq!(nome, "vini");
        assert_eq!(auth.usuario(&token).map(|q| q.nome).as_deref(), Some("vini"));
        assert_eq!(auth.usuario("token-inventado"), None);

        // a sessão sobrevive a reiniciar o servidor; o arquivo não guarda o token
        assert!(!fs::read_to_string(raiz.join(".sessoes.json")).unwrap().contains(&token));
        assert_eq!(Auth::abrir(&raiz).usuario(&token).map(|q| q.nome).as_deref(), Some("vini"));

        auth.sair(&token);
        assert_eq!(auth.usuario(&token), None);

        for _ in 0..MAX_FALHAS {
            assert_eq!(auth.entrar("vini", "errada"), Err(ErroLogin::Credenciais));
        }
        assert!(matches!(auth.entrar("vini", "outra-senha-longa"), Err(ErroLogin::Bloqueado(_))));

        // trocar a senha derruba as sessões abertas com a antiga
        let outro = Auth::abrir(&raiz);
        let (token, _) = outro.entrar("vini", "outra-senha-longa").unwrap();
        u.trocar_senha("vini", "terceira-senha-longa").unwrap();
        assert_eq!(outro.usuario(&token), None);
        // remover o usuário também, e recriá-lo não as ressuscita
        let (token, _) = outro.entrar("vini", "terceira-senha-longa").unwrap();
        u.remover("vini", |_| false).unwrap();
        assert_eq!(outro.usuario(&token), None);
        u.criar("vini", "terceira-senha-longa", "admin").unwrap();
        assert_eq!(outro.usuario(&token), None);
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn tokens_de_api() {
        let raiz = pasta("token");
        let auth = Auth::abrir(&raiz);
        let u = &auth.usuarios;

        assert!(u.criar_token("ninguem").is_err());
        u.criar("vini", "senha-longa", "admin").unwrap();
        let token = u.criar_token("VINI").unwrap();
        assert_eq!(auth.usuario_por_token(&token).map(|q| q.nome).as_deref(), Some("vini"));
        assert_eq!(auth.usuario_por_token("inventado"), None);
        assert_eq!(auth.usuario_por_token(""), None);
        assert!(!fs::read_to_string(raiz.join("usuarios.toml")).unwrap().contains(&token));

        // um token novo substitui o anterior
        let novo = u.criar_token("vini").unwrap();
        assert_eq!(auth.usuario_por_token(&token), None);
        assert_eq!(auth.usuario_por_token(&novo).map(|q| q.nome).as_deref(), Some("vini"));

        assert!(u.remover_token("vini").unwrap());
        assert!(!u.remover_token("vini").unwrap());
        assert_eq!(auth.usuario_por_token(&novo), None);

        // trocar a senha ou remover o usuário também derruba o token
        let t = u.criar_token("vini").unwrap();
        u.trocar_senha("vini", "outra-senha-longa").unwrap();
        assert_eq!(auth.usuario_por_token(&t), None);
        let t = u.criar_token("vini").unwrap();
        u.remover("vini", |_| false).unwrap();
        assert_eq!(auth.usuario_por_token(&t), None);
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn papel_global() {
        let raiz = pasta("papel");
        let u = Usuarios::abrir(&raiz);
        // gravado antes das permissões, sem papel: continua podendo tudo
        u.criar("vini", "senha-longa", "admin").unwrap();
        let texto = fs::read_to_string(raiz.join("usuarios.toml")).unwrap().replace("papel = \"admin\"\n", "");
        assert!(!texto.contains("papel"));
        fs::write(raiz.join("usuarios.toml"), texto).unwrap();
        let auth = Auth::abrir(&raiz);
        let t = auth.usuarios.criar_token("vini").unwrap();
        assert_eq!(auth.usuario_por_token(&t).unwrap().papel, "admin");

        // trocar o papel vale na hora, sem derrubar a sessão
        u.criar("ana", "senha-longa", "monitor").unwrap();
        let (sessao, _) = auth.entrar("ana", "senha-longa").unwrap();
        assert_eq!(auth.usuario(&sessao).unwrap().papel, "monitor");
        u.definir_papel("ANA", "professor", gere).unwrap();
        assert_eq!(auth.usuario(&sessao).unwrap().papel, "professor");
        assert_eq!(u.listar().unwrap().iter().map(|u| (u.nome.as_str(), u.papel.as_str())).collect::<Vec<_>>(), [("ana", "professor"), ("vini", "admin")]);
        assert!(u.senha_confere("ana", "senha-longa"));
        assert!(!u.senha_confere("ana", "errada"));

        // o último que gere usuários não perde o papel nem é removido
        assert!(matches!(u.definir_papel("vini", "professor", gere), Err(Erro::Conflito(_))));
        assert!(matches!(u.remover("vini", gere), Err(Erro::Conflito(_))));
        u.definir_papel("ana", "admin", gere).unwrap();
        u.remover("vini", gere).unwrap();
        assert_eq!(u.nome_gravado("ANA").as_deref(), Some("ana"));
        assert_eq!(u.nome_gravado("vini"), None);
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn mudancas_ao_mesmo_tempo_nao_se_perdem() {
        let raiz = pasta("concorrencia");
        let u = std::sync::Arc::new(Usuarios::abrir(&raiz));
        let fios: Vec<_> = (0..8)
            .map(|i| {
                let u = u.clone();
                std::thread::spawn(move || u.criar(&format!("u{i}"), "senha-longa", "monitor").unwrap())
            })
            .collect();
        fios.into_iter().for_each(|f| f.join().unwrap());
        assert_eq!(u.listar().unwrap().len(), 8);
        fs::remove_dir_all(&raiz).unwrap();
    }
}
