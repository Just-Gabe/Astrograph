//! Paridade dos textobjects em Rust (ROADMAP, S2) com o
//! nvim-treesitter-textobjects: para cada arquivo, objeto e cursor amostrado,
//! o trecho do `textobject_at_point` do plugin (com lookahead, como o
//! Astrograph configura) e o do `Document::textobject_at`.
//!
//!   cargo run --release --example textobj_e2e -- <arquivos...>

use std::time::{Duration, Instant};

use astrograph_syntax::{detect, lang, Document};
use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Value};
use tokio::process::{ChildStdin, Command};

#[derive(Clone)]
struct Nada;

#[async_trait::async_trait]
impl Handler for Nada {
    type Writer = Compat<ChildStdin>;
}

const OBJS: [&str; 12] = [
    "@block.outer",
    "@block.inner",
    "@class.outer",
    "@class.inner",
    "@conditional.outer",
    "@conditional.inner",
    "@function.outer",
    "@function.inner",
    "@loop.outer",
    "@loop.inner",
    "@parameter.outer",
    "@parameter.inner",
];

/// Para cada (objeto, linha, coluna): o trecho do plugin ou {} se nenhum.
const NVIM: &str = "local objs, pts = ... ; local sh = require('nvim-treesitter-textobjects.shared'); local out = {}; \
    for i, p in ipairs(pts) do local r = sh.textobject_at_point(objs[p[1]], 'textobjects', 0, { p[2] + 1, p[3] }, { lookahead = true }); \
      out[i] = r and { r[1], r[2], r[4], r[5] } or {} end; return out";

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let mut cmd = Command::new(std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".into()));
    cmd.args(["--embed", "--headless", "-n"]);
    let (nvim, _io, _child) = create::new_child_cmd(&mut cmd, Nada).await.expect("nvim");
    tokio::time::sleep(Duration::from_millis(4000)).await;
    let (mut total, mut same, mut none_both) = (0usize, 0usize, 0usize);
    let mut t_rust = Duration::ZERO;
    for file in &files {
        let Some(l) = detect(file, "").and_then(lang) else { continue };
        nvim.command(&format!("edit {}", file.replace(' ', "\\ "))).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let lines: Vec<String> = nvim
            .exec_lua("return vim.api.nvim_buf_get_lines(0, 0, -1, false)", vec![])
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();
        let doc = Document::new(l, &lines);
        // cursores: a cada ~5 linhas, no comeco, no 1o caractere e no meio
        let step = (lines.len() / 80).max(1);
        let mut pts: Vec<(usize, usize, usize)> = Vec::new();
        for (row, line) in lines.iter().enumerate().step_by(step) {
            let ind = line.len() - line.trim_start().len();
            let mut cols = vec![0, ind, line.len() / 2];
            cols.dedup();
            for col in cols.into_iter().filter(|c| *c < line.len().max(1)) {
                for o in 0..OBJS.len() {
                    if doc.has_textobject(OBJS[o]) {
                        pts.push((o, row, col));
                    }
                }
            }
        }
        let arg_pts = Value::from(
            pts.iter()
                .map(|(o, r, c)| Value::from(vec![Value::from(*o as i64 + 1), Value::from(*r as i64), Value::from(*c as i64)]))
                .collect::<Vec<_>>(),
        );
        let arg_objs = Value::from(OBJS.iter().map(|s| Value::from(*s)).collect::<Vec<_>>());
        let want = nvim.exec_lua(NVIM, vec![arg_objs, arg_pts]).await.unwrap();
        let want = want.as_array().unwrap();
        let mut diffs = Vec::new();
        for (i, (o, row, col)) in pts.iter().enumerate() {
            let w: Vec<i64> = want[i].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
            let t = Instant::now();
            let g = doc.textobject_at(OBJS[*o], *row, *col, true, false);
            t_rust += t.elapsed();
            let g: Vec<i64> = g.map(|r| vec![r.srow as i64, r.scol as i64, r.erow as i64, r.ecol as i64]).unwrap_or_default();
            total += 1;
            if g == w {
                same += 1;
                if g.is_empty() {
                    none_both += 1;
                }
            } else {
                diffs.push((OBJS[*o], *row, *col, w, g));
            }
        }
        println!("{file}: {} casos, {} iguais", pts.len(), pts.len() - diffs.len());
        for (o, r, c, w, g) in diffs.iter().take(4) {
            println!("    {o} em ({r},{c}): plugin {w:?}, rust {g:?}  | {}", lines[*r].trim_end());
        }

        // movimento: ]x/[x de inicio e de fim, como os atalhos do Astrograph
        let moves = [("goto_next_start", true, Some(true)), ("goto_next_end", true, Some(false)), ("goto_previous_start", false, Some(true)), ("goto_previous_end", false, Some(false))];
        let mv_objs = ["@block.outer", "@function.outer", "@parameter.inner"];
        let mut mv_pts = Vec::new();
        for (row, line) in lines.iter().enumerate().step_by(step * 3) {
            let col = line.len() - line.trim_start().len();
            for (mi, _) in moves.iter().enumerate() {
                for (oi, o) in mv_objs.iter().enumerate() {
                    if doc.has_textobject(o) {
                        mv_pts.push((mi, oi, row, col));
                    }
                }
            }
        }
        let arg = Value::from(
            mv_pts
                .iter()
                .map(|(mi, oi, r, c)| {
                    Value::from(vec![Value::from(moves[*mi].0), Value::from(mv_objs[*oi]), Value::from(*r as i64), Value::from(*c as i64)])
                })
                .collect::<Vec<_>>(),
        );
        let want = nvim
            .exec_lua(
                "local pts = ... ; local mv = require('nvim-treesitter-textobjects.move'); local out = {}; \
                 for i, p in ipairs(pts) do vim.api.nvim_win_set_cursor(0, { p[3] + 1, p[4] }); mv[p[1]](p[2], 'textobjects'); \
                   local c = vim.api.nvim_win_get_cursor(0); out[i] = { c[1] - 1, c[2] } end; return out",
                vec![arg],
            )
            .await
            .unwrap();
        let want = want.as_array().unwrap();
        let mut mv_diffs = Vec::new();
        for (i, (mi, oi, r, c)) in mv_pts.iter().enumerate() {
            let w: Vec<i64> = want[i].as_array().unwrap().iter().filter_map(Value::as_i64).collect();
            let (_, fwd, st) = moves[*mi];
            let g = doc.textobject_goto(mv_objs[*oi], fwd, st, *r, *c, 1).unwrap_or((*r, *c));
            total += 1;
            if w == [g.0 as i64, g.1 as i64] {
                same += 1;
            } else {
                mv_diffs.push((moves[*mi].0, mv_objs[*oi], *r, *c, w, g));
            }
        }
        println!("    movimento: {}/{} iguais", mv_pts.len() - mv_diffs.len(), mv_pts.len());
        for d in mv_diffs.iter().take(3) {
            println!("      {d:?}");
        }

        // troca: o plugin troca de verdade (e desfaz com `u`); o Rust devolve as edicoes
        let mut sw_total = 0;
        let mut sw_diffs = Vec::new();
        for (row, line) in lines.iter().enumerate().step_by(step * 4) {
            let col = line.len() - line.trim_start().len();
            for o in ["@parameter.inner", "@function.outer", "@block.outer"] {
                if !doc.has_textobject(o) {
                    continue;
                }
                for (fname, dir) in [("swap_next", 1isize), ("swap_previous", -1isize)] {
                    let w = nvim
                        .exec_lua(
                            "local f, o, r, c = ... ; vim.api.nvim_win_set_cursor(0, { r + 1, c }); \
                             local before = vim.b.changedtick; require('nvim-treesitter-textobjects.swap')[f](o, 'textobjects'); \
                             vim.cmd('normal! g@l'); local changed = vim.b.changedtick ~= before; \
                             local cur = vim.api.nvim_win_get_cursor(0); local ls = vim.api.nvim_buf_get_lines(0, 0, -1, false); \
                             if changed then vim.cmd('silent! undo') end; return { changed, cur[1] - 1, cur[2], ls }",
                            vec![Value::from(fname), Value::from(o), Value::from(row as i64), Value::from(col as i64)],
                        )
                        .await
                        .unwrap();
                    let w = w.as_array().unwrap().clone();
                    let changed = w[0].as_bool().unwrap_or(false);
                    let g = doc.textobject_swap(o, dir, row, col);
                    sw_total += 1;
                    total += 1;
                    let ok = match (&g, changed) {
                        (None, false) => true,
                        (Some((edits, cur)), true) => {
                            let mut text = lines.join("\n");
                            for e in edits {
                                let line_byte = |r: usize| lines[..r].iter().map(|l| l.len() + 1).sum::<usize>();
                                let (s, en) = (line_byte(e.srow) + e.scol, line_byte(e.erow) + e.ecol);
                                text.replace_range(s..en, &e.lines.join("\n"));
                            }
                            let want_lines: Vec<String> = w[3].as_array().unwrap().iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
                            text == want_lines.join("\n") && [cur.0 as i64, cur.1 as i64] == [w[1].as_i64().unwrap(), w[2].as_i64().unwrap()]
                        }
                        _ => false,
                    };
                    if ok {
                        same += 1;
                    } else {
                        if sw_diffs.len() < 2 && std::env::var("DETALHE").is_ok() {
                            let want_lines: Vec<String> = w[3].as_array().unwrap().iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
                            println!("      DETALHE {fname} {o} ({row},{col}): plugin cursor ({},{})", w[1], w[2]);
                            if let Some((edits, cur)) = &g {
                                println!("        rust cursor {cur:?}, edicoes {edits:?}");
                                let mut text = lines.join("\n");
                                for e in edits {
                                    let line_byte = |r: usize| lines[..r].iter().map(|l| l.len() + 1).sum::<usize>();
                                    let (s, en) = (line_byte(e.srow) + e.scol, line_byte(e.erow) + e.ecol);
                                    text.replace_range(s..en, &e.lines.join("\n"));
                                }
                                for (i, (a, b)) in text.split('\n').zip(want_lines.iter()).enumerate() {
                                    if a != b {
                                        println!("        1a linha diferente {i}:\n          rust  |{a}\n          plugin|{b}");
                                        break;
                                    }
                                }
                            } else {
                                println!("        rust: nenhuma troca; objeto no ponto = {:?}", doc.textobject_at(o, row, col, false, false));
                            }
                        }
                        sw_diffs.push((fname, o, row, col, changed, g.map(|x| x.1)));
                    }
                }
            }
        }
        println!("    troca: {}/{} iguais", sw_total - sw_diffs.len(), sw_total);
        for d in sw_diffs.iter().take(3) {
            println!("      {d:?}");
        }
    }
    println!(
        "TOTAL: {same}/{total} iguais ({:.2}%; {none_both} sem objeto nos dois); Rust {:.1} us por caso",
        100.0 * same as f64 / total.max(1) as f64,
        t_rust.as_secs_f64() * 1e6 / total.max(1) as f64
    );
    let _ = nvim.command("qa!").await;
}
