//! Quem pode fazer o quê. A regra fica em Rego (`politica.rego`, a linguagem do
//! Open Policy Agent, avaliada pelo regorus); aqui ficam os acessos concedidos
//! (`acessos.toml`) e a ponte entre os dois.
//!
//! Cada usuário tem um papel global (em `usuarios.toml`) e, em cada avaliação,
//! rubrica ou turma, talvez um papel concedido por alguém. A política junta os
//! dois e responde com o conjunto de ações permitidas.

use crate::dados::{Erro, Res, Tipo, gravar};
use regorus::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::{fs, io};

pub const POLITICA: &str = include_str!("politica.rego");

/// Quem pede: nome e papel global.
#[derive(Debug, Clone, PartialEq)]
pub struct Quem {
    pub nome: String,
    pub papel: String,
}

/// Sobre o que se pede: o sistema todo (criar, gerir usuários…) ou um item.
#[derive(Debug, Clone, Copy)]
pub enum Recurso<'a> {
    Sistema,
    Item(Tipo, &'a str),
}

/// As ações que o código pede à política. Os nomes são os de `politica.rego`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acao {
    Ver,
    Exportar,
    Corrigir,
    Editar,
    Arquivar,
    Apagar,
    Compartilhar,
    LerChecagens,
    Criar,
    GerirUsuarios,
    Executar,
    VerTudo,
}

impl Acao {
    pub fn nome(self) -> &'static str {
        match self {
            Acao::Ver => "ver",
            Acao::Exportar => "exportar",
            Acao::Corrigir => "corrigir",
            Acao::Editar => "editar",
            Acao::Arquivar => "arquivar",
            Acao::Apagar => "apagar",
            Acao::Compartilhar => "compartilhar",
            Acao::LerChecagens => "ler_checagens",
            Acao::Criar => "criar",
            Acao::GerirUsuarios => "gerir_usuarios",
            Acao::Executar => "executar",
            Acao::VerTudo => "ver_tudo",
        }
    }
}

/// Um papel concedido a um usuário num item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Acesso {
    pub usuario: String,
    pub tipo: String,
    pub id: String,
    pub papel: String,
    /// Quem concedeu e quando (segundos desde a época).
    #[serde(default)]
    pub por: String,
    #[serde(default)]
    pub em: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ArquivoAcessos {
    #[serde(default)]
    acessos: Vec<Acesso>,
}

pub struct Permissoes {
    arquivo: PathBuf,
    /// O regorus avalia com estado (a entrada) dentro do motor: um pedido por vez.
    motor: Mutex<Engine>,
    papeis: Value,
    papeis_globais: Vec<String>,
    papel_global_padrao: Option<String>,
    /// Serializa as gravações em `acessos.toml`.
    trava: Mutex<()>,
}

/// Os acessos lidos de uma vez, para decidir sobre muitos itens (uma lista) sem
/// reler o arquivo a cada um.
pub struct Visao<'a> {
    permissoes: &'a Permissoes,
    quem: Quem,
    acessos: Vec<Acesso>,
}

impl Permissoes {
    /// Usa `politica.rego` da pasta de dados, se existir; senão, a embutida.
    pub fn abrir(raiz: &Path) -> Result<Self, String> {
        let propria = raiz.join("politica.rego");
        let texto = match fs::read_to_string(&propria) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => POLITICA.to_string(),
            Err(e) => return Err(format!("{}: {e}", propria.display())),
        };
        Self::com_politica(raiz, &texto).map_err(|e| format!("{}: {e}", propria.display()))
    }

    pub fn com_politica(raiz: &Path, politica: &str) -> Result<Self, String> {
        let mut motor = Engine::new();
        motor.add_policy("politica.rego".into(), politica.into()).map_err(|e| e.to_string())?;
        let mut p = Permissoes {
            arquivo: raiz.join("acessos.toml"),
            motor: Mutex::new(motor),
            papeis: Value::Null,
            papeis_globais: Vec::new(),
            papel_global_padrao: None,
            trava: Mutex::new(()),
        };
        p.papeis = p.regra("data.rubricas.papeis")?;
        p.papeis_globais = serde_json::from_value(p.regra("data.rubricas.papeis_globais")?)
            .map_err(|e| format!("papeis_globais: {e}"))?;
        let todos_tem_papel = |l: &Value| l.as_array().is_some_and(|l| !l.is_empty());
        if !p.papeis.as_object().is_some_and(|o| o.values().all(todos_tem_papel)) || p.papeis_globais.is_empty() {
            return Err("a política precisa definir papeis (com ao menos um por tipo) e papeis_globais".into());
        }
        p.papel_global_padrao = p.regra("data.rubricas.papel_global_padrao").ok().and_then(|v| v.as_str().map(String::from));
        Ok(p)
    }

    fn regra(&self, regra: &str) -> Result<Value, String> {
        let v = self.motor.lock().unwrap_or_else(|e| e.into_inner()).eval_rule(regra.into()).map_err(|e| e.to_string())?;
        serde_json::from_str(&v.to_json_str().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }

    /// Papéis que podem ser concedidos em cada tipo de item, do menor ao maior.
    pub fn papeis(&self) -> &Value {
        &self.papeis
    }

    pub fn papeis_globais(&self) -> &[String] {
        &self.papeis_globais
    }

    pub fn papel_valido(&self, tipo: Tipo, papel: &str) -> bool {
        self.ordem(tipo, papel).is_some()
    }

    /// Posição do papel na lista do tipo, do menor ao maior.
    fn ordem(&self, tipo: Tipo, papel: &str) -> Option<usize> {
        self.papeis[tipo.nome()].as_array()?.iter().position(|p| p == papel)
    }

    /// O maior papel do tipo: o de quem cria o item. Todo item fica com ao menos um.
    pub fn papel_de_quem_cria(&self, tipo: Tipo) -> &str {
        self.papeis[tipo.nome()].as_array().and_then(|l| l.last()?.as_str()).unwrap_or_default()
    }

    /// Papel de um usuário novo criado sem papel dito, se a política define um.
    pub fn papel_global_padrao(&self) -> Option<&str> {
        self.papel_global_padrao.as_deref()
    }

    /// O papel global pode gerir usuários? O sistema nunca fica sem alguém assim.
    pub fn gere_usuarios(&self, papel: &str) -> bool {
        self.avaliar(&Quem { nome: String::new(), papel: papel.into() }, Recurso::Sistema, None).contains(Acao::GerirUsuarios.nome())
    }

    fn ler(&self) -> Vec<Acesso> {
        match fs::read_to_string(&self.arquivo) {
            Ok(t) => match toml::from_str::<ArquivoAcessos>(&t) {
                Ok(a) => a.acessos,
                Err(e) => {
                    // Na dúvida, ninguém além do admin tem acesso a nada.
                    eprintln!("{}: {e}", self.arquivo.display());
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        }
    }

    fn alterar<T>(&self, f: impl FnOnce(&mut Vec<Acesso>) -> Res<T>) -> Res<T> {
        let _t = self.trava.lock().unwrap_or_else(|e| e.into_inner());
        let mut acessos = match fs::read_to_string(&self.arquivo) {
            Ok(t) => toml::from_str::<ArquivoAcessos>(&t).map_err(|e| Erro::Invalido(format!("acessos.toml: {}", e.message())))?.acessos,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let r = f(&mut acessos)?;
        let texto = toml::to_string_pretty(&ArquivoAcessos { acessos }).map_err(|e| Erro::Invalido(e.to_string()))?;
        gravar(&self.arquivo, texto.as_bytes())?;
        Ok(r)
    }

    /// Recusa a mudança que deixaria o item sem ninguém no papel de quem cria.
    fn checar_resta_dono(&self, tipo: Tipo, id: &str, antes: &[Acesso], depois: &[Acesso]) -> Res<()> {
        let dono = self.papel_de_quem_cria(tipo);
        let tem_dono = |l: &[Acesso]| l.iter().any(|a| a.tipo == tipo.nome() && a.id == id && a.papel == dono);
        if tem_dono(antes) && !tem_dono(depois) {
            return Err(Erro::Conflito(format!("É preciso ficar pelo menos um {dono}. Dê o papel de {dono} a outra pessoa antes.")));
        }
        Ok(())
    }

    /// Muda os acessos de um item, sem deixá-lo sem dono.
    fn alterar_item<T>(&self, tipo: Tipo, id: &str, f: impl FnOnce(&mut Vec<Acesso>) -> T) -> Res<T> {
        self.alterar(|acessos| {
            let antes = acessos.clone();
            let r = f(acessos);
            self.checar_resta_dono(tipo, id, &antes, acessos)?;
            Ok(r)
        })
    }

    pub fn visao(&self, quem: &Quem) -> Visao<'_> {
        Visao { permissoes: self, quem: quem.clone(), acessos: self.ler() }
    }

    fn avaliar(&self, quem: &Quem, recurso: Recurso, acesso: Option<&str>) -> BTreeSet<String> {
        let (tipo, id) = match recurso {
            Recurso::Sistema => ("sistema", ""),
            Recurso::Item(t, id) => (t.nome(), id),
        };
        let entrada = json!({
            "usuario": { "nome": quem.nome, "papel": quem.papel },
            "recurso": { "tipo": tipo, "id": id },
            "acesso": acesso,
        });
        let mut motor = self.motor.lock().unwrap_or_else(|e| e.into_inner());
        let resultado = motor
            .set_input_json(&entrada.to_string())
            .and_then(|_| motor.eval_rule("data.rubricas.acoes".into()))
            .and_then(|v| v.to_json_str());
        match resultado.map(|t| serde_json::from_str::<BTreeSet<String>>(&t)) {
            Ok(Ok(acoes)) => acoes,
            // Erro na política nega: melhor um 403 do que liberar sem querer.
            erro => {
                eprintln!("política: {erro:?}");
                BTreeSet::new()
            }
        }
    }

    /// Acessos concedidos num item, do maior papel ao menor.
    pub fn acessos(&self, tipo: Tipo, id: &str) -> Vec<Acesso> {
        let ordem = |p: &str| self.ordem(tipo, p).unwrap_or(0);
        let mut v: Vec<Acesso> = self.ler().into_iter().filter(|a| a.tipo == tipo.nome() && a.id == id).collect();
        v.sort_by(|a, b| ordem(&b.papel).cmp(&ordem(&a.papel)).then(a.usuario.cmp(&b.usuario)));
        v
    }

    /// Concede (ou troca) o papel do usuário no item.
    pub fn conceder(&self, tipo: Tipo, id: &str, usuario: &str, papel: &str, por: &str, em: u64) -> Res<()> {
        if !self.papel_valido(tipo, papel) {
            return Err(Erro::Invalido(format!("Papel desconhecido para {}: {papel}", tipo.nome())));
        }
        let novo = Acesso { usuario: usuario.into(), tipo: tipo.nome().into(), id: id.into(), papel: papel.into(), por: por.into(), em };
        self.alterar_item(tipo, id, |acessos| {
            acessos.retain(|a| !(a.tipo == novo.tipo && a.id == novo.id && a.usuario.eq_ignore_ascii_case(&novo.usuario)));
            acessos.push(novo);
        })
    }

    /// O item acabou de ser criado: quem criou fica com o maior papel, e acessos
    /// que sobraram de um item apagado com o mesmo id não valem para este.
    pub fn nascer(&self, tipo: Tipo, id: &str, quem: &str, em: u64) -> Res<()> {
        let dono = Acesso { usuario: quem.into(), tipo: tipo.nome().into(), id: id.into(), papel: self.papel_de_quem_cria(tipo).into(), por: quem.into(), em };
        self.alterar(|acessos| {
            acessos.retain(|a| !(a.tipo == dono.tipo && a.id == dono.id));
            acessos.push(dono);
            Ok(())
        })
    }

    /// Tira o acesso do usuário ao item. `false` se ele não tinha.
    pub fn revogar(&self, tipo: Tipo, id: &str, usuario: &str) -> Res<bool> {
        self.alterar_item(tipo, id, |acessos| {
            let antes = acessos.len();
            acessos.retain(|a| !(a.tipo == tipo.nome() && a.id == id && a.usuario.eq_ignore_ascii_case(usuario)));
            acessos.len() != antes
        })
    }

    /// O item foi apagado: os acessos a ele também vão embora. (Se isto falhar,
    /// `nascer` limpa o que sobrou quando outro item tomar o mesmo id.)
    pub fn esquecer_item(&self, tipo: Tipo, id: &str) -> Res<()> {
        self.alterar(|acessos| {
            acessos.retain(|a| !(a.tipo == tipo.nome() && a.id == id));
            Ok(())
        })
    }

    /// O usuário foi removido: os acessos dele também, para não valerem para outro
    /// usuário criado depois com o mesmo nome.
    pub fn esquecer_usuario(&self, usuario: &str) -> Res<()> {
        self.alterar(|acessos| {
            acessos.retain(|a| !a.usuario.eq_ignore_ascii_case(usuario));
            Ok(())
        })
    }
}

impl Visao<'_> {
    /// Papel do usuário no item, se lhe concederam um.
    pub fn acesso(&self, tipo: Tipo, id: &str) -> Option<&str> {
        self.acessos
            .iter()
            .find(|a| a.tipo == tipo.nome() && a.id == id && a.usuario.eq_ignore_ascii_case(&self.quem.nome))
            .map(|a| a.papel.as_str())
    }

    pub fn acoes(&self, recurso: Recurso) -> BTreeSet<String> {
        let acesso = match recurso {
            Recurso::Sistema => None,
            Recurso::Item(t, id) => self.acesso(t, id),
        };
        self.permissoes.avaliar(&self.quem, recurso, acesso)
    }

    pub fn pode(&self, recurso: Recurso, acao: Acao) -> bool {
        self.acoes(recurso).contains(acao.nome())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pasta(nome: &str) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("rubricas-perm-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        fs::create_dir_all(&raiz).unwrap();
        raiz
    }

    fn quem(nome: &str, papel: &str) -> Quem {
        Quem { nome: nome.into(), papel: papel.into() }
    }

    fn lista(acoes: BTreeSet<String>) -> Vec<String> {
        acoes.into_iter().collect()
    }

    #[test]
    fn monitor_so_faz_o_que_lhe_concederam() {
        let raiz = pasta("monitor");
        let p = Permissoes::abrir(&raiz).unwrap();
        let ana = quem("ana", "monitor");
        let prova = Recurso::Item(Tipo::Avaliacoes, "prova-1");

        assert!(p.visao(&ana).acoes(prova).is_empty());
        assert!(p.visao(&ana).acoes(Recurso::Sistema).is_empty());

        p.conceder(Tipo::Avaliacoes, "prova-1", "ana", "avaliador", "vini", 1).unwrap();
        let v = p.visao(&ana);
        assert_eq!(lista(v.acoes(prova)), ["corrigir", "exportar", "ler_checagens", "ver"]);
        assert!(v.acoes(Recurso::Item(Tipo::Avaliacoes, "prova-2")).is_empty());
        // o acesso é à avaliação, não à rubrica de mesmo nome
        assert!(v.acoes(Recurso::Item(Tipo::Rubricas, "prova-1")).is_empty());
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn papeis_globais() {
        let raiz = pasta("globais");
        let p = Permissoes::abrir(&raiz).unwrap();
        let rubrica = Recurso::Item(Tipo::Rubricas, "laços");

        let admin = p.visao(&quem("vini", "admin"));
        assert_eq!(lista(admin.acoes(rubrica)), ["apagar", "arquivar", "compartilhar", "editar", "ver"]);
        assert_eq!(lista(admin.acoes(Recurso::Sistema)), ["criar", "executar", "gerir_usuarios", "ver_tudo"]);

        // professor cria, mas só vê o que é dele ou foi compartilhado
        let prof = p.visao(&quem("bia", "professor"));
        assert_eq!(lista(prof.acoes(Recurso::Sistema)), ["criar"]);
        assert!(prof.acoes(rubrica).is_empty());

        // o executor baixa o necessário para as checagens, nada além
        let exec = p.visao(&quem("maquina", "executor"));
        assert_eq!(lista(exec.acoes(Recurso::Sistema)), ["executar"]);
        assert_eq!(lista(exec.acoes(Recurso::Item(Tipo::Avaliacoes, "p"))), ["ler_checagens"]);
        assert!(exec.acoes(rubrica).is_empty());

        // papel global desconhecido não pode nada
        assert!(p.visao(&quem("x", "deus")).acoes(Recurso::Sistema).is_empty());
        assert!(p.gere_usuarios("admin") && !p.gere_usuarios("professor"));
        assert_eq!(p.papel_de_quem_cria(Tipo::Avaliacoes), "dono");
        assert_eq!(p.papel_global_padrao(), Some("monitor"));
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn conceder_trocar_revogar_e_esquecer() {
        let raiz = pasta("conceder");
        let p = Permissoes::abrir(&raiz).unwrap();
        let ana = quem("ana", "monitor");
        let turma = Recurso::Item(Tipo::Turmas, "t1");

        assert!(p.conceder(Tipo::Turmas, "t1", "ana", "avaliador", "vini", 1).is_err(), "turma não tem avaliador");
        p.conceder(Tipo::Turmas, "t1", "ana", "leitor", "vini", 1).unwrap();
        p.conceder(Tipo::Turmas, "t1", "ANA", "editor", "vini", 2).unwrap();
        p.conceder(Tipo::Turmas, "t1", "bia", "dono", "vini", 3).unwrap();
        assert!(p.visao(&ana).pode(turma, Acao::Editar));
        // um acesso por pessoa, do maior papel para o menor
        let acessos: Vec<_> = p.acessos(Tipo::Turmas, "t1").into_iter().map(|a| (a.usuario, a.papel)).collect();
        assert_eq!(acessos, [("bia".to_string(), "dono".to_string()), ("ANA".into(), "editor".into())]);

        assert!(p.revogar(Tipo::Turmas, "t1", "ana").unwrap());
        assert!(!p.revogar(Tipo::Turmas, "t1", "ana").unwrap());
        assert!(!p.visao(&ana).pode(turma, Acao::Ver));

        // apagar o item ou o usuário leva os acessos junto
        p.conceder(Tipo::Turmas, "t1", "ana", "leitor", "vini", 4).unwrap();
        p.conceder(Tipo::Turmas, "t2", "ana", "leitor", "vini", 4).unwrap();
        p.esquecer_item(Tipo::Turmas, "t1").unwrap();
        assert!(p.acessos(Tipo::Turmas, "t1").is_empty());
        assert_eq!(p.acessos(Tipo::Turmas, "t2").len(), 1);
        p.esquecer_usuario("Ana").unwrap();
        assert!(p.acessos(Tipo::Turmas, "t2").is_empty());

        // o último dono não sai nem perde o papel; com outro dono, pode
        assert!(matches!(p.revogar(Tipo::Turmas, "t1", "bia"), Ok(false)), "t1 foi esquecida");
        p.nascer(Tipo::Turmas, "t3", "bia", 5).unwrap();
        assert!(matches!(p.revogar(Tipo::Turmas, "t3", "bia"), Err(Erro::Conflito(_))));
        assert!(matches!(p.conceder(Tipo::Turmas, "t3", "BIA", "leitor", "bia", 6), Err(Erro::Conflito(_))));
        p.conceder(Tipo::Turmas, "t3", "ana", "dono", "bia", 6).unwrap();
        p.revogar(Tipo::Turmas, "t3", "bia").unwrap();
        // um item novo com o id de um apagado não herda os acessos que sobraram
        p.conceder(Tipo::Turmas, "t4", "ana", "leitor", "vini", 7).unwrap();
        p.nascer(Tipo::Turmas, "t4", "bia", 8).unwrap();
        let acessos: Vec<_> = p.acessos(Tipo::Turmas, "t4").into_iter().map(|a| (a.usuario, a.papel)).collect();
        assert_eq!(acessos, [("bia".to_string(), "dono".to_string())]);
        fs::remove_dir_all(&raiz).unwrap();
    }

    #[test]
    fn politica_propria_na_pasta_de_dados() {
        let raiz = pasta("propria");
        // monitores passam a ver todas as rubricas
        let propria = POLITICA.replace(
            "default do_executor := set()",
            "default do_executor := set()\n\ndo_executor := {\"ver\"} if {\n\tinput.usuario.papel == \"monitor\"\n\tinput.recurso.tipo == \"rubrica\"\n}",
        );
        fs::write(raiz.join("politica.rego"), propria).unwrap();
        let p = Permissoes::abrir(&raiz).unwrap();
        assert!(p.visao(&quem("ana", "monitor")).pode(Recurso::Item(Tipo::Rubricas, "r"), Acao::Ver));

        // uma política com erro impede de abrir, em vez de liberar ou negar tudo
        fs::write(raiz.join("politica.rego"), "package rubricas\nacoes := {").unwrap();
        assert!(Permissoes::abrir(&raiz).is_err());
        fs::remove_dir_all(&raiz).unwrap();
    }
}
