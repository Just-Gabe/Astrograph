//! Ponta a ponta dos textobjects no app: `nvim --embed` com a config instalada,
//! o realce/indent/textobjects em Rust ligados como no app, e uma conferencia
//! dentro do nvim: o que o plugin devolve pelo app (`b:astrograph_textobjects`)
//! contra o que ele devolve sozinho, e o cursor de `]f`/`[f`/`]a` e a selecao de
//! `vaf`/`dif` com e sem o app. Sai com codigo 1 se houver diferenca.
//!
//!   cargo run --release --example textobj_app_e2e -- <arquivo>
//! (precisa de HOME com a config e o nvim-treesitter-textobjects instalados)

#[path = "../src/realce.rs"]
mod realce;

use std::time::{Duration, Instant};

use nvim_rs::compat::tokio::Compat;
use nvim_rs::create::tokio as create;
use nvim_rs::{Handler, Neovim, Value};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

#[derive(Clone)]
struct Fwd(UnboundedSender<realce::Event>, realce::Shared);

#[async_trait::async_trait]
impl Handler for Fwd {
    type Writer = Compat<ChildStdin>;
    async fn handle_notify(&self, name: String, args: Vec<Value>, _n: Neovim<Self::Writer>) {
        if let Some(ev) = realce::parse_notify(&name, &args) {
            let _ = self.0.send(ev);
        }
    }
    async fn handle_request(&self, name: String, args: Vec<Value>, _n: Neovim<Self::Writer>) -> Result<Value, Value> {
        match name.as_str() {
            "astrograph_indent" => Ok(realce::indent(&self.1, &args).await),
            "astrograph_textobject" => Ok(realce::textobject(&self.1, &args).await),
            "astrograph_textobject_ranges" => Ok(realce::textobject_ranges(&self.1, &args).await),
            _ => Err(Value::from("desconhecido")),
        }
    }
}

const CHECK: &str = r#"
local shared = require("nvim-treesitter-textobjects.shared")
local ch = vim.b.astrograph_textobjects
if not ch then return { error = "buffer sem a marca do app" } end
local buf = vim.api.nvim_get_current_buf()
local function without(fn)
  local saved = vim.b.astrograph_textobjects
  vim.b.astrograph_textobjects = nil
  local ok, a, b = pcall(fn)
  vim.b.astrograph_textobjects = saved
  if not ok then error(a) end
  return a, b
end
local function key(r) return r and table.concat(r, ",") or "nil" end
local n, diffs, first = 0, 0, {}
local total = vim.api.nvim_buf_line_count(buf)
local step = math.max(1, math.floor(total / 60))
local t_app, t_nv, calls = 0, 0, 0
for _, q in ipairs { "@function.outer", "@function.inner", "@block.outer", "@parameter.inner", "@conditional.outer" } do
  for row = 1, total, step do
    for _, opts in ipairs { { lookahead = true }, {} } do
      local pos = { row, 0 }
      local t0 = vim.uv.hrtime()
      local a = shared.textobject_at_point(q, "textobjects", buf, pos, opts)
      t_app = t_app + (vim.uv.hrtime() - t0)
      t0 = vim.uv.hrtime()
      local b = without(function() return shared.textobject_at_point(q, "textobjects", buf, pos, opts) end)
      t_nv = t_nv + (vim.uv.hrtime() - t0)
      calls = calls + 1
      n = n + 1
      if key(a) ~= key(b) then
        diffs = diffs + 1
        if #first < 3 then first[#first + 1] = ("%s %d: app=%s plugin=%s"):format(q, row, key(a), key(b)) end
      end
    end
  end
end
-- teclas de verdade (mapeamentos do astrocore): cursor depois de ]f [f ]a ]k e selecao de vaf/dif
local kn, kd = 0, 0
local function run_keys(keys)
  vim.cmd("normal! " .. "gg")
  local out = {}
  for row = 1, total, step * 2 do
    vim.api.nvim_win_set_cursor(0, { row, 0 })
    local tick = vim.b.changedtick
    vim.cmd("normal " .. keys)
    local changed = vim.b.changedtick ~= tick
    if changed then out[#out + 1] = "texto:" .. table.concat(vim.api.nvim_buf_get_lines(0, math.max(0, row - 3), row + 3, false), "|") end
    if changed then vim.cmd("silent! undo") ; vim.cmd("sleep 30m") end
    out[#out + 1] = vim.api.nvim_win_get_cursor(0)[1] .. ":" .. vim.api.nvim_win_get_cursor(0)[2] .. ":" .. vim.fn.mode()
    vim.cmd("normal! \27")
  end
  return out
end
for _, keys in ipairs { "]f", "[f", "]a", "[a", "]k", "[K", "vaf\27", "vif\27", "daa", ">F" } do
  local a = run_keys(keys)
  local b = without(function() return run_keys(keys) end)
  for i = 1, math.max(#a, #b) do
    kn = kn + 1
    if a[i] ~= b[i] then
      kd = kd + 1
      if #first < 6 then first[#first + 1] = ("tecla %s #%d: app=%s plugin=%s"):format(keys, i, a[i], b[i]) end
    end
  end
end
return { n = n, diffs = diffs, kn = kn, kd = kd, first = first, t_app = t_app / 1e3 / calls, t_nv = t_nv / 1e3 / calls }
"#;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let file = std::env::args().nth(1).expect("arquivo");
    let (tx, rx) = unbounded_channel();
    let estado = realce::Shared::default();
    let mut cmd = Command::new(std::env::var("NVIM_BIN").unwrap_or_else(|_| "nvim".into()));
    cmd.args(["--embed", "--headless", "-n"]);
    let (nvim, _io, _child) = create::new_child_cmd(&mut cmd, Fwd(tx, estado.clone())).await.expect("nvim");
    tokio::spawn(realce::run(nvim.clone(), estado.clone(), rx));
    tokio::time::sleep(Duration::from_millis(4000)).await;
    nvim.command(&format!("edit {}", file.replace(' ', "\\ "))).await.unwrap();
    // espera a marca do app (a copia do buffer chegou)
    let t = Instant::now();
    loop {
        let m = nvim.exec_lua("return vim.b.astrograph_textobjects ~= nil", vec![]).await.unwrap();
        if m.as_bool() == Some(true) || t.elapsed() > Duration::from_secs(6) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let r = nvim.exec_lua(CHECK, vec![]).await.expect("conferencia");
    let get = |k: &str| {
        r.as_map()
            .and_then(|m| m.iter().find(|(a, _)| a.as_str() == Some(k)).map(|(_, v)| v.clone()))
            .unwrap_or(Value::Nil)
    };
    if let Some(e) = get("error").as_str() {
        println!("{file}: {e}");
        std::process::exit(1);
    }
    let (n, d, kn, kd) = (get("n").as_i64().unwrap(), get("diffs").as_i64().unwrap(), get("kn").as_i64().unwrap(), get("kd").as_i64().unwrap());
    println!(
        "{file}: objetos {}/{} iguais; teclas {}/{} iguais; por consulta: app {:.0} us, plugin {:.0} us",
        n - d,
        n,
        kn - kd,
        kn,
        get("t_app").as_f64().unwrap_or(0.0),
        get("t_nv").as_f64().unwrap_or(0.0)
    );
    if let Some(l) = get("first").as_array() {
        for s in l {
            println!("    {}", s.as_str().unwrap_or(""));
        }
    }
    let _ = nvim.command("qa!").await;
    if d > 0 || kd > 0 {
        std::process::exit(1);
    }
}
