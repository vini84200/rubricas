//! `rubricas executor`: busca no servidor o que está pendente, roda as checagens
//! do pipeline em contêineres (`podman run --network=none`) e devolve os
//! resultados. O servidor nunca executa código de aluno; quem executa é isto,
//! no notebook ou numa máquina à parte.

use crate::conferir::{self, Execucao};
use crate::ead::{RespostaPipeline, RespostaSubmissao};
use crate::fila::{ResultadoPlagio, Tarefa};
use crate::submissoes::Submissoes;
use crate::pipeline::{Checagem, Pipeline, Relatorio, ResultadoChecagem, Situacao, TipoChecagem};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// Versão de como o executor julga. Mudou a regra (por exemplo, o script dos
/// casos abaixo)? Mude esta versão: as checagens de todos os alunos voltam para
/// a fila uma vez, e o servidor recusa relatórios de executores com a regra antiga.
pub const MOTOR: &str = "2";

/// Roda os casos dentro do contêiner, cada um com seu limite de tempo, e
/// imprime o que cada execução produziu depois de `conferir::MARCA_CASOS`. Sem `considerar_prompts`, o programa
/// roda com um `input()` que lê a linha sem imprimir o prompt: cada aluno
/// escreve o seu ("Digite os votos: "), e isso não pode reprovar a saída.
const EXECUTA_CASOS: &str = r#"
import json, subprocess, sys
programa, tempo, arquivo_casos, pasta = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4]
considerar_prompts = sys.argv[5] == '1'
casos = json.load(open(arquivo_casos, encoding='utf-8'))
SEM_PROMPTS = (
    "import builtins, os, runpy, sys, traceback\n"
    "def _so_do_aluno(t, v, tb):\n"
    "    q = traceback.extract_tb(tb)\n"
    "    while q and (q[0].filename == '<string>' or 'runpy' in q[0].filename):\n"
    "        q.pop(0)\n"
    "    if q and q[-1].filename == '<string>' and q[-1].name == '<lambda>':\n"
    "        q.pop()\n"
    "    # o código do -c fica no cache de linhas: num exec() do aluno, sairia a linha errada\n"
    "    q = [traceback.FrameSummary(f.filename, f.lineno, f.name, line='') if f.filename == '<string>' else f for f in q]\n"
    "    cabeca = ['Traceback (most recent call last):\\n', *traceback.format_list(q)] if q else []\n"
    "    sys.stderr.write(''.join(cabeca + traceback.format_exception_only(t, v)))\n"
    "sys.excepthook = _so_do_aluno\n"
    "_ler = builtins.input\n"
    "builtins.input = lambda *_: _ler()\n"
    "sys.argv = [os.path.abspath(sys.argv[1]), *sys.argv[2:]]\n"
    "sys.path[0] = os.path.dirname(sys.argv[0])\n"
    "runpy.run_path(sys.argv[0], run_name='__main__')\n"
)
fim = lambda b: (b or b'').decode('utf-8', 'replace')[-20000:]
saida = []
for c in casos:
    comando = [sys.executable, programa] if considerar_prompts else [sys.executable, '-c', SEM_PROMPTS, programa]
    try:
        p = subprocess.run([*comando, *c['argumentos']], input=c['entrada'].encode(),
                           capture_output=True, timeout=tempo, cwd=pasta)
        saida.append({'saida': fim(p.stdout), 'erro': fim(p.stderr),
                      'codigo': p.returncode if p.returncode >= 0 else None, 'tempo_esgotado': False})
    except subprocess.TimeoutExpired as e:
        saida.append({'saida': fim(e.stdout), 'erro': fim(e.stderr), 'codigo': None, 'tempo_esgotado': True})
print('\n@@rubricas@@' + json.dumps(saida))
"#;

const JPLAG: &str = "localhost/rubricas-jplag:6.3.0-java25";
const RECEITA_JPLAG: &str = "FROM docker.io/library/eclipse-temurin:25-jre\n\
ADD https://github.com/jplag/JPlag/releases/download/v6.3.0/jplag-6.3.0-jar-with-dependencies.jar /opt/jplag.jar\n";

/// No modo contínuo, quanto esperar para tentar de novo depois de perder o servidor.
const ESPERA_APOS_FALHA: Duration = Duration::from_secs(10);

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

// ---------------------------------------------------------------------------
// Laço principal
// ---------------------------------------------------------------------------

pub fn rodar(op: Opcoes) -> Result<(), String> {
    podman(&["--version"]).map_err(|e| format!("podman não encontrado: {e}"))?;
    let cliente = Cliente::novo(&op.servidor, &op.token);
    // Tarefas que já foram tentadas nesta rodada e voltaram com erro de execução:
    // não adianta repetir até algo mudar.
    let mut tentadas: Vec<Tarefa> = Vec::new();
    loop {
        let fila: Fila = match cliente.json("/executor/fila") {
            Ok(f) => f,
            // No modo contínuo, o servidor fora do ar é só uma espera a mais.
            Err(e) if op.continuo => {
                eprintln!("{e}. Tentando de novo em 10 s.");
                thread::sleep(ESPERA_APOS_FALHA);
                continue;
            }
            Err(e) => return Err(e),
        };
        let novas: Vec<Tarefa> = fila.tarefas.into_iter().filter(|t| !tentadas.contains(t)).collect();
        if novas.is_empty() {
            if !op.continuo {
                println!("Fila vazia.");
                return Ok(());
            }
            println!("Fila vazia. Esperando novidades do servidor…");
            while let Err(e) = cliente.esperar_evento(Duration::from_secs(600)) {
                eprintln!("{e}. Tentando de novo em 10 s.");
                thread::sleep(ESPERA_APOS_FALHA);
            }
            tentadas.clear();
            continue;
        }
        for t in novas {
            let r = match &t {
                Tarefa::Checagens { avaliacao, aluno, versao } => checagens(&cliente, avaliacao, aluno, versao),
                Tarefa::Plagio { avaliacao, versao } => plagio(&cliente, avaliacao, versao),
            };
            if let Err(e) = r {
                eprintln!("  erro: {e}");
            }
            tentadas.push(t);
        }
    }
}

/// Um nome que não se repete neste processo, nem entre processos.
fn unico() -> String {
    static CONTADOR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = CONTADOR.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{}-{n}", std::process::id(), crate::auth::agora_ms())
}

/// Pasta temporária que se apaga sozinha.
struct Temporaria(PathBuf);

impl Temporaria {
    fn nova(nome: &str) -> Result<Self, String> {
        let p = std::env::temp_dir().join(format!("rubricas-{nome}-{}", unico()));
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
    let info: RespostaPipeline = cliente.json(&rota(&["pipelines", avaliacao]))?;
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
    arquivos: &[&str],
    destino: &Path,
    original: bool,
) -> Result<Vec<String>, String> {
    let mut nomes = Vec::new();
    for a in arquivos {
        let caminho = rota(&["avaliacoes", avaliacao, "submissoes", aluno, a]);
        let bytes = cliente.bytes(&if original { format!("{caminho}?original=1") } else { caminho })?;
        gravar(&destino.join(a), &bytes)?;
        nomes.push(a.to_string());
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
    let sub: RespostaSubmissao = cliente.json(&rota(&["avaliacoes", avaliacao, "submissoes", aluno]))?;
    let caminhos: Vec<&str> = sub.arquivos.iter().map(|a| a.caminho.as_str()).collect();
    let arquivos = baixar_submissao(cliente, avaliacao, aluno, &caminhos, &trabalho, false)?;
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

    let imagem = imagem_ferramentas(&p.imagem);
    let mut resultados = Vec::new();
    for ch in &p.checagens {
        let r = match (&imagem, &programa) {
            // Sem imagem não há como rodar: o relatório diz isso, e o aluno continua pendente.
            (Err(e), _) => ResultadoChecagem::erro(ch, e),
            (_, Err(motivo)) => ResultadoChecagem::nao_rodou(ch, motivo),
            (Ok(imagem), Ok(programa)) => {
                let ambiente = Ambiente {
                    imagem: ch.imagem.clone().map(nome_completo).unwrap_or_else(|| imagem.clone()),
                    memoria: ch.memoria.clone().unwrap_or_else(|| p.memoria.clone()),
                    cpus: p.cpus,
                    trabalho: &trabalho,
                    meta: &meta,
                };
                match conteiner(&ambiente, &invocacao(ch, &p, programa)) {
                    Ok(s) => conferir::julgar(ch, &s),
                    Err(e) => ResultadoChecagem::erro(ch, &e),
                }
            }
        };
        println!("  {}: {}", r.nome, if r.situacao == Situacao::Erro { format!("erro de execução: {}", r.resumo) } else { r.resumo.clone() });
        resultados.push(r);
    }
    let relatorio = Relatorio { versao: versao.into(), motor: MOTOR.into(), gerado_em: 0, checagens: resultados };
    cliente.put_json(&rota(&["executor", "relatorios", avaliacao, aluno]), &relatorio)?;
    imagem.map(|_| ())
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

/// O que rodar no contêiner: o comando, o ambiente, o tempo máximo e os
/// arquivos que ele lê em `/meta`.
struct Invocacao {
    cmd: Vec<String>,
    env: Vec<(&'static str, String)>,
    limite: Duration,
    meta: Vec<(&'static str, String)>,
}

fn invocacao(ch: &Checagem, p: &Pipeline, programa: &str) -> Invocacao {
    // Nos casos, o tempo vale por execução; nas outras checagens, para a checagem
    // inteira, mais uma folga para o contêiner subir.
    let tempo = ch.tempo.unwrap_or(p.tempo);
    let limite = Duration::from_secs_f64(tempo + 10.0);
    let cmd = |partes: &[&str]| partes.iter().map(|s| s.to_string()).collect();
    let com_programa = vec![("PROGRAMA", programa.to_string())];
    match ch.tipo {
        TipoChecagem::Casos => {
            let casos: Vec<Value> = ch.casos.iter().map(|c| json!({ "entrada": c.entrada, "argumentos": c.argumentos })).collect();
            let prompts = if ch.considerar_prompts { "1" } else { "0" };
            Invocacao {
                cmd: cmd(&["python3", "/meta/casos.py", programa, &tempo.to_string(), "/meta/casos.json", "/trabalho", prompts]),
                env: vec![],
                limite: Duration::from_secs_f64(tempo * ch.casos.len() as f64 + 30.0),
                meta: vec![("casos.json", serde_json::to_string(&casos).unwrap())],
            }
        }
        TipoChecagem::Pytest => Invocacao {
            cmd: cmd(&["python3", "-m", "pytest", "-q", "-p", "no:cacheprovider", "-rfE", "--tb=short", &ch.testes]),
            env: com_programa,
            limite,
            meta: vec![],
        },
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
                let marca = conferir::MARCA_FORMATO;
                script += &format!("; ruff format --no-cache --check --diff {} >/meta/formato 2>&1 || echo {marca}", aspas(programa));
            }
            Invocacao { cmd: cmd(&["sh", "-c", &script]), env: vec![], limite, meta: vec![] }
        }
        TipoChecagem::Comando => Invocacao { cmd: cmd(&["sh", "-c", &ch.comando]), env: com_programa, limite, meta: vec![] },
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
    let todas: Submissoes = cliente.json(&rota(&["avaliacoes", avaliacao, "submissoes"]))?;
    for (aluno, s) in &todas.alunos {
        let caminhos: Vec<&str> = s.arquivos.iter().map(|a| a.caminho.as_str()).collect();
        baixar_submissao(cliente, avaliacao, aluno, &caminhos, &trabalho.join("alunos").join(aluno), true)?;
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
        let inv = Invocacao { cmd, env: vec![("JAVA_TOOL_OPTIONS", "-Xmx768m".into())], limite: Duration::from_secs(600), meta: vec![] };
        let s = conteiner(&amb, &inv)?;
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

/// Roda um comando num contêiner sem rede, com memória, CPU e tempo limitados.
/// `Err` é falha do próprio executor (podman, imagem), nunca do código do aluno.
fn conteiner(amb: &Ambiente, inv: &Invocacao) -> Result<Execucao, String> {
    for (nome, conteudo) in &inv.meta {
        gravar(&amb.meta.join(nome), conteudo.as_bytes())?;
    }
    let nome = format!("rubricas-{}", unico());
    let vol = |p: &Path, destino: &str, modo: &str| format!("{}:{destino}:{modo}", p.display());
    let mut args: Vec<String> = [
        "run", "--rm", "--name", &nome, "--network=none", "--pids-limit=256", "--cap-drop=ALL",
        "--security-opt=no-new-privileges", "-w", "/trabalho", "-e", "PYTHONDONTWRITEBYTECODE=1", "-e", "PYTHONIOENCODING=utf-8",
    ]
    .map(String::from)
    .into();
    args.extend([format!("--memory={}", amb.memoria), format!("--memory-swap={}", amb.memoria), format!("--cpus={}", amb.cpus)]);
    args.extend(["-v".into(), vol(amb.trabalho, "/trabalho", "Z"), "-v".into(), vol(amb.meta, "/meta", "z")]);
    for (k, v) in &inv.env {
        args.extend(["-e".into(), format!("{k}={v}")]);
    }
    args.push(amb.imagem.clone());
    args.extend(inv.cmd.iter().cloned());

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
        if inicio.elapsed() > inv.limite {
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
    Ok(Execucao { saida, erro, codigo: status.code(), tempo_esgotado })
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Roda o script dos casos com o python3 da máquina, sem contêiner. `arquivos`
    /// é o código do aluno (o programa é `main.py`); cada caso, (entrada, argumentos).
    fn rodar(arquivos: &[(&str, &str)], casos: &[(&str, &[&str])], considerar_prompts: bool) -> Vec<Execucao> {
        let tmp = Temporaria::nova("teste-casos").unwrap();
        for (nome, conteudo) in arquivos {
            gravar(&tmp.0.join(nome), conteudo.as_bytes()).unwrap();
        }
        gravar(&tmp.0.join("casos.py"), EXECUTA_CASOS.as_bytes()).unwrap();
        let casos: Vec<Value> = casos.iter().map(|(e, a)| json!({ "entrada": e, "argumentos": a })).collect();
        gravar(&tmp.0.join("casos.json"), serde_json::to_string(&casos).unwrap().as_bytes()).unwrap();
        let pasta = tmp.0.to_string_lossy().into_owned();
        let prompts = if considerar_prompts { "1" } else { "0" };
        let saida = Command::new("python3")
            .args([&format!("{pasta}/casos.py"), "main.py", "5", &format!("{pasta}/casos.json"), &pasta, prompts])
            .output()
            .expect("python3 na máquina");
        let mut ex = conferir::ler_execucoes(&String::from_utf8_lossy(&saida.stdout)).expect("o script imprimiu os resultados");
        // Cada rodada tem sua pasta; no contêiner ela é sempre /trabalho.
        for e in &mut ex {
            e.erro = e.erro.replace(&pasta, "/trabalho");
        }
        ex
    }

    const ELEICAO: &str = "nome = input('Qual o seu nome? ')\nvotos = int(input('Quantos votos? '))\nprint(f'{nome} teve {votos} votos')\n";

    #[test]
    fn prompts_do_input_nao_entram_na_saida_dos_casos() {
        let ex = rodar(&[("main.py", ELEICAO)], &[("Ana\n10\n", &[]), ("Ana\n", &[])], false);
        assert_eq!(ex[0].saida, "Ana teve 10 votos\n");
        assert_eq!(ex[0].codigo, Some(0));
        // faltou entrada: o input() continua dando EOFError, como o original
        assert_eq!(ex[1].codigo, Some(1));
        assert!(ex[1].erro.contains("EOFError"), "{}", ex[1].erro);
        assert!(!ex[1].saida.contains("Quantos votos?"));

        // com considerar_prompts, a saída é a de sempre
        let ex = rodar(&[("main.py", ELEICAO)], &[("Ana\n10\n", &[])], true);
        assert_eq!(ex[0].saida, "Qual o seu nome? Quantos votos? Ana teve 10 votos\n");
    }

    #[test]
    fn sem_prompts_o_erro_mostrado_e_o_mesmo_da_execucao_direta() {
        // a execução direta (considerar_prompts) é a referência
        let programas = [
            ("sintaxe", "x = (\n"),
            ("exec do aluno", "exec('1/0')\n"),
            ("falta entrada", ELEICAO),
        ];
        for (nome, programa) in programas {
            let sem = rodar(&[("main.py", programa)], &[("Ana\n", &[])], false);
            let direto = rodar(&[("main.py", programa)], &[("Ana\n", &[])], true);
            assert_eq!(sem[0].erro, direto[0].erro, "{nome}");
            assert_eq!(sem[0].codigo, direto[0].codigo, "{nome}");
        }
    }

    #[test]
    fn sem_prompts_o_programa_roda_como_principal_e_importa_vizinhos() {
        let main_py = "import sys\nimport util\nif __name__ == '__main__':\n    print(util.dobro(int(input('x? '))), sys.argv[1:])\n    sys.exit(3)\n";
        let util_py = "def dobro(x):\n    return 2 * x\n";
        let ex = rodar(&[("main.py", main_py), ("util.py", util_py)], &[("21\n", &["-v"])], false);
        assert_eq!(ex[0].saida, "42 ['-v']\n");
        assert_eq!(ex[0].codigo, Some(3), "sys.exit vale como antes");
    }
}
