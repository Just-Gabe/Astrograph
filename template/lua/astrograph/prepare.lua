-- Preparo em segundo plano: baixa, compila e aquece o que o projeto aberto vai
-- precisar, sem interromper o uso nem poluir a tela.
--
-- Roda uma vez depois do startup e de novo a cada troca de pasta (DirChanged):
--   * detecta o projeto (Cargo.toml, package.json, pyproject.toml, go.mod, lua...)
--     e daí os parsers do Treesitter e os servidores do Mason que ele pede;
--   * compila os parsers que faltam e instala os pacotes do Mason (sem avisos);
--   * se há git, roda os comandos de leitura dele (status, HEAD, refs...) para o
--     índice e o cache já estarem quentes, sem travar o repositório do usuário
--     (`--no-optional-locks`, nunca `fetch`);
--   * compila o agente nativo e o app em Rust quando faltam e há `cargo`.
--
-- Tudo assíncrono. O que demorar mais de ~0,4 s mostra um ícone de carregamento
-- pequeno no canto inferior direito, que some quando acaba. Desligue com
-- `vim.g.astrograph_prepare = false`; só os pacotes do Mason de linguagem com
-- `vim.g.astrograph_prepare_mason = false`; a compilação em Rust com
-- `vim.g.astrograph_prepare_native = false`. `:Astrograph preparo` mostra o estado.

local M = {}

local uv = vim.uv or vim.loop

M.SHOW_AFTER = 400 -- ms ocupado antes de aparecer o ícone (evita piscar)
M.TIMEOUT = 10 * 60 * 1000 -- ms: tarefa que não termina deixa de segurar o ícone
M.FRAMES = { "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏" }

---@class AstrographPrepareState
---@field root? string
---@field langs string[]           linguagens detectadas no projeto
---@field git? table|false         nil = fora de repo; false = git não instalado
---@field done table<string, boolean>

---@type AstrographPrepareState
M.state = { langs = {}, done = {} }

---------------------------------------------------------------------------
-- Ocupado / ícone
---------------------------------------------------------------------------

local busy = {} ---@type table<string, integer>
local ind = { win = nil, buf = nil, frame = 1, timer = nil, show_timer = nil }

local function has_busy() return next(busy) ~= nil end

local function close_indicator()
  if ind.timer then
    ind.timer:stop()
    ind.timer:close()
    ind.timer = nil
  end
  if ind.win and vim.api.nvim_win_is_valid(ind.win) then pcall(vim.api.nvim_win_close, ind.win, true) end
  ind.win = nil
end

---Canto inferior direito, logo acima da linha de status.
local function place()
  local status = vim.o.laststatus > 0 and 1 or 0
  return { relative = "editor", row = math.max(0, vim.o.lines - vim.o.cmdheight - status - 1), col = math.max(0, vim.o.columns - 3) }
end

local function draw()
  if not has_busy() then return close_indicator() end
  if not (ind.buf and vim.api.nvim_buf_is_valid(ind.buf)) then
    ind.buf = vim.api.nvim_create_buf(false, true)
    vim.bo[ind.buf].bufhidden = "wipe"
  end
  ind.frame = ind.frame % #M.FRAMES + 1
  vim.api.nvim_buf_set_lines(ind.buf, 0, -1, false, { " " .. M.FRAMES[ind.frame] })
  vim.api.nvim_buf_clear_namespace(ind.buf, -1, 0, -1)
  vim.api.nvim_buf_set_extmark(ind.buf, vim.api.nvim_create_namespace "astrograph_prepare", 0, 1, {
    end_col = #M.FRAMES[ind.frame] + 1,
    hl_group = "AstrographPrepare",
  })
  local pos = place()
  if ind.win and vim.api.nvim_win_is_valid(ind.win) then
    vim.api.nvim_win_set_config(ind.win, pos)
  else
    ind.win = vim.api.nvim_open_win(
      ind.buf,
      false,
      vim.tbl_extend("force", pos, {
        width = 3,
        height = 1,
        style = "minimal",
        border = "none", -- sem herdar o `winborder` do usuário
        focusable = false,
        zindex = 250,
        noautocmd = true,
      })
    )
    vim.wo[ind.win].winhighlight = "NormalFloat:Normal"
  end
end

local function open_indicator()
  if ind.timer then return end
  vim.api.nvim_set_hl(0, "AstrographPrepare", { link = "DiagnosticInfo", default = true })
  ind.timer = uv.new_timer()
  ind.timer:start(0, 100, vim.schedule_wrap(function() pcall(draw) end))
end

local function refresh()
  if has_busy() then
    if ind.timer or ind.show_timer then return end
    ind.show_timer = uv.new_timer()
    ind.show_timer:start(M.SHOW_AFTER, 0, vim.schedule_wrap(function()
      ind.show_timer:close()
      ind.show_timer = nil
      if has_busy() then open_indicator() end
    end))
  else
    if ind.show_timer then
      ind.show_timer:stop()
      ind.show_timer:close()
      ind.show_timer = nil
    end
    close_indicator()
  end
end

---Marca `name` como ocupado e devolve a função que libera (idempotente).
---Se ela não for chamada em `M.TIMEOUT`, libera sozinha.
---@param name string
---@return fun() done
function M.track(name)
  busy[name] = (busy[name] or 0) + 1
  refresh()
  local released = false
  local function release()
    if released then return end
    released = true
    busy[name] = (busy[name] or 1) - 1
    if busy[name] <= 0 then busy[name] = nil end
    refresh()
  end
  vim.defer_fn(release, M.TIMEOUT)
  return release
end

---@return string[] nomes das tarefas em andamento
function M.running()
  local out = vim.tbl_keys(busy)
  table.sort(out)
  return out
end

---------------------------------------------------------------------------
-- Projeto
---------------------------------------------------------------------------

-- arquivo/pasta que revela a linguagem -> parsers do Treesitter
local MARKERS = {
  { { "Cargo.toml" }, { "rust", "toml" } },
  { { "package.json" }, { "javascript", "json" } },
  { { "tsconfig.json" }, { "typescript", "json" } },
  { { "pyproject.toml", "requirements.txt", "setup.py", "Pipfile" }, { "python", "toml" } },
  { { "go.mod" }, { "go", "gomod" } },
  { { ".luarc.json", "stylua.toml", ".stylua.toml", "init.lua", "lua" }, { "lua" } },
  { { ".github", "docker-compose.yml", "docker-compose.yaml" }, { "yaml" } },
  { { "Makefile", "justfile" }, { "bash", "make" } },
}

-- linguagem -> pacotes do Mason (servidor/formatador) que o projeto costuma pedir
local MASON = {
  rust = { "rust-analyzer" },
  python = { "basedpyright" },
  javascript = { "typescript-language-server" },
  typescript = { "typescript-language-server" },
  go = { "gopls" },
  json = { "json-lsp" },
  yaml = { "yaml-language-server" },
  toml = { "taplo" },
  bash = { "bash-language-server" },
}

-- parsers pequenos que valem em qualquer repositório git
local GIT_LANGS = { "diff", "git_config", "git_rebase", "gitattributes", "gitcommit", "gitignore" }

---Linguagens do projeto em `root`, olhando só a raiz (barato; sem varrer pastas).
---@param root string
---@return string[]
function M.detect(root)
  local seen, out = {}, {}
  local function add(l)
    if not seen[l] then
      seen[l] = true
      out[#out + 1] = l
    end
  end
  for _, m in ipairs(MARKERS) do
    for _, file in ipairs(m[1]) do
      if uv.fs_stat(vim.fs.joinpath(root, file)) then
        for _, l in ipairs(m[2]) do
          add(l)
        end
        break
      end
    end
  end
  return out
end

---Pacotes do Mason para um conjunto de linguagens (sem repetir).
---@param langs string[]
---@return string[]
function M.mason_packages(langs)
  local seen, out = {}, {}
  for _, l in ipairs(langs) do
    for _, p in ipairs(MASON[l] or {}) do
      if not seen[p] then
        seen[p] = true
        out[#out + 1] = p
      end
    end
  end
  return out
end

---Pastas onde varrer ou rodar git não vale a pena (home, raiz do disco).
local function too_broad(root)
  local r = vim.fs.normalize(root):lower():gsub("/+$", "")
  local home = vim.fs.normalize(vim.fn.expand "~"):lower():gsub("/+$", "")
  return r == "" or r == home or r:match "^%a:$" ~= nil
end

---------------------------------------------------------------------------
-- Git
---------------------------------------------------------------------------

local GIT_ENV = { GIT_TERMINAL_PROMPT = "0", GIT_OPTIONAL_LOCKS = "0" }

---Roda os comandos de leitura do git em `root` (em paralelo) e guarda o resumo
---em `M.state.git`. Nada que escreva no repositório nem fale com a rede.
---@param root string
---@param cb? fun()
function M.git(root, cb)
  cb = cb or function() end
  if vim.fn.executable "git" ~= 1 then
    M.state.git = false
    return cb()
  end
  local function git(args, on)
    local cmd = { "git", "-C", root, "--no-optional-locks" }
    vim.list_extend(cmd, args)
    local ok = pcall(vim.system, cmd, { text = true, env = GIT_ENV }, on)
    if not ok then on { code = 1, stdout = "", stderr = "" } end
  end
  git({ "rev-parse", "--show-toplevel" }, function(top)
    if top.code ~= 0 then
      return vim.schedule(function()
        M.state.git = nil
        cb()
      end)
    end
    local info = { toplevel = vim.trim(top.stdout or "") }
    local pending = 4
    local function finish()
      pending = pending - 1
      if pending == 0 then
        vim.schedule(function()
          M.state.git = info
          cb()
        end)
      end
    end
    git({ "status", "--porcelain=v1", "-b" }, function(r)
      local lines = vim.split(r.stdout or "", "\n", { trimempty = true })
      local head = table.remove(lines, 1) or ""
      info.branch = head:match "^## No commits yet on (%S+)" or head:match "^## Initial commit on (%S+)" or head:match "^## ([^%.%s]+)"
      info.dirty = #lines
      finish()
    end)
    git({ "rev-parse", "--short", "HEAD" }, function(r)
      info.head = r.code == 0 and vim.trim(r.stdout or "") or nil
      finish()
    end)
    git({ "for-each-ref", "--count=500", "--format=%(refname)", "refs/heads", "refs/remotes" }, function(r)
      info.refs = #vim.split(r.stdout or "", "\n", { trimempty = true })
      finish()
    end)
    git({ "log", "-1", "--format=%ct" }, function(r)
      info.last_commit = tonumber(vim.trim(r.stdout or ""))
      finish()
    end)
  end)
end

---------------------------------------------------------------------------
-- Treesitter e Mason
---------------------------------------------------------------------------

---@param langs string[]
---@param cb fun()
local function treesitter(langs, cb)
  local ok_mod, ts = pcall(require, "astrograph.treesitter")
  if not ok_mod then return cb() end
  local ok = pcall(ts.install_quiet, langs, cb)
  if not ok then cb() end
end

---@param pkgs string[]
---@param cb fun()
local function mason(pkgs, cb)
  if #pkgs == 0 or vim.g.astrograph_prepare_mason == false then return cb() end
  local ok, registry = pcall(require, "mason-registry")
  if not ok then return cb() end
  local function install()
    local left = 1 -- a trava de quem lança: só termina depois do laço
    local function one_done()
      left = left - 1
      if left == 0 then cb() end
    end
    for _, name in ipairs(pkgs) do
      pcall(function()
        if not registry.has_package(name) then return end
        local pkg = registry.get_package(name)
        if pkg:is_installed() or pkg:is_installing() then return end
        left = left + 1
        local handle = pkg:install()
        handle:once("closed", vim.schedule_wrap(one_done))
      end)
    end
    one_done()
  end
  -- sem rede o registro não atualiza: instala só o que ele já souber
  local called = false
  local ok_refresh = pcall(registry.refresh, function()
    if called then return end
    called = true
    vim.schedule(install)
  end)
  if not ok_refresh then install() end
end

---------------------------------------------------------------------------
-- Rust (agente nativo e app)
---------------------------------------------------------------------------

local function exe(name) return name .. (vim.fn.has "win32" == 1 and ".exe" or "") end

local NATIVE = {
  { crate = "astrograph-agent", bin = "astrograph-agent" },
  { crate = "astrograph-ui", bin = "astrograph" },
}

---Compila, um por vez e com prioridade baixa, o que faltar em `<clone>/bin`.
---Uma falha fica anotada (por commit) para não tentar de novo a cada abertura.
---@param cb fun()
function M.native(cb)
  if vim.g.astrograph_prepare_native == false or vim.fn.executable "cargo" ~= 1 then return cb() end
  local root = require("astrograph.commands").root()
  if not root then return cb() end
  local commit = vim.trim(vim.fn.system { "git", "-C", root, "rev-parse", "--short", "HEAD" })
  local failed_dir = vim.fs.joinpath(vim.fn.stdpath "state", "astrograph-build")
  local queue = {}
  for _, n in ipairs(NATIVE) do
    local out = vim.fs.joinpath(root, "bin", exe(n.bin))
    local manifest = vim.fs.joinpath(root, "prototype", n.crate, "Cargo.toml")
    local mark = vim.fs.joinpath(failed_dir, n.crate)
    local failed = vim.fn.filereadable(mark) == 1 and vim.fn.readfile(mark)[1] or nil
    if not uv.fs_stat(out) and uv.fs_stat(manifest) and failed ~= commit then
      queue[#queue + 1] = { n = n, out = out, manifest = manifest, mark = mark }
    end
  end
  local function step()
    local job = table.remove(queue, 1)
    if not job then return cb() end
    local cmd = { "cargo", "build", "--release", "--manifest-path", job.manifest }
    if vim.fn.has "win32" ~= 1 and vim.fn.executable "nice" == 1 then cmd = vim.list_extend({ "nice", "-n", "19" }, cmd) end
    local ok = pcall(vim.system, cmd, { text = true }, function(r)
      vim.schedule(function()
        local built = vim.fs.joinpath(vim.fs.dirname(job.manifest), "target", "release", exe(job.n.crate))
        if r.code == 0 and uv.fs_stat(built) then
          vim.fn.mkdir(vim.fs.dirname(job.out), "p")
          uv.fs_copyfile(built, job.out)
        else
          vim.fn.mkdir(failed_dir, "p")
          vim.fn.writefile({ commit }, job.mark)
        end
        step()
      end)
    end)
    if not ok then step() end
  end
  step()
end

---------------------------------------------------------------------------
-- Orquestração
---------------------------------------------------------------------------

---Prepara o projeto em `root` (no máximo uma vez por pasta e por sessão, salvo `force`).
---@param root? string  padrão: pasta atual
---@param force? boolean
---@param extra? string[]  parsers a mais (os do `ensure_installed`, na abertura)
function M.project(root, force, extra)
  if vim.g.astrograph_prepare == false then return end
  root = vim.fs.normalize(root or uv.cwd() or vim.fn.getcwd())
  M.state.root = root
  local broad = too_broad(root)
  if broad then M.state.langs, M.state.git = {}, nil end
  local fresh = force or not M.state.done[root]
  M.state.done[root] = true

  local langs = broad and {} or M.detect(root)
  if not broad then M.state.langs = langs end
  local is_repo = not broad and uv.fs_stat(vim.fs.joinpath(root, ".git")) ~= nil

  -- git: sempre que entra na pasta (o estado muda), barato e sem travas
  if not broad then M.git(root, M.track "git") end

  if not fresh then return end
  local all = vim.list_extend(vim.list_extend({}, extra or {}), langs)
  if is_repo then vim.list_extend(all, GIT_LANGS) end
  local release_ts = M.track "parsers"
  vim.schedule(function() treesitter(all, release_ts) end)
  local pkgs = M.mason_packages(langs)
  if #pkgs > 0 then mason(pkgs, M.track "mason") end
end

---Parsers base + projeto + ferramentas + build nativo (uma vez, depois do startup).
function M.start()
  if vim.g.astrograph_prepare == false then return end
  local ok, ts = pcall(require, "astrograph.treesitter")
  local base_ok, base = false, nil
  if ok then base_ok, base = pcall(ts.ensure_installed) end
  local release_native = M.track "rust"
  vim.defer_fn(function() M.native(release_native) end, 2000)
  M.project(nil, nil, base_ok and base or {})
end

function M.setup()
  local group = vim.api.nvim_create_augroup("astrograph_prepare", { clear = true })
  vim.api.nvim_create_autocmd("User", {
    pattern = "VeryLazy",
    once = true,
    group = group,
    callback = function() vim.defer_fn(M.start, 800) end,
  })
  -- o mason-tool-installer instala por conta própria na abertura: o ícone cobre
  vim.api.nvim_create_autocmd("User", {
    pattern = { "MasonToolsStartingInstall" },
    group = group,
    callback = function()
      local release = M.track "mason-ferramentas"
      vim.api.nvim_create_autocmd("User", { pattern = "MasonToolsUpdateCompleted", once = true, group = group, callback = release })
    end,
  })
  local timer
  vim.api.nvim_create_autocmd("DirChanged", {
    group = group,
    desc = "Astrograph: preparar o projeto ao trocar de pasta",
    callback = function()
      if timer then timer:stop() end
      timer = timer or uv.new_timer()
      timer:start(300, 0, vim.schedule_wrap(function() M.project() end))
    end,
  })
  vim.api.nvim_create_autocmd("VimResized", { group = group, callback = function() if ind.win then pcall(draw) end end })
end

---Resumo para `:Astrograph preparo`.
---@return string
function M.report()
  local s = M.state
  local lines = { "Preparo em segundo plano" }
  lines[#lines + 1] = "  pasta: " .. (s.root or "(ainda não preparada)")
  lines[#lines + 1] = "  linguagens: " .. (#s.langs > 0 and table.concat(s.langs, ", ") or "(nenhuma detectada)")
  if s.git == false then
    lines[#lines + 1] = "  git: não instalado"
  elseif s.git then
    lines[#lines + 1] = ("  git: %s @ %s, %d alteração(ões), %d refs"):format(
      s.git.branch or "?",
      s.git.head or "?",
      s.git.dirty or 0,
      s.git.refs or 0
    )
  else
    lines[#lines + 1] = "  git: fora de um repositório"
  end
  local run = M.running()
  lines[#lines + 1] = "  em andamento: " .. (#run > 0 and table.concat(run, ", ") or "nada")
  return table.concat(lines, "\n")
end

return M
