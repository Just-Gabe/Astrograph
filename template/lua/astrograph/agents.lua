-- Núcleo compartilhado do modo agentic do Astrograph.
--
-- Detecta, de forma barata e tolerante a falhas, quais agentes/CLIs existem e se
-- há um Ollama local no ar. É consumido por:
--   * lua/plugins/codecompanion.lua  (HTTP + ACP + CLI)
--   * lua/plugins/avante.lua         (inline/chat local)
--   * lua/plugins/terminals.lua      (launcher de CLIs em terminal)
--
-- Nada aqui bloqueia o boot: a detecção é sob demanda e cacheada.

local M = {}

---@class AstrographAgent
---@field bin string        # executável no PATH
---@field label string
---@field cmd string        # comando base para o terminal / interação CLI
---@field login string      # comando interativo de login
---@field adapter string    # adapter ACP do CodeCompanion
---@field acp_bin string    # binário que fala ACP (ponte para o chat)
---@field acp_args? string[]
---@field acp_npm? string   # pacote npm da ponte, quando ela é separada do CLI
---@field npm string        # pacote npm do próprio CLI

-- Ordem em que os agentes aparecem em pickers/menus.
M.agents = {
  {
    bin = "claude",
    label = "Claude Code",
    cmd = "claude",
    login = "claude auth login",
    adapter = "claude_code",
    acp_bin = "claude-agent-acp",
    acp_npm = "@agentclientprotocol/claude-agent-acp",
    npm = "@anthropic-ai/claude-code",
  },
  {
    bin = "opencode",
    label = "opencode",
    cmd = "opencode",
    login = "opencode auth login",
    adapter = "opencode",
    acp_bin = "opencode",
    acp_args = { "acp" },
    npm = "opencode-ai",
  },
  {
    bin = "codex",
    label = "Codex CLI",
    cmd = "codex",
    login = "codex login",
    adapter = "codex",
    acp_bin = "codex-acp",
    acp_npm = "@agentclientprotocol/codex-acp",
    npm = "@openai/codex",
  },
  {
    bin = "gemini",
    label = "Gemini CLI",
    cmd = "gemini",
    login = "gemini",
    adapter = "gemini_cli",
    acp_bin = "gemini",
    acp_args = { "--experimental-acp" },
    npm = "@google/gemini-cli",
  },
}

-- Pacote antigo da ponte do Claude (renomeado); instala o mesmo binário e
-- impede o `npm install -g` do novo, então é removido antes.
M.legacy_npm = { claude = "@zed-industries/claude-agent-acp" }

local cache ---@type table?
local exe_cache = {} ---@type table<string, boolean>
local path_cache = {} ---@type table<string, string|false>

---`true` se o executável está no PATH (cacheado; no Windows cada busca de um
---binário ausente custa ~20ms). `detect(true)` limpa o cache.
---@param bin string
---@return boolean
function M.has(bin)
  if exe_cache[bin] == nil then exe_cache[bin] = vim.fn.executable(bin) == 1 end
  return exe_cache[bin]
end

---Caminho para executar `bin` com `vim.system`/`uv.spawn`. No Windows os CLIs
---instalados pelo npm são scripts `.cmd`, e o spawn pelo nome falha com ENOENT;
---o caminho completo do `.cmd` funciona.
---@param bin string
---@return string
function M.exe(bin)
  if path_cache[bin] == nil then
    local found = false
    if vim.fn.has "win32" == 1 then
      for _, ext in ipairs { ".cmd", ".exe", ".bat" } do
        local p = vim.fn.exepath(bin .. ext)
        if p ~= "" then
          found = p
          break
        end
      end
    end
    if not found then
      local p = vim.fn.exepath(bin)
      found = p ~= "" and p or false
    end
    path_cache[bin] = found
  end
  return path_cache[bin] or bin
end

---Comando ACP completo (lista) de um agente, pronto para o spawn.
---@param a AstrographAgent
---@return string[]
function M.acp_command(a)
  local cmd = { M.exe(a.acp_bin) }
  vim.list_extend(cmd, a.acp_args or {})
  return cmd
end

---Roda um comando curto e devolve o resultado (nil se falhar ao iniciar).
---@param cmd string[]
---@param timeout? integer
---@return vim.SystemCompleted?
local function run(cmd, timeout)
  local ok, res = pcall(function() return vim.system(cmd, { text = true }):wait(timeout or 8000) end)
  return ok and res or nil
end

---Como checar o login de um agente: um booleano quando a resposta é imediata
---(chave de API no ambiente, arquivo de credencial) ou `{ cmd, parse }` quando
---é preciso perguntar ao próprio CLI.
---@param a AstrographAgent
---@return boolean|{cmd:string[], parse:fun(res:vim.SystemCompleted):boolean?}|nil
local function login_check(a)
  if not M.has(a.bin) then return false end
  if a.bin == "claude" then
    if vim.env.ANTHROPIC_API_KEY or vim.env.CLAUDE_CODE_OAUTH_TOKEN then return true end
    return {
      cmd = { M.exe "claude", "auth", "status" },
      parse = function(res)
        local ok, data = pcall(vim.json.decode, res.stdout or "")
        if ok and type(data) == "table" then return data.loggedIn == true end
        return nil
      end,
    }
  elseif a.bin == "opencode" then
    return {
      cmd = { M.exe "opencode", "auth", "list" },
      parse = function(res)
        local text = ((res.stdout or "") .. (res.stderr or "")):gsub("\27%[[%d;]*m", "")
        local n = tonumber(text:match "(%d+)%s+credentials?")
        if n == nil then return nil end
        return n > 0
      end,
    }
  elseif a.bin == "codex" then
    if vim.env.OPENAI_API_KEY then return true end
    return { cmd = { M.exe "codex", "login", "status" }, parse = function(res) return res.code == 0 end }
  elseif a.bin == "gemini" then
    if vim.env.GEMINI_API_KEY or vim.env.GOOGLE_API_KEY then return true end
    return vim.fn.filereadable(vim.fn.expand "~/.gemini/oauth_creds.json") == 1
  end
  return nil
end

---Situação do login: `true` (logado), `false` (não) ou `nil` (não dá para
---saber). Síncrono: roda o CLI, então só use fora do startup (ex.: health).
---@param a AstrographAgent
---@return boolean?
function M.logged_in(a)
  local c = login_check(a)
  if type(c) ~= "table" then return c end
  local res = run(c.cmd)
  if not res then return nil end
  return c.parse(res)
end

---Situação de todos os agentes instalados, em paralelo e sem travar o editor.
---`cb` recebe `{ agent, acp, login }[]` no loop principal.
---@param cb fun(list:{agent:AstrographAgent, acp:boolean, login:boolean?}[])
function M.status(cb)
  local list, pending = {}, 1
  local function done()
    pending = pending - 1
    if pending == 0 then vim.schedule(function() cb(list) end) end
  end
  for _, a in ipairs(M.agents) do
    if M.has(a.bin) then
      local item = { agent = a, acp = M.has_acp(a), login = nil }
      list[#list + 1] = item
      local c = login_check(a)
      if type(c) == "table" then
        pending = pending + 1
        local ok = pcall(vim.system, c.cmd, { text = true, timeout = 10000 }, function(res)
          item.login = c.parse(res)
          done()
        end)
        if not ok then done() end
      else
        item.login = c
      end
    end
  end
  done()
end

---A ponte ACP (chat do CodeCompanion) deste agente está instalada?
---@param a AstrographAgent
---@return boolean
function M.has_acp(a) return M.has(a.acp_bin) end

---A ponte instalada é o pacote antigo (renomeado, sem atualizações)?
---@param a AstrographAgent
---@return boolean
function M.legacy_acp(a)
  local legacy = M.legacy_npm[a.bin]
  if not legacy or not M.has(a.acp_bin) then return false end
  local dir = vim.fs.dirname(M.exe(a.acp_bin))
  for _, root in ipairs { dir .. "/node_modules", dir .. "/../lib/node_modules" } do
    if (vim.uv or vim.loop).fs_stat(root .. "/" .. legacy) then return true end
  end
  return false
end

---Host base do Ollama, aceitando `OLLAMA_HOST` com ou sem esquema.
---@return string
function M.ollama_host()
  local host = vim.env.OLLAMA_HOST
  if not host or host == "" then host = "http://localhost:11434" end
  if not host:match "^https?://" then host = "http://" .. host end
  return host
end

---Testa se uma porta TCP responde (sem dependências externas), com timeout curto.
---@param host string
---@param port integer
---@param timeout? integer
---@return boolean
function M.port_open(host, port, timeout)
  local uv = vim.uv or vim.loop
  local tcp = uv.new_tcp()
  local done, open = false, false
  local ok = pcall(tcp.connect, tcp, host, port, function(err)
    open = err == nil
    done = true
    pcall(function() tcp:close() end)
  end)
  if not ok then return false end
  vim.wait(timeout or 300, function() return done end, 20)
  if not done then pcall(function() tcp:close() end) end
  return open
end

---`curl -s --max-time …` com timeout; retorna stdout ou `nil`.
---@param args string[]
---@param timeout? integer
---@return string?
function M.curl(args, timeout)
  if not M.has "curl" then return nil end
  local cmd = { "curl", "-s", "--max-time", tostring(math.floor((timeout or 2000) / 1000) + 1) }
  vim.list_extend(cmd, args)
  local ok, res = pcall(function() return vim.system(cmd, { text = true, timeout = timeout or 2000 }):wait() end)
  if ok and res and res.code == 0 then return res.stdout end
  return nil
end

---Ollama está no ar? Usa a porta TCP (barato) e evita HTTP quando só queremos saber.
---@return boolean
function M.ollama_up()
  local host = (M.ollama_host():gsub("^https?://", ""))
  local h, p = host:match "^([^:/]+):?(%d*)$"
  return M.port_open(h or "localhost", tonumber(p) ~= nil and tonumber(p) or 11434, 300)
end

---Lista os modelos instalados no Ollama (via `/api/tags`).
---@return string[]
function M.ollama_models()
  local out = M.curl { M.ollama_host() .. "/api/tags" }
  if not out then return {} end
  local ok, data = pcall(vim.json.decode, out)
  if not ok or type(data) ~= "table" or type(data.models) ~= "table" then return {} end
  local names = {}
  for _, m in ipairs(data.models) do
    if m.name then names[#names + 1] = m.name end
  end
  return names
end

---Diretório de config do Astrograph (gravado pelo instalador).
---No Windows fica em %APPDATA%/astrograph: o antigo %LOCALAPPDATA%/astrograph é a
---mesma pasta do clone `%LOCALAPPDATA%\Astrograph` (o NTFS não diferencia caixa).
---@return string
function M.config_dir()
  local base = vim.fn.has "win32" == 1 and (vim.env.APPDATA or vim.fn.expand "~/AppData/Roaming")
    or vim.fn.expand((vim.env.XDG_CONFIG_HOME or "~/.config"))
  return vim.fs.joinpath(base, "astrograph")
end

---@return string
function M.config_path() return vim.fs.joinpath(M.config_dir(), "config.json") end

---Caminho legado (instaladores antigos no Windows), lido só como fallback.
---@return string?
local function legacy_config_path()
  if vim.fn.has "win32" == 0 or not vim.env.LOCALAPPDATA then return nil end
  return vim.fs.joinpath(vim.env.LOCALAPPDATA, "astrograph", "config.json")
end

---@return table
function M.read_config()
  local path = M.config_path()
  if vim.fn.filereadable(path) == 0 then path = legacy_config_path() or path end
  if vim.fn.filereadable(path) == 0 then return {} end
  local ok, lines = pcall(vim.fn.readfile, path)
  if not ok then return {} end
  local dec_ok, data = pcall(vim.json.decode, table.concat(lines, "\n"))
  if not dec_ok or type(data) ~= "table" then return {} end
  return data
end

---Modelo local a usar, por ordem: env → config do instalador → 1º modelo do Ollama.
---@param models? string[]
---@return string?
function M.local_model(models)
  local env = vim.env.ASTROGRAPH_LOCAL_MODEL
  if env and env ~= "" then return env end
  local cfg = M.read_config().local_model
  if type(cfg) == "string" and cfg ~= "" then return cfg end
  models = models or M.ollama_models()
  return models[1]
end

---URL/model de endpoint local OpenAI-compatível (LM Studio, llama.cpp, vLLM…).
---@return string?
function M.local_url()
  local url = vim.env.ASTROGRAPH_LOCAL_URL
  return (url and url ~= "" and url) or nil
end

---Comando base de um agente pelo nome do binário.
---@param bin string
---@return string?
function M.terminal_cmd(bin)
  for _, a in ipairs(M.agents) do
    if a.bin == bin then return a.cmd end
  end
  return nil
end

---Root do projeto (astrocore), com fallback para o cwd.
---@return string
function M.root()
  local ok, root = pcall(function() return require("astrocore.rooter").root(0) end)
  return (ok and root) or vim.fn.getcwd()
end

---Detecção completa (cacheada). `refresh` força nova checagem.
---@param refresh? boolean
---@return table
function M.detect(refresh)
  if cache and not refresh then return cache end
  if refresh then
    exe_cache, path_cache = {}, {}
  end
  local info = {
    claude = M.has "claude",
    opencode = M.has "opencode",
    codex = M.has "codex",
    gemini = M.has "gemini",
    claude_acp = M.has "claude-agent-acp",
    codex_acp = M.has "codex-acp",
    node = M.has "node",
    ollama = M.ollama_up(),
  }
  info.models = info.ollama and M.ollama_models() or {}
  info.local_model = info.ollama and M.local_model(info.models) or nil
  cache = info
  return info
end

---Agentes detectados, na ordem de `M.agents` (para pickers/menus).
---@return AstrographAgent[]
function M.available()
  local out = {}
  for _, a in ipairs(M.agents) do
    if M.has(a.bin) then out[#out + 1] = a end
  end
  return out
end

return M
