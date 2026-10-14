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
---@field bin string  # executável no PATH (vim.fn.executable)
---@field label string
---@field cmd string  # comando base para terminal / CLI interaction

-- Ordem em que os agentes aparecem em pickers/menus.
M.agents = {
  { bin = "claude", label = "Claude Code", cmd = "claude" },
  { bin = "opencode", label = "opencode", cmd = "opencode" },
  { bin = "codex", label = "Codex CLI", cmd = "codex" },
  { bin = "gemini", label = "Gemini CLI", cmd = "gemini" },
}

local cache ---@type table?
local exe_cache = {} ---@type table<string, boolean>

---`true` se o executável está no PATH (cacheado; no Windows cada busca de um
---binário ausente custa ~20ms). `detect(true)` limpa o cache.
---@param bin string
---@return boolean
function M.has(bin)
  if exe_cache[bin] == nil then exe_cache[bin] = vim.fn.executable(bin) == 1 end
  return exe_cache[bin]
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
  if refresh then exe_cache = {} end
  local info = {
    claude = M.has "claude",
    opencode = M.has "opencode",
    codex = M.has "codex",
    gemini = M.has "gemini",
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
