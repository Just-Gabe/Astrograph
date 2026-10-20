//! Paridade do indent em Rust (ROADMAP, S2) com o nvim-treesitter: para cada
//! arquivo, a `indentexpr` do Neovim (com a config instalada) e o
//! `Document::indent` em Rust, linha a linha, sobre o mesmo texto.
//!
//!   cargo run --release --example indent_e2e -- <arquivos...>

use std::time::{Duration, Instant};

use astrograph_syntax::{detect, lang, Document};
use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, Value};
use tokio::process::{ChildStdin, Command};

#[derive(Clone)]
struct Nada;

#[async_trait::async_trait]
impl Handler for Nada {
    type Writer = Compat<ChildStdin>;
}

const NVIM_INDENTS: &str = "local expr = vim.bo.indentexpr; local out = {}; \
    for l = 1, vim.api.nvim_buf_line_count(0) do vim.v.lnum = l; out[l] = vim.fn.eval(expr) end; \
    return { expr, vim.fn.shiftwidth(), vim.bo.tabstop, out, vim.api.nvim_buf_get_lines(0, 0, -1, false), vim.bo.filetype }";

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let mut cmd = Command::new(std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".into()));
    cmd.args(["--embed", "--headless", "-n"]);
    let (nvim, _io, _child) = create::new_child_cmd(&mut cmd, Nada).await.expect("nvim");
    tokio::time::sleep(Duration::from_millis(4000)).await;

    let (mut total, mut same) = (0usize, 0usize);
    let (mut t_rust, mut n_rust) = (Duration::ZERO, 0u32);
    for file in &files {
        let Some(l) = detect(file, "").and_then(lang) else {
            println!("{file}: sem gramatica");
            continue;
        };
        nvim.command(&format!("edit {}", file.replace(' ', "\\ "))).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let r = nvim.exec_lua(NVIM_INDENTS, vec![]).await.unwrap();
        let r = r.as_array().unwrap();
        let expr = r[0].as_str().unwrap_or("").to_string();
        let sw = r[1].as_i64().unwrap();
        let ts = r[2].as_i64().unwrap() as usize;
        let want: Vec<i64> = r[3].as_array().unwrap().iter().map(|v| v.as_i64().unwrap_or(-9)).collect();
        let ft = r[5].as_str().unwrap_or("").to_string();
        let lines: Vec<String> = r[4].as_array().unwrap().iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
        if !expr.contains("nvim-treesitter") {
            println!("{file}: indentexpr do nvim nao e do nvim-treesitter ({expr}); pulei");
            continue;
        }
        let doc = Document::new(l, &lines);
        let mut diffs = Vec::new();
        for (i, w) in want.iter().enumerate() {
            let t = Instant::now();
            let got = doc.indent(i + 1, sw, ts);
            t_rust += t.elapsed();
            n_rust += 1;
            if got == *w {
                same += 1;
            } else {
                diffs.push((i + 1, *w, got));
            }
            total += 1;
        }
        let pos = want.iter().filter(|w| **w > 0).count();
        let auto = want.iter().filter(|w| **w == -1).count();
        let mut levels: Vec<i64> = want.clone();
        levels.sort();
        levels.dedup();
        println!(
            "{file}: {} linhas, {} iguais ({:.1}%); nvim: {pos} com indent > 0, {auto} = -1, niveis {:?}",
            want.len(),
            want.len() - diffs.len(),
            100.0 * (want.len() - diffs.len()) as f64 / want.len().max(1) as f64,
            &levels[..levels.len().min(8)]
        );
        for (ln, w, g) in diffs.iter().take(4) {
            println!("    linha {ln}: nvim {w}, rust {g}  | {}", lines[ln - 1].trim_end());
        }

        // digitando: o arquivo cortado na linha k (codigo incompleto, sem o
        // `end`/`}`) e uma linha nova vazia, como ao apertar `o` no fim
        let step = (lines.len() / 60).max(1);
        let (mut cut_total, mut cut_same) = (0usize, 0usize);
        let mut cut_diffs = Vec::new();
        for k in (1..lines.len()).step_by(step) {
            let mut part: Vec<String> = lines[..k].to_vec();
            part.push(String::new());
            // buffer novo a cada caso: reaproveitar a arvore antiga (parse
            // incremental) muda a recuperacao de erro em codigo incompleto, e o
            // Rust parseia do zero
            let w = nvim
                .exec_lua(
                    "local ls, l, ft, sw, ts = ... ; local b = vim.api.nvim_create_buf(false, true); \
                     vim.api.nvim_buf_set_lines(b, 0, -1, false, ls); vim.bo[b].filetype = ft; \
                     vim.bo[b].shiftwidth = sw; vim.bo[b].tabstop = ts; \
                     local r = vim.api.nvim_buf_call(b, function() vim.v.lnum = l; return require('nvim-treesitter').indentexpr() end); \
                     vim.api.nvim_buf_delete(b, { force = true }); return r",
                    vec![
                        Value::from(part.iter().map(|s| Value::from(s.as_str())).collect::<Vec<_>>()),
                        Value::from(k as i64 + 1),
                        Value::from(ft.as_str()),
                        Value::from(sw),
                        Value::from(ts as i64),
                    ],
                )
                .await
                .unwrap()
                .as_i64()
                .unwrap_or(-9);
            let g = Document::new(l, &part).indent(k + 1, sw, ts);
            cut_total += 1;
            if g == w {
                cut_same += 1;
            } else {
                cut_diffs.push((k, w, g));
            }
        }
        same += cut_same;
        total += cut_total;
        println!("    digitando (linha nova depois de k): {cut_same}/{cut_total} iguais");
        for (k, w, g) in cut_diffs.iter().take(4) {
            println!("    depois da linha {k}: nvim {w}, rust {g}  | {}", lines[k - 1].trim_end());
        }
        let _ = nvim.command("silent! edit!").await;
    }
    println!(
        "TOTAL: {same}/{total} linhas iguais ({:.2}%); Rust {:.1} us por linha",
        100.0 * same as f64 / total.max(1) as f64,
        t_rust.as_secs_f64() * 1e6 / n_rust.max(1) as f64
    );
    let _ = nvim.command("qa!").await;
    let _ = Value::Nil;
}
