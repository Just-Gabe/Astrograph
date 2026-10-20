//! Prova de ponta a ponta do realce em Rust (ROADMAP, S1) num `nvim --embed`:
//! abre um arquivo Lua real, confere os extmarks e o realce nativo desligado,
//! digita uma linha e mede a espera ate o realce chegar, e compara a cobertura
//! com o treesitter nativo do Neovim no mesmo arquivo.
//!
//!   cargo run --release --example realce_e2e -- <arquivo.lua>

#[path = "../src/realce.rs"]
mod realce;

use std::time::{Duration, Instant};

use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, Value};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[derive(Clone)]
struct Fwd(UnboundedSender<realce::Event>, realce::Shared, UnboundedSender<Instant>);

#[async_trait::async_trait]
impl Handler for Fwd {
    type Writer = Compat<ChildStdin>;
    async fn handle_notify(&self, name: String, args: Vec<Value>, _n: Neovim<Self::Writer>) {
        if name == "e2e_fim" {
            let _ = self.2.send(Instant::now());
            return;
        }
        if let Some(ev) = realce::parse_notify(&name, &args) {
            let _ = self.0.send(ev);
        }
    }
    async fn handle_request(&self, name: String, args: Vec<Value>, _n: Neovim<Self::Writer>) -> Result<Value, Value> {
        match name.as_str() {
            "astrograph_indent" => Ok(realce::indent(&self.1, &args).await),
            _ => Err(Value::from("desconhecido")),
        }
    }
}

/// Digita uma funcao com `if` no fim do arquivo (o indent de cada linha nova
/// vem da indentexpr) e devolve as linhas digitadas e o tempo.
async fn digitar(nvim: &Neovim<Compat<ChildStdin>>, fim: &mut UnboundedReceiver<Instant>) -> (Vec<String>, Duration) {
    nvim.command("normal! G").await.unwrap();
    let chan = nvim.get_api_info().await.unwrap()[0].as_i64().unwrap();
    let t = Instant::now();
    // o fim e marcado pelo proprio nvim, depois de processar todas as teclas
    // (o get_mode responde antes; outras consultas podem rodar dentro da
    // espera da indentexpr)
    nvim.input(&format!(
        "olocal function teste(a)<CR>if a then<CR>return {{<CR>1,<CR>}}<CR>end<CR>end<Esc><Cmd>call rpcnotify({chan}, 'e2e_fim')<CR>"
    ))
    .await
    .unwrap();
    let took = fim.recv().await.unwrap() - t;
    let v = lua(nvim, "return vim.api.nvim_buf_get_lines(0, -8, -1, false)").await;
    let lines = v.as_array().unwrap().iter().map(|l| l.as_str().unwrap_or("").to_string()).collect();
    (lines, took)
}

async fn lua(nvim: &Neovim<Compat<ChildStdin>>, code: &str) -> Value {
    nvim.exec_lua(code, vec![]).await.expect(code)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let file = std::env::args().nth(1).expect("arquivo .lua");
    let (tx, rx) = unbounded_channel();
    let estado = realce::Shared::default();
    let (fim_tx, mut fim_rx) = unbounded_channel();
    let mut cmd = Command::new(std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".into()));
    // E2E_CONFIG=1: com a config instalada (astrocore, nvim-treesitter...)
    if std::env::var("E2E_CONFIG").is_ok() {
        cmd.args(["--embed", "--headless", "-n"]);
    } else {
        cmd.args(["--embed", "--headless", "--clean", "-n"]);
    }
    let (nvim, _io, _child) = create::new_child_cmd(&mut cmd, Fwd(tx, estado.clone(), fim_tx)).await.expect("nvim");
    std::env::set_var("ASTROGRAPH_REALCE_TEMPO", "1");
    // E2E_NATIVO=1: referencia com o realce nativo do Neovim, sem o Rust
    let nativo_ref = std::env::var("E2E_NATIVO").is_ok();
    if !nativo_ref {
        tokio::spawn(realce::run(nvim.clone(), estado.clone(), rx));
    }
    // com a config, espera ela terminar de carregar (lazy, VeryLazy...)
    let espera = if std::env::var("E2E_CONFIG").is_ok() { 4000 } else { 200 };
    tokio::time::sleep(Duration::from_millis(espera)).await;

    nvim.command(&format!("edit {}", file.replace(' ', "\\ "))).await.unwrap();
    if std::env::var("E2E_DIGITAR").is_ok() {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let expr = lua(&nvim, "return vim.bo.indentexpr").await;
        let (lines, took) = digitar(&nvim, &mut fim_rx).await;
        println!("indentexpr: {expr}");
        println!("digitado em {:.1} ms:", took.as_secs_f64() * 1e3);
        for l in lines {
            println!("|{l}");
        }
        for (espera, calculo) in realce::tempos_indent() {
            println!("indent: espera {:.2} ms, calculo {:.2} ms", espera.as_secs_f64() * 1e3, calculo.as_secs_f64() * 1e3);
        }
        let _ = nvim.command("qa!").await;
        return;
    }
    if nativo_ref {
        // quanto o loop do nvim fica ocupado depois do :edit (plugins de BufRead)
        let t = Instant::now();
        let mut livre = Duration::ZERO;
        while t.elapsed() < Duration::from_millis(600) {
            let c = Instant::now();
            nvim.get_current_line().await.unwrap();
            if c.elapsed() > Duration::from_millis(5) {
                livre = t.elapsed();
            }
        }
        println!("nativo: nvim respondeu devagar (>5 ms) ate {:.1} ms depois do :edit", livre.as_secs_f64() * 1e3);
        nativo(&nvim).await;
        return;
    }
    // espera o primeiro realce do arquivo inteiro
    let count = "local ns = vim.api.nvim_get_namespaces().astrograph_realce; \
                 if not ns then return 0 end; \
                 return #vim.api.nvim_buf_get_extmarks(0, ns, 0, -1, {})";
    let t = Instant::now();
    let mut marks = 0;
    while t.elapsed() < Duration::from_secs(5) {
        marks = lua(&nvim, count).await.as_i64().unwrap_or(0);
        if marks > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    println!("primeiro realce: {marks} extmarks em {:.1} ms (desde o :edit)", t.elapsed().as_secs_f64() * 1e3);
    assert!(marks > 0, "nenhum extmark");

    let native = lua(
        &nvim,
        "return { ts = vim.treesitter.highlighter.active[vim.api.nvim_get_current_buf()] ~= nil, syntax = vim.bo.syntax }",
    )
    .await;
    println!("realce nativo depois do Rust: {native}");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let later = lua(
        &nvim,
        "return { ts = vim.treesitter.highlighter.active[vim.api.nvim_get_current_buf()] ~= nil, syntax = vim.bo.syntax }",
    )
    .await;
    println!("realce nativo 1,5 s depois: {later}");

    // digitar: nova linha com comentario, mede ate o extmark @comment chegar
    let has_comment = "local ns = vim.api.nvim_get_namespaces().astrograph_realce; \
                       local r = vim.fn.line('$') - 1; \
                       for _, m in ipairs(vim.api.nvim_buf_get_extmarks(0, ns, {r, 0}, {r, -1}, {details = true})) do \
                         if m[4].hl_group == '@comment.lua' then return true end end; return false";
    let mut waits = Vec::new();
    for i in 0..30 {
        nvim.command("normal! G").await.unwrap();
        let t = Instant::now();
        nvim.input(&format!("o-- nota {i}<Esc>")).await.unwrap();
        loop {
            if lua(&nvim, has_comment).await.as_bool() == Some(true) {
                break;
            }
            assert!(t.elapsed() < Duration::from_secs(2), "realce da linha nova nao chegou");
        }
        waits.push(t.elapsed());
    }
    waits.sort();
    println!(
        "linha digitada -> realce: mediana {:.2} ms, p90 {:.2} ms (n={}; inclui as consultas de conferencia)",
        waits[waits.len() / 2].as_secs_f64() * 1e3,
        waits[waits.len() * 9 / 10].as_secs_f64() * 1e3,
        waits.len()
    );

    println!("lotes de realce recusados pelo nvim: {}", realce::ERROS.load(std::sync::atomic::Ordering::Relaxed));
    let ti = realce::tempos_indent();
    let lentos = ti.iter().filter(|(e, _)| e.as_millis() >= 5).count();
    println!(
        "pedidos de indent no laco: {} (espera >= 5 ms: {lentos}; maior espera {:.1} ms)",
        ti.len(),
        ti.iter().map(|(e, _)| e.as_secs_f64() * 1e3).fold(0.0, f64::max)
    );
    let mut t = realce::tempos();
    t.sort();
    println!(
        "custo do realce em Rust por lote (evento -> extmarks aplicados): mediana {:.2} ms, p90 {:.2} ms, max {:.2} ms (n={}; o 1o e o arquivo inteiro)",
        t[t.len() / 2].as_secs_f64() * 1e3,
        t[t.len() * 9 / 10].as_secs_f64() * 1e3,
        t.last().unwrap().as_secs_f64() * 1e3,
        t.len()
    );

    // o nvim atende pedidos enquanto espera uma resposta da indentexpr; a
    // cobertura (que edita um buffer) so roda com ele livre
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = nvim.get_mode().await;

    // cobertura x treesitter nativo: bytes que o nativo colore e nos tambem
    let cover = lua(
        &nvim,
        &format!(
            "local ns = vim.api.nvim_get_namespaces().astrograph_realce
             local ours = vim.api.nvim_get_current_buf()
             local lines = vim.api.nvim_buf_get_lines(ours, 0, -1, false)
             local ref = vim.api.nvim_create_buf(false, true)
             vim.api.nvim_buf_set_lines(ref, 0, -1, false, lines)
             vim.treesitter.start(ref, 'lua'); vim.treesitter.get_parser(ref, 'lua'):parse(true)
             local both, nat, mine, same, diff = 0, 0, 0, 0, {{}}
             for r = 0, #lines - 1 do
               local marks = vim.api.nvim_buf_get_extmarks(ours, ns, {{r, 0}}, {{r, -1}}, {{details = true}})
               for c = 0, #lines[r + 1] - 1 do
                 local caps = vim.treesitter.get_captures_at_pos(ref, r, c)
                 local n = nil
                 for _, cap in ipairs(caps) do if not cap.capture:match('^_') and cap.capture ~= 'spell' and cap.capture ~= 'nospell' then n = cap.capture end end
                 local o = nil
                 for _, m in ipairs(marks) do if m[3] <= c and c < m[4].end_col then o = m[4].hl_group end end
                 if n then nat = nat + 1 end
                 if o then mine = mine + 1 end
                 if n and o then both = both + 1; if o:gsub('%.[%w_]+$', '') == '@' .. n then same = same + 1 else local k = n .. ' -> ' .. o; diff[k] = (diff[k] or 0) + 1 end end
               end
             end
             local top = {{}}; for k, v in pairs(diff) do top[#top + 1] = {{k, v}} end; table.sort(top, function(x, y) return x[2] > y[2] end); local t = {{}}; for i = 1, math.min(8, #top) do t[i] = top[i][1] .. ' = ' .. top[i][2] end; return {{ nativo = nat, rust = mine, ambos = both, mesmo_grupo = same, diferentes = t }}"
        ),
    )
    .await;
    println!("cobertura (bytes): {cover}");
    let _ = nvim.command("qa!").await;
}

/// Referencia: o mesmo arquivo e a mesma digitacao com o realce nativo do
/// Neovim (sem o Rust). Mede ate o highlighter nativo estar ativo e ate a linha
/// digitada ter a captura de comentario (redraw forcado: o nativo realca nele).
async fn nativo(nvim: &Neovim<Compat<ChildStdin>>) {
    let t = Instant::now();
    let active = "return vim.treesitter.highlighter.active[vim.api.nvim_get_current_buf()] ~= nil";
    while lua(nvim, active).await.as_bool() != Some(true) {
        if t.elapsed() > Duration::from_secs(5) {
            println!("nativo: highlighter nao ligou em 5 s (sem parser/config?)");
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    println!("nativo: highlighter ativo em {:.1} ms (desde o :edit)", t.elapsed().as_secs_f64() * 1e3);
    let probe = "vim.cmd.redraw(); local b = vim.api.nvim_get_current_buf(); local r = vim.fn.line('$') - 1; \
                 for _, c in ipairs(vim.treesitter.get_captures_at_pos(b, r, 0)) do \
                   if c.capture == 'comment' then return true end end; return false";
    let mut waits = Vec::new();
    for i in 0..30 {
        nvim.command("normal! G").await.unwrap();
        let t = Instant::now();
        nvim.input(&format!("o-- nota {i}<Esc>")).await.unwrap();
        while lua(nvim, probe).await.as_bool() != Some(true) {
            assert!(t.elapsed() < Duration::from_secs(2), "linha nova sem realce nativo");
        }
        waits.push(t.elapsed());
    }
    waits.sort();
    println!(
        "nativo: linha digitada -> realce: mediana {:.2} ms, p90 {:.2} ms (n={})",
        waits[waits.len() / 2].as_secs_f64() * 1e3,
        waits[waits.len() * 9 / 10].as_secs_f64() * 1e3,
        waits.len()
    );
    let _ = nvim.command("qa!").await;
}
