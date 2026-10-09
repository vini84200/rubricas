// Gera a tabela de arquivos da pasta `web/` embutidos no binário.
use std::{env, fs, path::Path};

fn coletar(dir: &Path, base: &Path, saida: &mut Vec<(String, String)>) {
    let mut entradas: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entradas.sort();
    for caminho in entradas {
        if caminho.is_dir() {
            coletar(&caminho, base, saida);
        } else {
            let rel = caminho.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            saida.push((rel, caminho.to_string_lossy().into_owned()));
        }
    }
}

fn main() {
    let raiz = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("web");
    let mut arquivos = Vec::new();
    coletar(&raiz, &raiz, &mut arquivos);
    let mut codigo = String::from("pub static ARQUIVOS: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &arquivos {
        codigo.push_str(&format!("    ({rel:?}, include_bytes!({abs:?})),\n"));
    }
    codigo.push_str("];\n");
    let destino = Path::new(&env::var("OUT_DIR").unwrap()).join("embutidos.rs");
    fs::write(destino, codigo).unwrap();
    println!("cargo:rerun-if-changed=web");
}
