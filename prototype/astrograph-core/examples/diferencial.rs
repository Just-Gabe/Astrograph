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
    &["x = 10", "  y = -5 and 007", "0x1f 9", "a1b2 c-3", "no digits"],
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

fn vocabulario_busca() -> Vec<&'static str> {
    vec![
        "/o<CR>", "/wo<CR>", "?e<CR>", "?o<CR>", "/\\<t<CR>", "/o\\><CR>", "/foo<CR>", "/a.b<CR>", "/\\vo+<CR>", "/<CR>", "?<CR>", "n", "N", "2n", "3N", "*", "#", "2*", "g*",
        "d/e<CR>", "d?o<CR>", "dn", "dN", "d*", "y/o<CR>", "yn", "c/e<CR>X<Esc>", "cnX<Esc>", "d2/o<CR>", "/[aeiou]<CR>", "/\\s<CR>", "/\\d<CR>", "/x\\|y<CR>",
        "w", "j", "$", "0", "e", "b", "G", "gg", "fo", "2w", "}", "x", "u", ".", "p",
    ]
}

fn vocabulario_macro() -> Vec<&'static str> {
    vec![
        "qa", "qb", "q", "@a", "@b", "@@", "2@a", "3@b", "qaxq", "qadwjq", "qaA;<Esc>jq", "qbI#<Esc>q", "qaq", "qaj0q", "qbjq", "qaiX<Esc>q", "qAdwq",
        "x", "dw", "j", "k", "w", "b", "iX<Esc>", "A.<Esc>", "dd", "p", "u", ".", "3@a", "0", "$", "G", "gg", "J", "ddp", "yyp", "cwY<Esc>", "ve<Esc>",
    ]
}

fn vocabulario_marcas() -> Vec<&'static str> {
    vec![
        "ma", "mb", "'a", "'b", "`a", "`b", "d'a", "d`a", "y'a", "y`a", "c'aX<Esc>", "c`aX<Esc>", "''", "``", "d''", "y``", "gg", "G", "3G", "2j", "3k", "w", "e", "$", "0", "fo", "}", "{",
        "dd", "p", "P", "x", "u", ".", "ohi<Esc>", "Ohi<Esc>", "J", "ddp", "yyp", "3dd", "maj", "mbjj", "jma", "kmb", "v'a<Esc>", "d%", "/o<CR>", "n", "'<", "`>", "v<Esc>", "jv<Esc>",
    ]
}

fn vocabulario_vp() -> Vec<&'static str> {
    vec![
        "yw", "yy", "yiw", "yj", "y$", "yG", "dw", "dd", "x", "D", "vp", "vP", "Vp", "VP", "viwp", "viwP", "vep", "vjp", "vlp", "V2jp", "vawp", "v$p", "vgp", "vip", "Vjp", "vgP", "p", "P",
        "w", "e", "b", "j", "k", "$", "0", "G", "gg", "2w", "3j", "fo", "u", "yyp", "ve<Esc>", "v<Esc>",
    ]
}

fn vocabulario_ex() -> Vec<&'static str> {
    vec![
        ":s/o/0/<CR>", ":s/o/0/g<CR>", ":%s/e/E/g<CR>", ":%s/\\<t/T/<CR>", ":2,3s/a/b/<CR>", ":%s/\\(o\\)\\(n\\)/\\2\\1/<CR>", ":%s/o/&&/g<CR>", ":%s/x//<CR>", ":%s/\\s\\+$//e<CR>",
        ":s/\\w\\+/\\u&/<CR>", ":%s/a/\\U&/g<CR>", ":s/ /\\r/g<CR>", ":%s/e/-/gi<CR>", ":s#o#0#<CR>", ":s/\\//x/<CR>", ":.,+1s/o/O/g<CR>", ":'a,$s/e/3/<CR>", ":%s/\\v(\\w)(\\w)/\\2\\1/g<CR>",
        ":d<CR>", ":2d<CR>", ":1,2d<CR>", ":%d<CR>", ":$d<CR>", ":.,$d<CR>", ":2,3y<CR>", ":y<CR>", ":j<CR>", ":1,3j<CR>", ":><CR>", ":2,3><CR>", ":2,3<<CR>", ":>><CR>",
        ":g/o/d<CR>", ":v/o/d<CR>", ":g/e/s/e/E/<CR>", ":g/a/normal Ax<CR>", ":g!/o/d<CR>", ":2,4g/e/d<CR>", ":g/o/normal dd<CR>", ":%normal Ax<CR>", ":2,3normal I-<CR>", ":normal 2dw<CR>",
        ":2<CR>", ":$<CR>", ":1<CR>", ":.+1<CR>", ":'a<CR>",
        ":t.<CR>", ":t0<CR>", ":t$<CR>", ":2,3t0<CR>", ":1t2<CR>", ":m0<CR>", ":m$<CR>", ":1m3<CR>", ":2,3m0<CR>", ":3m1<CR>", ":1,2m$<CR>", ":2,3m.<CR>",
        ":s/o/0/<CR>j:&<CR>", ":s/o/0/g<CR>j:&&<CR>", ":s/o/0/g<CR>j:&<CR>", ":%s/o/0/g<CR>u:%&&<CR>", ":s/e/E/<CR>j&", ":s/e/E/<CR>jj:s<CR>", "/e<CR>:s//X/<CR>", ":s/e/E/<CR>/o<CR>:~<CR>", ":%s/e//n<CR>", ":%s/o/0/gn<CR>x", ":s/o/0/<CR>j@:", ":2d<CR>@:", ":d<CR>@:@@", ":s/a/b/<CR>2@:",
        ":sort<CR>", ":sort!<CR>", ":sort u<CR>", ":sort i<CR>", ":sort n<CR>", ":2,4sort<CR>", ":sort! n<CR>", ":%sort ui<CR>",
        "ma", "mb", "jma", "G", "gg", "j", "w", "$", "0", "p", "P", "u", ".", "yy", "dd", "x", "<C-r>", "3j", "wma",
    ]
}

fn vocabulario_ins() -> Vec<&'static str> {
    let mut v = vec![
        "ifoo bar<C-w>baz<Esc>", "ifoo<C-w><Esc>", "ione two<C-w><C-w>x<Esc>", "A  a.b<C-w><C-w><Esc>", "Ahey<C-u><Esc>", "Iab<C-u>c<Esc>", "ox<C-u>y<Esc>", "A<C-t>x<Esc>", "Ax<C-d><Esc>", "A<C-t><C-t>y<Esc>",
        "Afoo<C-o>0X<Esc>", "ifoo<C-o>$X<Esc>", "Afoo<C-o>xY<Esc>", "Afoo bar<C-o>bX<Esc>", "ifoo<C-o>wX<Esc>", "Abc<C-o>ddX<Esc>", "Abc<C-o>yyx<Esc>p", "ia<C-o>uX<Esc>", "Ia b<C-h><C-h>X<Esc>", "A.. a.b..<C-w>X<Esc>",
        "i  x<C-u>y<Esc>", "A<C-u>q<Esc>", "ofoo<CR>bar<C-w><C-w>q<Esc>", "A foo.bar  <C-w>X<Esc>", "A<C-w>X<Esc>", "jI<C-w>X<Esc>", "jI<C-u>X<Esc>", "jA  <C-u>X<Esc>",
        "ix<Esc>", "ayy<Esc>", "ofoo<Esc>", "Obar<Esc>", "u", ".", "ix<Esc>.",
    ];
    v.extend(mov_basico());
    v
}

fn vocabulario_bloco() -> Vec<&'static str> {
    let inicio = ["<C-v>"];
    let mov = ["", "l", "j", "jl", "jj", "jjl", "2l", "3j", "$", "j$", "jj$", "w", "e", "b", "0", "^", "ll", "jw", "je", "G", "gg", "k", "h", "jh", "jjh", "fo", "}", "jlj", "lj"];
    let ops = ["d", "x", "y", "cX<Esc>", "cXY<Esc>", "IX<Esc>", "AX<Esc>", "AXY<Esc>", "rz", "~", "u", "U", "oy", "Od", "od", "Oy", "J", "<Esc>", "s-<Esc>", "<Esc>"];
    let mut v: Vec<&'static str> = Vec::new();
    for i in inicio {
        for m in mov {
            for o in ops {
                // monta como texto estático: usa Box::leak (só o harness)
                let t: &'static str = Box::leak(format!("{i}{m}{o}").into_boxed_str());
                v.push(t);
            }
        }
    }
    v.extend(["w", "j", "k", "$", "0", "G", "gg", "u", "<C-r>", ".", "x", "dd", "p", "P", "yy", "ohi<Esc>", "3j", "fo", "e", "b", "p", "P", "p", "2p", "P"]);
    v
}

fn mov_basico() -> Vec<&'static str> {
    vec!["w", "b", "e", "j", "k", "0", "$", "^", "gg", "G", "fo", "2w", "3j", "}", "{", "x", "u", ".", "p", "P", "yy", "dd"]
}

fn vocabulario_num() -> Vec<&'static str> {
    let mut v = vec![
        "<C-a>", "<C-x>", "5<C-a>", "10<C-x>", "3<C-a>", "w<C-a>", "$<C-a>", "j<C-a>", "fx<C-a>", "0<C-a>", "e<C-x>", "b<C-a>", "99<C-a>", "100<C-x>", "w5<C-a>", "wj<C-x>", "<C-a>.", "<C-x>.", "2<C-a>.",
        "V<C-a>", "Vj<C-a>", "Vjj<C-x>", "vj<C-a>", "VG<C-a>", "Vjg<C-a>", "Vjjg<C-x>", "V2<C-a>", "Vj3<C-a>", "VGg<C-a>",
    ];
    v.extend(mov_basico());
    v
}

fn vocabulario_caso() -> Vec<&'static str> {
    let mut v = vec![
        "guu", "gUU", "g~~", "guiw", "gUiw", "g~iw", "guw", "gUw", "g~w", "gU$", "gu0", "gUj", "g~j", "gUap", "gUU", "gUgU", "gugu", "g~g~", "3gUU", "gU2w", "2guu", "gUe", "gUb", "g~$", "gUG", "gUgg",
        "gUf.", "guf.", "gUt.", "g~l", "gUh", "gU}", "gUi(", "guaw", "g~aw", "gUiW", "gUaW", "gUip", "viwU", "viwu", "vj~", "Vu", "VU", "V~", "veU", "gUl", "g~iw.",
    ];
    v.extend(mov_basico());
    v
}

fn vocabulario_ponto_visual() -> Vec<&'static str> {
    let mut v = vec![
        "vlld", "viwd", "Vjd", "vjd", "v$d", "vey", "Vy", "viwcX<Esc>", "VcX<Esc>", "vjcX<Esc>", "vld", "Vd", "<C-v>jld", "<C-v>jd", "<C-v>lld", "viw>", "V>", "Vj>", "vjJ", "VJ", "VjJ", "viwr-", "Vr-", "vlr-", "v~", "viw~", "vjU", "viwu",
        ".", "..", "2.", "j.", "w.", "u.", "u", "<C-r>", "3.",
    ];
    v.extend(mov_basico());
    v
}

fn vocabulario_registros() -> Vec<&'static str> {
    let mut v = vec![
        "\"ayy", "\"ap", "\"Ayy", "\"aP", "\"_dd", "\"_x", "\"_dw", "\"add", "\"bdw", "\"bp", "\"ayw", "\"Ayw", "\"byiw", "\"Bdw", "\"0p", "\"0P", "\"1p", "\"2p", "\"3p", "\"1P", "\"-p", "\"-P", "\".p", "\"/p", "\"ayiw", "\"acwX<Esc>", "\"_cwX<Esc>",
        "i<C-r>\"<Esc>", "i<C-r>a<Esc>", "i<C-r>0<Esc>", "i<C-r>-<Esc>", "i<C-r>1<Esc>", "a<C-r>\"<Esc>", "A<C-r>a<Esc>", "\"ayy\"bdd\"ap\"bp", "yyjdd\"0p", "dwjdw\"1p\"-p", "dddd\"1p\"2p", "/o<CR>\"/P", "ix<Esc>\".p",
        "yiwviwp", "dwviw\"0p", "\"aviwy", "\"avjd", "\"avjy\"ap", "\"Ayy\"ap",
    ];
    v.extend(mov_basico());
    v
}

fn vocabulario_marcas2() -> Vec<&'static str> {
    let mut v = vec![
        "'[", "']", "`[", "`]", "'.", "`.", "`^", "'^", "gi<Esc>", "g;", "g,", "<C-o>", "<C-i>", "d'[", "y']", "d`.", "'[d']", "`[v`]d", "ihi<Esc>`[", "yw`]", "yy'[", "yy']", "yj']", "dd'.", "x`.", "p`[", "p`]", "P`]", "ox<Esc>'[", "3G<C-o>", "G<C-o>", "gg<C-o>G<C-o><C-o>",
        "3Gx2Gx1Gxg;g;", "2Gxjjxg;g;g,", "/o<CR><C-o>", "/o<CR>n<C-o><C-o>", "G<C-o><C-i>", "}<C-o>", "%<C-o>", "gg<C-o>", "3G''", "3G``", "``", "2G3G``",
    ];
    v.extend(mov_basico());
    v
}

fn vocabulario_pct() -> Vec<&'static str> {
    let mut v = vec!["d%", "y%", "c%X<Esc>", "%", "d%.", "2d%", "d2%", "df(", "d%p", "y%P", "v%d", "V%d", "dt)", "f(d%", "f{d%", "f[d%", "f<d%", "$d%", "0d%", "wd%", "fo%", "d%u", "c%X<Esc>.", "y%`]"];
    v.extend(mov_basico());
    v
}

fn vocabulario(nome: &str) -> Vec<&'static str> {
    match nome {
        "ca" => return vec!["<C-a>", "}", "j", "x", "u", "<C-x>", "w", "."],
        "falha" => return vec!["fz", "fz", "x", "l", "dd", "w", "<C-a>", "j", "u", "tz", "Fz", "p", "3x", "yy"],
        "num" => return vocabulario_num(),
        "caso" => return vocabulario_caso(),
        "ponto" => return vocabulario_ponto_visual(),
        "reg" => return vocabulario_registros(),
        "marc2" => return vocabulario_marcas2(),
        "pct" => return vocabulario_pct(),
        "blk" => return vocabulario_bloco(),
        "ins" => return vocabulario_ins(),
        "ex" => return vocabulario_ex(),
        "mac" => return vocabulario_macro(),
        "marc" => return vocabulario_marcas(),
        "vp" => return vocabulario_vp(),
        _ => {}
    }
    if nome == "bus" {
        return vocabulario_busca();
    }
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
        vim.o.more = false
        vim.o.shortmess = vim.o.shortmess .. 'sSI'
        local tmp = vim.fn.tempname()
        vim.fn.writefile(lines, tmp)
        vim.cmd('silent edit! ' .. vim.fn.fnameescape(tmp))
        vim.api.nvim_win_set_cursor(0, { 1, 0 })
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
    let (nvim, _io, mut child) = create::new_child_cmd(&mut cmd, Nada).await.ok()?;
    let args = vec![Value::from(lines.iter().map(|l| Value::from(*l)).collect::<Vec<_>>()), Value::from(keys)];
    // um caso que trava o nvim (pergunta, prompt) não pode travar o teste
    let r = tokio::time::timeout(std::time::Duration::from_secs(4), nvim.exec_lua(code, args)).await.ok().and_then(|r| r.ok());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), nvim.command("qa!")).await;
    let _ = child.kill().await;
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
    let reg = ed
        .register('"')
        .map(|r| {
            let tipo = if r.block {
                format!("\u{16}{}", r.bwidth)
            } else if r.linewise {
                "V".to_string()
            } else {
                "v".to_string()
            };
            format!("{}|{}", if r.linewise { format!("{}\n", r.text) } else { r.text.clone() }, tipo)
        })
        .unwrap_or_else(|| "|v".into());
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

/// Sem `@` dentro de uma gravação aberta: macro que chama a si mesmo não termina (nem no nvim).
fn sem_recursao(toks: &[&str]) -> bool {
    let mut gravando = false;
    for t in toks {
        let completo = t.len() > 2 && t.starts_with('q') && t.ends_with('q');
        if *t == "q" {
            gravando = false;
        } else if t.len() == 2 && t.starts_with('q') {
            gravando = true;
        } else if !completo && t.contains('@') && gravando {
            return false;
        }
        if completo {
            continue;
        }
    }
    true
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
        let mut toks: Vec<&str> = (0..n).map(|_| *rng.pick(&voc)).collect();
        while !sem_recursao(&toks) {
            toks = (0..n).map(|_| *rng.pick(&voc)).collect();
        }
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
