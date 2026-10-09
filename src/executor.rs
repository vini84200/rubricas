//! `rubricas executor`: busca no servidor o que está pendente, roda as checagens
//! do pipeline em contêineres (`podman run --network=none`) e devolve os
//! resultados. O servidor nunca executa código de aluno; quem executa é isto,
//! no notebook ou numa máquina à parte.

use crate::conferir::{self, Execucao};
use crate::fila::{ResultadoPlagio, Tarefa};
use crate::pipeline::{Checagem, Pipeline, Relatorio, ResultadoChecagem, Situacao, TipoChecagem};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// Roda os casos dentro do contêiner, cada um com seu limite de tempo, e
/// imprime o que cada execução produziu.
const EXECUTA_CASOS: &str = r#"
import json, subprocess, sys
programa, tempo = sys.argv[1], float(sys.argv[2])
casos = json.load(open('/meta/casos.json', encoding='utf-8'))
fim = lambda b: (b or b'').decode('utf-8', 'replace')[-20000:]
saida = []
for c in casos:
    try:
        p = subprocess.run([sys.executable, programa, *c['argumentos']], input=c['entrada'].encode(),
                           capture_output=True, timeout=tempo, cwd='/trabalho')
        saida.append({'saida': fim(p.stdout), 'erro': fim(p.stderr),
                      'codigo': p.returncode if p.returncode >= 0 else None, 'tempo_esgotado': False})
    except subprocess.TimeoutExpired as e:
        saida.append({'saida': fim(e.stdout), 'erro': fim(e.stderr), 'codigo': None, 'tempo_esgotado': True})
print('\n@@rubricas@@' + json.dumps(saida))
"#;

const JPLAG: &str = "localhost/rubricas-jplag:6.3.0-java25";
const RECEITA_JPLAG: &str = "FROM docker.io/library/eclipse-temurin:25-jre\n\
ADD https://github.com/jplag/JPlag/releases/download/v6.3.0/jplag-6.3.0-jar-with-dependencies.jar /opt/jplag.jar\n";

pub struct Opcoes {
    pub servidor: String,
    pub token: String,
    pub continuo: bool,
}

// ---------------------------------------------------------------------------
// Servidor
// ---------------------------------------------------------------------------

struct Cliente {
    agente: ureq::Agent,
    base: String,
    autorizacao: String,
}

fn codificar(parte: &str) -> String {
    parte
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `/a/b c` com cada parte escapada; `/` dentro de caminhos de arquivo continua `/`.
fn rota(partes: &[&str]) -> String {
    partes.iter().map(|p| p.split('/').map(codificar).collect::<Vec<_>>().join("/")).map(|p| format!("/{p}")).collect()
}

impl Cliente {
    fn novo(servidor: &str, token: &str) -> Self {
        let agente = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .build()
            .into();
        Cliente { agente, base: format!("{}/api", servidor.trim_end_matches('/')), autorizacao: format!("Bearer {token}") }
    }

    fn checar(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let mut r = r.map_err(|e| format!("sem conexão com o servidor: {e}"))?;
        if r.status().is_success() {
            return Ok(r);
        }
        let corpo = r.body_mut().read_to_string().unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&corpo).ok().and_then(|v| v["erro"].as_str().map(String::from)).unwrap_or(corpo);
        Err(format!("o servidor respondeu {}: {msg}", r.status()))
    }

    fn get(&self, caminho: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
        Self::checar(self.agente.get(format!("{}{caminho}", self.base)).header("Authorization", &self.autorizacao).call())
    }

    fn json<T: serde::de::DeserializeOwned>(&self, caminho: &str) -> Result<T, String> {
        self.get(caminho)?.body_mut().with_config().limit(64 << 20).read_json().map_err(|e| format!("{caminho}: {e}"))
    }

    fn bytes(&self, caminho: &str) -> Result<Vec<u8>, String> {
        self.get(caminho)?.body_mut().with_config().limit(400 << 20).read_to_vec().map_err(|e| format!("{caminho}: {e}"))
    }

    fn put_json(&self, caminho: &str, corpo: &impl serde::Serialize) -> Result<(), String> {
        let r = self.agente.put(format!("{}{caminho}", self.base)).header("Authorization", &self.autorizacao).send_json(corpo);
        Self::checar(r).map(|_| ())
    }

    fn put_bytes(&self, caminho: &str, corpo: &[u8]) -> Result<(), String> {
        let r = self.agente.put(format!("{}{caminho}", self.base)).header("Authorization", &self.autorizacao).send(corpo);
        Self::checar(r).map(|_| ())
    }

    /// Espera um evento `fila` do servidor (ou `limite`). Os batimentos do
    /// servidor chegam a cada 30 s, então a conexão nunca fica muda.
    fn esperar_evento(&self, limite: Duration) -> Result<(), String> {
        let inicio = Instant::now();
        let mut r = self.get("/executor/eventos")?;
        let leitor = BufReader::new(r.body_mut().as_reader());
        for linha in leitor.lines() {
            let linha = linha.map_err(|e| format!("conexão de eventos caiu: {e}"))?;
            if linha.trim() == "event: fila" || inicio.elapsed() >= limite {
                return Ok(());
            }
        }
        Err("o servidor encerrou a conexão de eventos".into())
    }
}

#[derive(Deserialize)]
struct Fila {
    tarefas: Vec<Tarefa>,
}

#[derive(Deserialize)]
struct InfoPipeline {
    pipeline: Option<Pipeline>,
    #[serde(default)]
    erro: Option<String>,
    #[serde(default)]
    anexos: Vec<InfoArquivo>,
}

#[derive(Deserialize)]
struct InfoArquivo {
    caminho: String,
}

#[derive(Deserialize)]
struct SubmissaoAluno {
    #[serde(default)]
    arquivos: Vec<InfoArquivo>,
}

#[derive(Deserialize)]
struct TodasSubmissoes {
    alunos: std::collections::BTreeMap<String, SubmissaoAluno>,
}

// ---------------------------------------------------------------------------
// Laço principal
// ---------------------------------------------------------------------------

pub fn rodar(op: Opcoes) -> Result<(), String> {
    podman(&["--version"]).map_err(|e| format!("podman não encontrado: {e}"))?;
    let cliente = Cliente::novo(&op.servidor, &op.token);
    // Tarefas que já foram tentadas nesta rodada e voltaram com erro de execução:
    // não adianta repetir até algo mudar.
    let mut tentadas: HashSet<String> = HashSet::new();
    loop {
        let fila: Fila = cliente.json("/executor/fila")?;
        let novas: Vec<Tarefa> = fila.tarefas.into_iter().filter(|t| !tentadas.contains(&chave(t))).collect();
        if novas.is_empty() {
            if !op.continuo {
                println!("Fila vazia.");
                return Ok(());
            }
            println!("Fila vazia. Esperando novidades do servidor…");
            while let Err(e) = cliente.esperar_evento(Duration::from_secs(600)) {
                eprintln!("{e}. Tentando de novo em 10 s.");
                thread::sleep(Duration::from_secs(10));
            }
            tentadas.clear();
            continue;
        }
        for t in novas {
            tentadas.insert(chave(&t));
            let r = match &t {
                Tarefa::Checagens { avaliacao, aluno, versao } => checagens(&cliente, avaliacao, aluno, versao),
                Tarefa::Plagio { avaliacao, versao } => plagio(&cliente, avaliacao, versao),
            };
            if let Err(e) = r {
                eprintln!("  erro: {e}");
            }
        }
    }
}

fn chave(t: &Tarefa) -> String {
    match t {
        Tarefa::Checagens { avaliacao, aluno, versao } => format!("{avaliacao}\n{aluno}\n{versao}"),
        Tarefa::Plagio { avaliacao, versao } => format!("{avaliacao}\n\n{versao}"),
    }
}

/// Pasta temporária que se apaga sozinha.
struct Temporaria(PathBuf);

impl Temporaria {
    fn nova(nome: &str) -> Result<Self, String> {
        let p = std::env::temp_dir().join(format!("rubricas-{nome}-{}-{}", std::process::id(), crate::auth::agora_ms()));
        fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(Temporaria(p))
    }
}

impl Drop for Temporaria {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn gravar(caminho: &Path, bytes: &[u8]) -> Result<(), String> {
    crate::dados::gravar(caminho, bytes).map_err(|e| format!("{}: {e}", caminho.display()))
}

fn baixar_pipeline(cliente: &Cliente, avaliacao: &str, destino: &Path) -> Result<Pipeline, String> {
    let info: InfoPipeline = cliente.json(&rota(&["pipelines", avaliacao]))?;
    let p = info.pipeline.ok_or_else(|| info.erro.unwrap_or_else(|| "a avaliação não tem pipeline".into()))?;
    for a in &info.anexos {
        gravar(&destino.join(&a.caminho), &cliente.bytes(&rota(&["pipelines", avaliacao, &a.caminho]))?)?;
    }
    Ok(p)
}

/// `original`: como o aluno enviou (plágio); senão, com as edições do professor (checagens).
fn baixar_submissao(
    cliente: &Cliente,
    avaliacao: &str,
    aluno: &str,
    arquivos: &[InfoArquivo],
    destino: &Path,
    original: bool,
) -> Result<Vec<String>, String> {
    let mut nomes = Vec::new();
    for a in arquivos {
        let caminho = rota(&["avaliacoes", avaliacao, "submissoes", aluno, &a.caminho]);
        let bytes = cliente.bytes(&if original { format!("{caminho}?original=1") } else { caminho })?;
        gravar(&destino.join(&a.caminho), &bytes)?;
        nomes.push(a.caminho.clone());
    }
    Ok(nomes)
}

// ---------------------------------------------------------------------------
// Checagens de um aluno
// ---------------------------------------------------------------------------

fn checagens(cliente: &Cliente, avaliacao: &str, aluno: &str, versao: &str) -> Result<(), String> {
    println!("{avaliacao} / {aluno}");
    let tmp = Temporaria::nova("checagem")?;
    let (anexos, trabalho, meta) = (tmp.0.join("pipeline"), tmp.0.join("trabalho"), tmp.0.join("meta"));
    let p = baixar_pipeline(cliente, avaliacao, &anexos)?;
    let sub: SubmissaoAluno = cliente.json(&rota(&["avaliacoes", avaliacao, "submissoes", aluno]))?;
    let arquivos = baixar_submissao(cliente, avaliacao, aluno, &sub.arquivos, &trabalho, false)?;
    // Os anexos (testes, esqueleto) ficam junto do código do aluno.
    copiar_pasta(&anexos, &trabalho)?;
    let _ = fs::remove_file(trabalho.join("pipeline.toml"));
    gravar(&meta.join("casos.py"), EXECUTA_CASOS.as_bytes())?;

    let programa = conferir::escolher_arquivo(&arquivos, &p.arquivo).and_then(|escolhido| {
        if p.arquivo.is_empty() || escolhido == p.arquivo {
            return Ok(escolhido);
        }
        fs::copy(trabalho.join(&escolhido), trabalho.join(&p.arquivo)).map_err(|e| e.to_string())?;
        Ok(p.arquivo.clone())
    });

    let imagem = match imagem_ferramentas(&p.imagem) {
        Ok(i) => i,
        Err(e) => {
            // Sem imagem não há como rodar: o relatório diz isso, e o aluno continua pendente.
            let resultados = p.checagens.iter().map(|ch| erro_execucao(ch, e.clone())).collect();
            let relatorio = Relatorio { versao: versao.into(), gerado_em: 0, checagens: resultados, sugestoes: vec![] };
            cliente.put_json(&rota(&["executor", "relatorios", avaliacao, aluno]), &relatorio)?;
            return Err(e);
        }
    };
    let mut resultados = Vec::new();
    for ch in &p.checagens {
        let r = match &programa {
            Err(motivo) => ResultadoChecagem {
                nome: ch.nome.clone(),
                tipo: ch.tipo,
                situacao: Situacao::Falhou,
                passaram: 0,
                total: if ch.tipo == TipoChecagem::Casos { ch.casos.len() as u32 } else { 1 },
                resumo: format!("não rodou: {motivo}"),
                casos: vec![],
                saida: String::new(),
            },
            Ok(programa) => {
                let ambiente = Ambiente {
                    imagem: ch.imagem.clone().map(nome_completo).unwrap_or_else(|| imagem.clone()),
                    memoria: ch.memoria.clone().unwrap_or_else(|| p.memoria.clone()),
                    cpus: p.cpus,
                    trabalho: &trabalho,
                    meta: &meta,
                };
                rodar_checagem(ch, &p, programa, &ambiente)
            }
        };
        println!("  {}: {}", r.nome, if r.situacao == Situacao::Erro { format!("erro de execução: {}", r.resumo) } else { r.resumo.clone() });
        resultados.push(r);
    }
    let relatorio = Relatorio { versao: versao.into(), gerado_em: 0, checagens: resultados, sugestoes: vec![] };
    cliente.put_json(&rota(&["executor", "relatorios", avaliacao, aluno]), &relatorio)
}

fn copiar_pasta(de: &Path, para: &Path) -> Result<(), String> {
    let Ok(entradas) = fs::read_dir(de) else { return Ok(()) };
    for e in entradas.flatten() {
        let (origem, destino) = (e.path(), para.join(e.file_name()));
        if origem.is_dir() {
            copiar_pasta(&origem, &destino)?;
        } else {
            gravar(&destino, &fs::read(&origem).map_err(|e| e.to_string())?)?;
        }
    }
    Ok(())
}

struct Ambiente<'a> {
    imagem: String,
    memoria: String,
    cpus: f64,
    trabalho: &'a Path,
    meta: &'a Path,
}

fn erro_execucao(ch: &Checagem, msg: String) -> ResultadoChecagem {
    ResultadoChecagem {
        nome: ch.nome.clone(),
        tipo: ch.tipo,
        situacao: Situacao::Erro,
        passaram: 0,
        total: 0,
        resumo: msg.lines().next().unwrap_or("").chars().take(200).collect(),
        casos: vec![],
        saida: conferir::fim(&msg),
    }
}

fn rodar_checagem(ch: &Checagem, p: &Pipeline, programa: &str, amb: &Ambiente) -> ResultadoChecagem {
    // Nos casos, o tempo vale por execução; nas outras checagens, para a checagem
    // inteira, mais uma folga para o contêiner subir.
    let tempo = ch.tempo.unwrap_or(p.tempo);
    let limite = Duration::from_secs_f64(tempo + 10.0);
    let (passou, falhou) = (Situacao::Passou, Situacao::Falhou);
    match ch.tipo {
        TipoChecagem::Casos => {
            let casos: Vec<Value> = ch.casos.iter().map(|c| json!({ "entrada": c.entrada, "argumentos": c.argumentos })).collect();
            if let Err(e) = gravar(&amb.meta.join("casos.json"), serde_json::to_string(&casos).unwrap().as_bytes()) {
                return erro_execucao(ch, e);
            }
            let limite_casos = Duration::from_secs_f64(tempo * ch.casos.len() as f64 + 30.0);
            let cmd = ["python3", "/meta/casos.py", programa, &tempo.to_string()];
            let saida = match conteiner(amb, &cmd, limite_casos, &[]) {
                Ok(s) => s,
                Err(e) => return erro_execucao(ch, e),
            };
            let execucoes = saida.saida.rsplit_once("@@rubricas@@").and_then(|(_, j)| serde_json::from_str::<Vec<ExecucaoJson>>(j.trim()).ok());
            match execucoes {
                Some(ex) => {
                    let ex: Vec<Execucao> = ex.into_iter().map(Into::into).collect();
                    conferir::julgar_casos(&ch.nome, &ch.casos, &ex, ch.ignorar_maiusculas)
                }
                // O contêiner inteiro estourou (memória, tempo total): todos os casos falham.
                None if saida.tempo_esgotado || saida.codigo == Some(137) => {
                    let motivo = if saida.tempo_esgotado { "tempo esgotado" } else { "memória esgotada" };
                    let ex = vec![Execucao { erro: motivo.into(), tempo_esgotado: saida.tempo_esgotado, ..Default::default() }; ch.casos.len()];
                    conferir::julgar_casos(&ch.nome, &ch.casos, &ex, ch.ignorar_maiusculas)
                }
                None => erro_execucao(ch, format!("o executor de casos não respondeu:\n{}{}", saida.saida, saida.erro)),
            }
        }
        TipoChecagem::Pytest => {
            let cmd = ["python3", "-m", "pytest", "-q", "-p", "no:cacheprovider", "-rfE", "--tb=short", &ch.testes];
            let s = match conteiner(amb, &cmd, limite, &[("PROGRAMA", programa)]) {
                Ok(s) => s,
                Err(e) => return erro_execucao(ch, e),
            };
            let texto = format!("{}{}", s.saida, s.erro);
            let (passaram, total) = conferir::resumo_pytest(&s.saida).unwrap_or((0, 0));
            let resumo = if s.tempo_esgotado {
                "tempo esgotado".to_string()
            } else if total == 0 {
                "nenhum teste rodou".to_string()
            } else {
                format!("{passaram} de {total} {}", if total == 1 { "teste" } else { "testes" })
            };
            let ok = total > 0 && passaram == total && !s.tempo_esgotado;
            ResultadoChecagem {
                nome: ch.nome.clone(),
                tipo: ch.tipo,
                situacao: if ok { passou } else { falhou },
                passaram,
                // Sem nenhum teste rodado (o arquivo do aluno nem importa), conta como uma falha.
                total: total.max(1),
                resumo,
                casos: vec![],
                saida: conferir::fim(&texto),
            }
        }
        TipoChecagem::Estilo => {
            let mut script = String::from("ruff check --no-cache --output-format=concise");
            if !ch.regras.is_empty() {
                script += &format!(" --select {}", aspas(&ch.regras.join(",")));
            }
            if !ch.ignorar.is_empty() {
                script += &format!(" --ignore {}", aspas(&ch.ignorar.join(",")));
            }
            script += &format!(" {}", aspas(programa));
            if ch.formato {
                script += &format!("; ruff format --no-cache --check --diff {} >/meta/formato 2>&1 || echo FORMATO", aspas(programa));
            }
            let s = match conteiner(amb, &["sh", "-c", &script], limite, &[]) {
                Ok(s) => s,
                Err(e) => return erro_execucao(ch, e),
            };
            let Some(mut problemas) = conferir::problemas_ruff(&s.saida) else {
                return erro_execucao(ch, format!("não entendi a saída do ruff:\n{}{}", s.saida, s.erro));
            };
            let formato = s.saida.lines().any(|l| l.trim() == "FORMATO");
            if formato {
                problemas += 1;
            }
            let resumo = match (problemas, formato) {
                (0, _) => "sem problemas de estilo".to_string(),
                (1, true) => "formatação fora do padrão".to_string(),
                (n, true) => format!("{} problemas de estilo e formatação fora do padrão", n - 1),
                (1, false) => "1 problema de estilo".to_string(),
                (n, false) => format!("{n} problemas de estilo"),
            };
            ResultadoChecagem {
                nome: ch.nome.clone(),
                tipo: ch.tipo,
                situacao: if problemas == 0 { passou } else { falhou },
                passaram: u32::from(problemas == 0),
                total: 1,
                resumo,
                casos: vec![],
                saida: conferir::fim(&s.saida.replace("FORMATO\n", "")),
            }
        }
        TipoChecagem::Comando => {
            let s = match conteiner(amb, &["sh", "-c", &ch.comando], limite, &[("PROGRAMA", programa)]) {
                Ok(s) => s,
                Err(e) => return erro_execucao(ch, e),
            };
            let ok = s.codigo == Some(0) && !s.tempo_esgotado;
            let resumo = match (s.tempo_esgotado, s.codigo) {
                (true, _) => "tempo esgotado".to_string(),
                (_, Some(0)) => "passou".to_string(),
                (_, Some(c)) => format!("terminou com código {c}"),
                (_, None) => "interrompido".to_string(),
            };
            ResultadoChecagem {
                nome: ch.nome.clone(),
                tipo: ch.tipo,
                situacao: if ok { passou } else { falhou },
                passaram: u32::from(ok),
                total: 1,
                resumo,
                casos: vec![],
                saida: conferir::fim(&format!("{}{}", s.saida, s.erro)),
            }
        }
    }
}

#[derive(Deserialize)]
struct ExecucaoJson {
    saida: String,
    erro: String,
    codigo: Option<i32>,
    tempo_esgotado: bool,
}

impl From<ExecucaoJson> for Execucao {
    fn from(e: ExecucaoJson) -> Self {
        Execucao { saida: e.saida, erro: e.erro, codigo: e.codigo, tempo_esgotado: e.tempo_esgotado }
    }
}

fn aspas(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------------------------------------------------------------------
// Plágio
// ---------------------------------------------------------------------------

fn plagio(cliente: &Cliente, avaliacao: &str, versao: &str) -> Result<(), String> {
    println!("{avaliacao}: plágio (JPlag)");
    let tmp = Temporaria::nova("plagio")?;
    let (anexos, trabalho) = (tmp.0.join("pipeline"), tmp.0.join("trabalho"));
    let p = baixar_pipeline(cliente, avaliacao, &anexos)?;
    let cfg = p.plagio.clone().ok_or("o pipeline não pede plágio")?;
    let todas: TodasSubmissoes = cliente.json(&rota(&["avaliacoes", avaliacao, "submissoes"]))?;
    for (aluno, s) in &todas.alunos {
        baixar_submissao(cliente, avaliacao, aluno, &s.arquivos, &trabalho.join("alunos").join(aluno), true)?;
    }
    fs::create_dir_all(trabalho.join("saida")).map_err(|e| e.to_string())?;
    let mut cmd: Vec<String> =
        ["java", "-jar", "/opt/jplag.jar", "-l", "python3", "-M", "RUN", "--csv-export", "--overwrite", "-r", "/trabalho/saida/resultado"]
            .map(String::from)
            .into();
    if !cfg.base.is_empty() {
        // O JPlag quer o código-base numa pasta.
        let origem = anexos.join(cfg.base.trim_end_matches('/'));
        let destino = trabalho.join("base");
        if origem.is_dir() {
            copiar_pasta(&origem, &destino)?;
        } else {
            gravar(&destino.join(origem.file_name().unwrap_or_default()), &fs::read(&origem).map_err(|e| format!("{}: {e}", cfg.base))?)?;
        }
        cmd.extend(["-bc".into(), "/trabalho/base".into()]);
    }
    if !cfg.referencias.is_empty() {
        copiar_pasta(&anexos.join(cfg.referencias.trim_end_matches('/')), &trabalho.join("referencias"))?;
        cmd.extend(["-old".into(), "/trabalho/referencias".into()]);
    }
    cmd.push("/trabalho/alunos".into());

    let resultado = (|| {
        garantir_imagem(JPLAG, RECEITA_JPLAG)?;
        let amb = Ambiente { imagem: JPLAG.into(), memoria: "1g".into(), cpus: p.cpus.max(1.0), trabalho: &trabalho, meta: &tmp.0 };
        let cmd: Vec<&str> = cmd.iter().map(String::as_str).collect();
        let s = conteiner(&amb, &cmd, Duration::from_secs(600), &[("JAVA_TOOL_OPTIONS", "-Xmx768m")])?;
        let csv = fs::read_to_string(trabalho.join("saida/resultado/results.csv"))
            .map_err(|_| format!("o JPlag não gerou resultado:\n{}", conferir::fim(&format!("{}{}", s.saida, s.erro))))?;
        let pares = conferir::pares_jplag(&csv)?;
        let jplag = fs::read(trabalho.join("saida/resultado.jplag")).map_err(|e| format!("resultado.jplag: {e}"))?;
        Ok::<_, String>((pares, jplag))
    })();
    match resultado {
        Ok((pares, jplag)) => {
            println!("  {} pares comparados", pares.len());
            cliente.put_bytes(&rota(&["executor", "plagio", avaliacao, "jplag"]), &jplag)?;
            cliente.put_json(&rota(&["executor", "plagio", avaliacao]), &ResultadoPlagio { versao: versao.into(), gerado_em: 0, pares, erro: String::new() })
        }
        Err(e) => {
            cliente.put_json(&rota(&["executor", "plagio", avaliacao]), &ResultadoPlagio { versao: versao.into(), gerado_em: 0, pares: vec![], erro: e.clone() })?;
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Podman
// ---------------------------------------------------------------------------

fn podman(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("podman").args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())
}

/// `python:3.12-slim` → `docker.io/library/python:3.12-slim`: sem o registro,
/// o podman pode querer perguntar de onde baixar.
fn nome_completo(imagem: String) -> String {
    match imagem.split_once('/') {
        None => format!("docker.io/library/{imagem}"),
        Some((registro, _)) if !registro.contains(['.', ':']) && registro != "localhost" => format!("docker.io/{imagem}"),
        _ => imagem,
    }
}

/// A imagem do pipeline com pytest e ruff, construída uma vez.
fn imagem_ferramentas(base: &str) -> Result<String, String> {
    let nome = format!("localhost/rubricas-python:{}", crate::modelo::slug(base));
    garantir_imagem(&nome, &format!("FROM {}\nRUN pip install --no-cache-dir pytest ruff\n", nome_completo(base.into())))?;
    Ok(nome)
}

fn garantir_imagem(nome: &str, containerfile: &str) -> Result<(), String> {
    if podman(&["image", "exists", nome])?.status.success() {
        return Ok(());
    }
    println!("Construindo a imagem {nome} (só desta vez)…");
    let tmp = Temporaria::nova("imagem")?;
    gravar(&tmp.0.join("Containerfile"), containerfile.as_bytes())?;
    let dir = tmp.0.to_string_lossy();
    let s = Command::new("podman").args(["build", "-t", nome, &dir]).status().map_err(|e| e.to_string())?;
    if s.success() { Ok(()) } else { Err(format!("não foi possível construir a imagem {nome}")) }
}

struct Saida {
    saida: String,
    erro: String,
    codigo: Option<i32>,
    tempo_esgotado: bool,
}

/// Roda um comando num contêiner sem rede, com memória, CPU e tempo limitados.
/// `Err` é falha do próprio executor (podman, imagem), nunca do código do aluno.
fn conteiner(amb: &Ambiente, cmd: &[&str], limite: Duration, env: &[(&str, &str)]) -> Result<Saida, String> {
    let nome = format!("rubricas-{}-{}", std::process::id(), crate::auth::agora_ms());
    let vol = |p: &Path, destino: &str, modo: &str| format!("{}:{destino}:{modo}", p.display());
    let mut args: Vec<String> = [
        "run", "--rm", "--name", &nome, "--network=none", "--pids-limit=256", "--cap-drop=ALL",
        "--security-opt=no-new-privileges", "-w", "/trabalho", "-e", "PYTHONDONTWRITEBYTECODE=1", "-e", "PYTHONIOENCODING=utf-8",
    ]
    .map(String::from)
    .into();
    args.extend([format!("--memory={}", amb.memoria), format!("--memory-swap={}", amb.memoria), format!("--cpus={}", amb.cpus)]);
    args.extend(["-v".into(), vol(amb.trabalho, "/trabalho", "Z"), "-v".into(), vol(amb.meta, "/meta", "z")]);
    for (k, v) in env {
        args.extend(["-e".into(), format!("{k}={v}")]);
    }
    args.push(amb.imagem.clone());
    args.extend(cmd.iter().map(|s| s.to_string()));

    let mut filho = Command::new("podman")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("podman: {e}"))?;
    // Lê a saída enquanto espera, para um programa que imprime muito não travar no pipe.
    let ler = |r: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut r) = r {
                let _ = r.by_ref().take(4 << 20).read_to_end(&mut v);
                let _ = std::io::copy(&mut r, &mut std::io::sink());
            }
            String::from_utf8_lossy(&v).into_owned()
        })
    };
    let (out, err) = (ler(filho.stdout.take().map(|x| Box::new(x) as _)), ler(filho.stderr.take().map(|x| Box::new(x) as _)));
    let inicio = Instant::now();
    let mut tempo_esgotado = false;
    let status = loop {
        if let Some(s) = filho.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        if inicio.elapsed() > limite {
            tempo_esgotado = true;
            let _ = podman(&["rm", "-f", "-t", "0", &nome]);
            let _ = filho.kill();
            break filho.wait().map_err(|e| e.to_string())?;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let (saida, erro) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    // 125: o podman não conseguiu nem criar o contêiner (imagem, opções, serviço).
    if !tempo_esgotado && matches!(status.code(), Some(125..=127)) && erro.contains("Error") {
        return Err(format!("podman: {}", erro.trim()));
    }
    Ok(Saida { saida, erro, codigo: status.code(), tempo_esgotado })
}
