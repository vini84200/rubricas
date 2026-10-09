//! A aplicação web: embutida no binário ou, com `--web`, lida do disco a cada pedido.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/embutidos.rs"));

pub struct Web {
    disco: Option<PathBuf>,
    versao: String,
}

fn tipo(caminho: &str) -> &'static str {
    match caminho.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn listar_disco(dir: &Path, base: &Path, saida: &mut Vec<String>) {
    let Ok(entradas) = std::fs::read_dir(dir) else { return };
    for e in entradas.flatten() {
        let p = e.path();
        if p.is_dir() {
            listar_disco(&p, base, saida);
        } else if let Ok(rel) = p.strip_prefix(base) {
            saida.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

impl Web {
    pub fn novo(disco: Option<PathBuf>) -> Self {
        let versao = match &disco {
            Some(_) => format!("dev-{}", crate::auth::agora()),
            None => {
                let mut h = Sha256::new();
                for (nome, bytes) in ARQUIVOS {
                    h.update(nome.as_bytes());
                    h.update(bytes);
                }
                h.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect()
            }
        };
        Web { disco, versao }
    }

    fn nomes(&self) -> Vec<String> {
        let mut nomes = match &self.disco {
            Some(raiz) => {
                let mut v = Vec::new();
                listar_disco(raiz, raiz, &mut v);
                v
            }
            None => ARQUIVOS.iter().map(|(n, _)| n.to_string()).collect(),
        };
        nomes.sort();
        nomes
    }

    fn ler(&self, nome: &str) -> Option<Vec<u8>> {
        match &self.disco {
            Some(raiz) => {
                if nome.split('/').any(|parte| parte.is_empty() || parte.starts_with('.')) {
                    return None;
                }
                std::fs::read(raiz.join(nome)).ok()
            }
            None => ARQUIVOS.iter().find(|(n, _)| *n == nome).map(|(_, b)| b.to_vec()),
        }
    }

    pub fn servir(&self, caminho: &str, pedido: &HeaderMap) -> Response {
        let nome = caminho.trim_start_matches('/');
        // As telas ficam depois do `#`, então só a raiz devolve o index.
        let nome = if nome.is_empty() { "index.html" } else { nome };
        let Some(mut corpo) = self.ler(nome) else {
            return (StatusCode::NOT_FOUND, "Não encontrado").into_response();
        };
        let etag = format!("\"{}\"", self.versao);
        if self.disco.is_none() && pedido.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
            return StatusCode::NOT_MODIFIED.into_response();
        }
        if nome == "sw.js" {
            // O service worker recebe a versão e a lista do que guardar para uso offline.
            let guardar: Vec<String> = self
                .nomes()
                .into_iter()
                .filter(|n| n != "sw.js" && !n.ends_with(".txt"))
                .map(|n| if n == "index.html" { "./".to_string() } else { n })
                .collect();
            let texto = String::from_utf8_lossy(&corpo)
                .replace("__VERSAO__", &self.versao)
                .replace("\"__ARQUIVOS__\"", &serde_json::to_string(&guardar).unwrap());
            corpo = texto.into_bytes();
        }
        let mut resposta = corpo.into_response();
        let h = resposta.headers_mut();
        h.insert(header::CONTENT_TYPE, HeaderValue::from_static(tipo(nome)));
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        if let Ok(v) = HeaderValue::from_str(&etag) {
            h.insert(header::ETAG, v);
        }
        resposta
    }
}
