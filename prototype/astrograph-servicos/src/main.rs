//! `astrograph-servicos <comando>`: uma linha de JSON no stdout.
//!
//!   preparo <pasta>        linguagens do projeto e resumo do git
//!   atualizar [opcoes]     atualizacao transacional (veja `atualizar.rs`)
//!   nativo <raiz>          compila os executaveis em Rust que faltam

use std::path::PathBuf;

use astrograph_servicos::{atualizar, nativo, projeto};
use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let resto = &args[args.len().min(1)..];
    let saida = match cmd {
        "preparo" => {
            let pasta = PathBuf::from(resto.first().cloned().unwrap_or_else(|| ".".into()));
            let langs = projeto::detectar(&pasta);
            let larga = projeto::muito_larga(&pasta);
            let git = if larga { Ok(None) } else { projeto::resumo_git(&pasta) };
            json!({
                "pasta": pasta.to_string_lossy(),
                "linguagens": langs,
                "pacotes_mason": projeto::pacotes_mason(&langs),
                "git": git.as_ref().ok().cloned().flatten(),
                "git_instalado": git.is_ok(),
                "muito_larga": larga,
            })
        }
        "atualizar" => {
            let mut o = atualizar::Opcoes::nova(".");
            let mut i = 0;
            while i < resto.len() {
                match resto[i].as_str() {
                    "--raiz" => {
                        i += 1;
                        o.raiz = PathBuf::from(resto.get(i).cloned().unwrap_or_default());
                    }
                    "--ramo" => {
                        i += 1;
                        o.ramo = resto.get(i).cloned();
                    }
                    "--nvim" => {
                        i += 1;
                        o.nvim = resto.get(i).cloned().unwrap_or(o.nvim);
                    }
                    "--forcar" => o.forcar = true,
                    "--sem-plugins" => o.restaurar_plugins = false,
                    "--sem-nativo" => o.recompilar = false,
                    "--sem-verificar" => o.verificar = false,
                    outro => {
                        eprintln!("opcao desconhecida: {outro}");
                        std::process::exit(2);
                    }
                }
                i += 1;
            }
            serde_json::to_value(atualizar::atualizar(&o)).unwrap()
        }
        "nativo" => {
            let raiz = PathBuf::from(resto.first().cloned().unwrap_or_else(|| ".".into()));
            let (ok, falhou) = nativo::compilar(&raiz, None);
            json!({ "compilados": ok, "falharam": falhou })
        }
        _ => {
            eprintln!("uso: astrograph-servicos preparo <pasta> | atualizar [--raiz D] [--ramo R] [--nvim BIN] [--forcar] [--sem-plugins] [--sem-nativo] [--sem-verificar] | nativo <raiz>");
            std::process::exit(2);
        }
    };
    println!("{saida}");
}
