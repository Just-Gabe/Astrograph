//! Teste diferencial: sequências de teclas aleatórias (determinísticas pela
//! semente) rodam no `nvim --embed --headless --clean` e no núcleo em Rust; o
//! texto, o cursor, o modo e o registrador sem nome têm de bater. Cada divergência
//! é reduzida ao menor trecho de teclas que ainda diverge.
//!
//!   NVIM_BIN=... cargo run --release --example diferencial -- [casos] [semente] [vocabulario]
//!
//! `vocabulario`: `mov` (só movimentos), `op` (+ operadores), `tudo`.

use std::collections::BTreeMap;

use astrograph_core::Editor;
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

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

const CORPUS: &[&[&str]] = &[
    &["hello world foo.bar baz", "  indented line here", "", "last, line (with parens) [and] {braces}"],
    &["one two three", "four five six", "seven eight nine", "ten"],
    &["a", "", "", "b c", "", "d"],
    &["foo(bar, baz) + qux", "\tx = y;", "if (a && b) { c }", "  trailing  "],
    &["árvore é verde", "日本語 text", "x"],
    &["single line only"],
    &["", ""],
    &["say \"hello world\" and 'x y' end", "f(a, (b), c) { x } [1, 2] <t>", "  \"q1\"  \"q2\" z"],
    &["if (x) {", "  foo(1, 2);", "  bar;", "}", "", "para two", "second line", "", "", "last para"],
    &["one  two   three", "  lead  word", "tail ", "", "a.b-c d_e"],
];

fn vocabulario_visual() -> Vec<&'static str> {
    let starts = ["v", "V"];
    let motions = [
        "", "l", "j", "k", "w", "e", "b", "$", "0", "^", "iw", "aw", "ip", "ap", "i(", "a(", "i\"", "a\"", "}", "gg", "G", "fo", "2w", "3j", "2l", "jj", "jw", "wwe", "$j", "j$", "ob", "jol", "i{", "a{", "iW",
    ];
    let ops = ["d", "y", "cX<Esc>", "x", "X", "D", "Y", "C-<Esc>", "sX<Esc>", "S-<Esc>", ">", "<", "~", "u", "U", "J", "rz", "<Esc>", "2>"];
    let mut out: Vec<&'static str> = Vec::new();
    for s in starts {
        for m in motions {
            for o in ops {
                out.push(Box::leak(format!("{s}{m}{o}").into_boxed_str()));
            }
        }
    }
    for m in ["w", "j", "$", "fo", "e", "b", "k", "0", "}", "jj", "2w", "l", "x", "dd", "p"] {
        out.push(m);
    }
    out
}

fn vocabulario(nome: &str) -> Vec<&'static str> {
    if nome == "vis" {
        return vocabulario_visual();
    }
    let mov = vec![
        "h", "j", "k", "l", "w", "W", "b", "B", "e", "E", "0", "^", "$", "_", "gg", "G", "ge", "gE", "fo", "fe", "Fo", "to", "te", "To", ";", ",", "%", "{", "}", "+", "-",
        "2w", "3w", "2b", "2e", "3l", "2j", "2k", "3h", "2fo", "2$", "2G", "3G", "2}", "2{", "2ge",
    ];
    let op = vec![
        "dw", "dW", "db", "dB", "de", "dE", "d0", "d^", "d$", "dd", "dj", "dk", "dG", "dgg", "dfo", "dto", "dFo", "dTo", "d}", "d{", "d2w", "2dw", "d3l", "3dd", "2dd", "d_",
        "yw", "yW", "yb", "ye", "y0", "y$", "yy", "yj", "yk", "yG", "ygg", "yfo", "y}", "2yy", "y2w", "Y",
        "cwX<Esc>", "cWX<Esc>", "ceX<Esc>", "cbX<Esc>", "c$X<Esc>", "ccX<Esc>", "cjX<Esc>", "c2wX<Esc>", "cfoX<Esc>", "ctoX<Esc>", "C-<Esc>", "SX<Esc>", "sX<Esc>", "2sX<Esc>",
        "x", "X", "D", "3x", "2X", "p", "P", "2p", "3P", "J", "3J", "ra", "3rb", "~", "3~", ">>", "<<", ">j", "2>>",
    ];
    let ins = vec![
        "ihi<Esc>", "ahi<Esc>", "Ihi<Esc>", "Ahi<Esc>", "ohi<Esc>", "Ohi<Esc>", "3ix<Esc>", "2ay<Esc>", "ia<CR>b<Esc>", "ox<BS><BS>y<Esc>", "A<CR><Esc>", "o<Esc>", "O<Esc>", "ia<BS><Esc>",
        "i<CR>x<Esc>", "A  z<Esc>",
    ];
    let obj = vec![
        "diw", "daw", "diW", "daW", "ciwX<Esc>", "cawX<Esc>", "yiw", "yaw", "d2aw", "d3iw", "d2iw", "dip", "dap", "cipX<Esc>", "yip", "yap", "d2ap", "d2ip", "di(", "da(", "ci(X<Esc>",
        "dib", "dab", "d2i(", "d2a(", "di[", "da[", "di{", "da{", "diB", "daB", "ci{X<Esc>", "d2i{", "di\"", "da\"", "ci\"X<Esc>", "di'", "da'", "yi\"", "ya\"", "d2i\"", "d2a\"", "di<", "da<",
        "w", "e", "$", "0", "f(", "fo", "f\"", "j", "jj", "2w", "3j", "b", "ge", "}", "{", "f{", "fx",
    ];
    let hist = vec!["u", "<C-r>", "u", ".", ".", "2u", "3."];
    match nome {
        "mov" => mov,
        "obj" => obj,
        "op" => [mov, op].concat(),
        _ => [mov, op, ins, hist].concat(),
    }
}

async fn nvim_run(lines: &[&str], keys: &str) -> Option<(Vec<String>, (usize, usize), String, String)> {
    let code = r#"
        local lines, keys = ...
        local tmp = vim.fn.tempname()
        vim.fn.writefile(lines, tmp)
        vim.cmd('silent edit! ' .. vim.fn.fnameescape(tmp))
        vim.api.nvim_win_set_cursor(0, { 1, 0 })
        for _, r in ipairs({ '"', '-', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9' }) do vim.fn.setreg(r, '') end
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(keys, true, false, true), 'xt', false)
        vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes('<Esc>', true, false, true), 'nxt', false)
        local c = vim.api.nvim_win_get_cursor(0)
        local out = vim.api.nvim_buf_get_lines(0, 0, -1, false)
        local line = out[c[1]] or ''
        local col = vim.fn.charidx(line, c[2])
        return { out, { c[1] - 1, col }, vim.fn.mode(), vim.fn.getreg('"') .. '|' .. vim.fn.getregtype('"') }
    "#;
    // um nvim novo por caso: sem histórico de undo, registradores, busca de f/t nem `.` herdados
    let mut cmd = Command::new(std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".into()));
    cmd.args(["--embed", "--headless", "--clean", "-n"]);
    let (nvim, _io, _child) = create::new_child_cmd(&mut cmd, Nada).await.ok()?;
    let args = vec![Value::from(lines.iter().map(|l| Value::from(*l)).collect::<Vec<_>>()), Value::from(keys)];
    let r = nvim.exec_lua(code, args).await.ok();
    let _ = nvim.command("qa!").await;
    let r = r?;
    let a = r.as_array()?;
    let text: Vec<String> = a[0].as_array()?.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect();
    let cur = a[1].as_array()?;
    let reg = a[3].as_str()?.to_string();
    Some((text, (cur[0].as_u64()? as usize, cur[1].as_u64()? as usize), a[2].as_str()?.to_string(), reg))
}

fn rust_run(lines: &[&str], keys: &str) -> (Vec<String>, (usize, usize), String, String) {
    let mut ed = Editor::new(lines);
    ed.feed_str(keys);
    ed.feed_str("<Esc>");
    let reg = ed.register('"').map(|r| format!("{}|{}", if r.linewise { format!("{}\n", r.text) } else { r.text.clone() }, if r.linewise { "V" } else { "v" })).unwrap_or_else(|| "|v".into());
    (ed.lines(), (ed.cursor.row, ed.cursor.col), "n".into(), reg)
}

async fn diverge(lines: &[&str], toks: &[&str], cmp_reg: bool) -> Option<String> {
    let keys = toks.concat();
    let n = nvim_run(lines, &keys).await?;
    let r = rust_run(lines, &keys);
    let mut why = Vec::new();
    if n.0 != r.0 {
        why.push(format!("texto nvim={:?} rust={:?}", n.0, r.0));
    }
    if n.1 != r.1 {
        why.push(format!("cursor nvim={:?} rust={:?}", n.1, r.1));
    }
    if cmp_reg && n.3 != r.3 && !n.3.starts_with("|") {
        why.push(format!("reg nvim={:?} rust={:?}", n.3, r.3));
    }
    if why.is_empty() { None } else { Some(why.join("; ")) }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let casos: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(500);
    let seed: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let voc = vocabulario(args.get(2).map(String::as_str).unwrap_or("tudo"));
    let cmp_reg = std::env::var("SEM_REG").is_err();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (mut ok, mut ruins) = (0usize, 0usize);
    // OURO=arquivo: grava os casos em que o nvim e o Rust concordam (regressão sem nvim)
    let mut ouro: Vec<String> = Vec::new();
    let ouro_path = std::env::var("OURO").ok();
    let mut padroes: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for _ in 0..casos {
        let lines = *rng.pick(CORPUS);
        let n = 1 + rng.below(4);
        let toks: Vec<&str> = (0..n).map(|_| *rng.pick(&voc)).collect();
        match diverge(lines, &toks, cmp_reg).await {
            None => {
                ok += 1;
                if ouro_path.is_some() && ouro.len() < 400 {
                    let r = rust_run(lines, &toks.concat());
                    let linha = format!(
                        "{}\t{}\t{}\t{}\t{}",
                        json(&lines.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
                        json(&[toks.concat()]),
                        json(&r.0),
                        r.1 .0,
                        r.1 .1
                    );
                    if !ouro.contains(&linha) {
                        ouro.push(linha);
                    }
                }
            }
            Some(_) => {
                ruins += 1;
                // reduz: tira fichas enquanto ainda diverge
                let mut cur = toks.clone();
                let mut i = 0;
                while i < cur.len() {
                    let mut t = cur.clone();
                    t.remove(i);
                    if !t.is_empty() && diverge(lines, &t, cmp_reg).await.is_some() {
                        cur = t;
                    } else {
                        i += 1;
                    }
                }
                let why = diverge(lines, &cur, cmp_reg).await.unwrap_or_default();
                let chave = cur.concat();
                let e = padroes.entry(chave.clone()).or_insert((0, format!("{lines:?}\n      {why}")));
                e.0 += 1;
            }
        }
    }
    println!("{ok}/{casos} iguais ({ruins} divergem, {} padrões)", padroes.len());
    let mut lista: Vec<_> = padroes.into_iter().collect();
    lista.sort_by_key(|(k, (_, _))| k.len());
    for (k, (n, d)) in lista.iter().take(40) {
        println!("  [{n}x] {k:?}\n      {d}");
    }
    if let Some(p) = ouro_path {
        std::fs::write(&p, ouro.join("\n") + "\n").unwrap();
        println!("{} casos gravados em {p}", ouro.len());
    }
    if ruins > 0 {
        std::process::exit(1);
    }
}

fn json(v: &[String]) -> String {
    let esc = |s: &str| {
        let mut o = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\t' => o.push_str("\\t"),
                '\r' => o.push_str("\\r"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    };
    format!("[{}]", v.iter().map(|s| esc(s)).collect::<Vec<_>>().join(","))
}
