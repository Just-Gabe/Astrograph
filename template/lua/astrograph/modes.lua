-- Modo do agente (permissões): uma escolha só, salva e aplicada em todas as
-- telas de agente do Astrograph, como o agente atual (<Leader>Ag):
--
--   * painel do agente nativo (<Leader>Ao / <Leader>Ae)
--   * chat do CodeCompanion (<Leader>Ac)
--   * sidebar do avante (<Leader>At)
--   * terminal do agente (<Leader>Al / <Leader>AT) e a interação CLI
--
-- Onde o agente tem o modo (ex.: Claude Code: Manual, Aceitar edições, Plano,
-- Auto, Sem permissões), o próprio agente faz valer: a sessão nova já nasce
-- nele (session/set_mode do ACP; `--permission-mode` no terminal) e a tela
-- mostra tudo o que ele pedir. Onde não tem (ex.: o opencode só tem build e
-- plan), o Astrograph imita: aprova sozinho o que o modo deixaria passar e
-- pergunta o resto, e avisa uma vez que é imitação.
--
--   <Leader>Aw  ou  :Astrograph modo   escolher (vale para tudo, fica salvo)

local M = {}

local function agents() return require "astrograph.agents" end
local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Astrograph" }) end

---@class AstrographMode
---@field id string
---@field label string
---@field desc string

---@type AstrographMode[]
M.list = {
  { id = "manual", label = "Manual", desc = "pergunta antes de editar ou executar" },
  { id = "acceptEdits", label = "Aceitar edições", desc = "edita sem perguntar; comandos ainda pedem" },
  { id = "plan", label = "Plano", desc = "só lê e planeja, não altera nada" },
  { id = "auto", label = "Auto", desc = "o agente decide o que precisa de você" },
  { id = "bypass", label = "Sem permissões", desc = "aceita tudo, sem perguntar (cuidado)" },
}

M.default = "manual"

-- quanto a conversa do avante espera o agente confirmar a troca de modo (ms)
M.switch_timeout = 5000

-- Ids de modo ACP que equivalem a cada modo. Só os medidos (Claude Code:
-- default/acceptEdits/plan/auto/bypassPermissions na ponte 0.91; opencode:
-- build/plan na 2.0.31). Id de outro agente que não esteja aqui cai na imitação
-- (mais perguntas, nunca menos) até ser medido e entrar na lista.
local ACP_IDS = {
  manual = { "default" },
  acceptEdits = { "acceptEdits" },
  plan = { "plan" },
  auto = { "auto" },
  bypass = { "bypassPermissions" },
}

-- Imitação (agente sem o modo): o tipo de ferramenta (`kind` do ACP) que o
-- Astrograph aprova sozinho. `true` = tudo.
local EMULATED = {
  manual = {},
  plan = {},
  acceptEdits = { edit = true },
  auto = { edit = true, read = true, search = true, think = true, fetch = true },
  bypass = true,
}

-- Ferramentas do próprio avante (provedores por HTTP, sem modo de agente)
local AVANTE_EDIT = { "edit_file", "replace_in_file", "str_replace", "write_to_file", "create", "insert", "undo_edit" }
local AVANTE_READ = { "view", "ls", "glob", "grep", "read_todos", "think", "get_diagnostics", "web_search" }

---@param id string?
---@return AstrographMode?
function M.get(id)
  for _, m in ipairs(M.list) do
    if m.id == id then return m end
  end
end

local session_choice ---@type string? escolha desta sessão (se o arquivo falhar)

---Modo atual (o escolhido, salvo no config.json; sem escolha, Manual).
---@return string
function M.current()
  local id = session_choice
  if not id then
    local ok, cfg = pcall(function() return agents().read_config() end)
    id = ok and type(cfg) == "table" and cfg.agent_mode or nil
  end
  return M.get(id) and id or M.default
end

---@return string
function M.label(id) return (M.get(id or M.current()) or M.list[1]).label end

---Id de modo ACP do agente para o modo `id`, entre os que ele oferece.
---`exact` = o agente tem o modo; senão volta o modo de trabalho normal dele
---(ex.: `build` no opencode) e o Astrograph imita o resto.
---@param available (string|{id?:string, value?:string})[]
---@param id? string
---@return string? mode_id, boolean exact
function M.resolve(available, id)
  id = id or M.current()
  local have = {}
  for _, m in ipairs(available or {}) do
    local v = type(m) == "table" and (m.id or m.value) or m
    if type(v) == "string" then have[v] = true end
  end
  for _, cand in ipairs(ACP_IDS[id] or {}) do
    if have[cand] then return cand, true end
  end
  if id ~= "plan" then
    -- o modo de trabalho comum do agente, sem restrição própria de permissão
    for _, cand in ipairs { "build", "default", "code" } do
      if have[cand] then return cand, false end
    end
  end
  return nil, false
end

---O Astrograph aprova sozinho este pedido de permissão? Só quando o agente
---não tem o modo (imitação); com o modo de verdade, quem decide é o agente e
---a tela mostra tudo o que ele pedir.
---@param kind string?
---@param exact boolean
---@param id? string
---@return boolean
function M.auto_approves(kind, exact, id)
  if exact then return false end
  local rule = EMULATED[id or M.current()]
  if rule == true then return true end
  return type(rule) == "table" and kind ~= nil and rule[kind] == true
end

---Valor de `behaviour.auto_approve_tool_permissions` do avante (ele recebe o
---`kind` do ACP ou o nome da ferramenta própria).
---@param exact boolean
---@param id? string
---@return boolean|string[]
function M.avante_auto_approve(exact, id)
  if exact then return false end
  id = id or M.current()
  local rule = EMULATED[id]
  if rule == true then return true end
  local list = {}
  for kind in pairs(rule or {}) do
    list[#list + 1] = kind
  end
  if id == "acceptEdits" or id == "auto" then vim.list_extend(list, AVANTE_EDIT) end
  if id == "auto" then vim.list_extend(list, AVANTE_READ) end
  if #list == 0 then return false end
  table.sort(list)
  return list
end

---Modo de aprovação do chat do CodeCompanion (ask/auto/yolo) para a sessão.
---@param exact boolean
---@param id? string
---@return "ask"|"auto"|"yolo"
function M.codecompanion_approval(exact, id)
  if exact then return "ask" end
  id = id or M.current()
  if id == "bypass" then return "yolo" end
  if id == "auto" then return "auto" end
  return "ask" -- Aceitar edições imitado: o chat não separa edição de comando
end

local warned = {} ---@type table<string, true>

---Avisa (uma vez por agente e modo) que o agente não tem o modo escolhido.
---@param who string nome do agente
---@param exact boolean
---@param id? string
---@param what? string o que acontece no lugar (padrão: a imitação do painel e do avante)
function M.warn_emulated(who, exact, id, what)
  id = id or M.current()
  if exact or id == "manual" then return end
  local key = who .. "|" .. id
  if warned[key] then return end
  warned[key] = true
  what = what or ({
    plan = "aqui NÃO é só leitura: ele pode pedir para editar, e cada pedido aparece para você aprovar ou recusar",
    acceptEdits = "o Astrograph aprova só as edições e pergunta o resto",
    auto = "o Astrograph aprova leituras e edições e pergunta comandos",
    bypass = "o Astrograph aprova tudo o que ele pedir",
  })[id]
  notify(("%s não tem o modo %s — %s."):format(who, M.label(id), what or "fica no modo dele"), vim.log.levels.WARN)
end

---------------------------------------------------------------------------
-- Terminal
---------------------------------------------------------------------------

-- Opções de linha de comando por agente (só as conferidas no CLI instalado:
-- `claude --permission-mode` aceita default/acceptEdits/plan/auto/
-- bypassPermissions; `opencode --auto` aprova o que não for negado).
local TERMINAL = {
  claude = function(id)
    local mode = ({ manual = "default", acceptEdits = "acceptEdits", plan = "plan", auto = "auto", bypass = "bypassPermissions" })[id]
    return mode and ("--permission-mode " .. mode), true
  end,
  opencode = function(id)
    if id == "auto" or id == "bypass" then return "--auto", id == "bypass" end
    return nil, id == "manual"
  end,
}

---Comando do terminal do agente já no modo atual. `exact` = falso quando o
---CLI não tem como abrir nesse modo (fica no padrão dele).
---@param a AstrographAgent
---@param id? string
---@return string cmd, boolean exact
function M.terminal_cmd(a, id)
  id = id or M.current()
  local f = TERMINAL[a.bin]
  if not f then return a.cmd, id == "manual" end
  local extra, exact = f(id)
  return extra and (a.cmd .. " " .. extra) or a.cmd, exact
end

---------------------------------------------------------------------------
-- Sessões abertas: CodeCompanion e avante (o painel nativo escuta o evento)
---------------------------------------------------------------------------

---Valores de uma opção de modo do ACP (configOptions, categoria `mode`).
local function option_values(opt)
  local vals = {}
  for _, o in ipairs(type(opt.options) == "table" and opt.options or {}) do
    if type(o) == "table" then
      if o.value then
        vals[#vals + 1] = o.value
      elseif type(o.options) == "table" then -- grupos
        for _, g in ipairs(o.options) do
          vals[#vals + 1] = g.value
        end
      end
    end
  end
  return vals
end

---@param config_options table[]?
---@return table?
local function mode_option(config_options)
  for _, opt in ipairs(type(config_options) == "table" and config_options or {}) do
    if type(opt) == "table" and (opt.category == "mode" or opt.id == "mode") then return opt end
  end
end

---Coloca uma conexão ACP do CodeCompanion no modo atual (síncrono: roda logo
---depois da sessão nascer, antes do primeiro pedido).
---@param conn table CodeCompanion.ACP.Connection
function M.apply_codecompanion(conn)
  if type(conn) ~= "table" or not conn.session_id then return end
  local opt = mode_option(conn._config_options)
  local target, exact = M.resolve(opt and option_values(opt) or {})
  if opt and target and target ~= opt.currentValue then
    if not conn:set_config_option(opt.id, target) then
      notify(("Não consegui pôr o chat no modo %s."):format(M.label()), vim.log.levels.WARN)
      exact = false
    end
  end
  local chat = conn.chat
  if type(chat) == "table" and chat.bufnr then
    pcall(function()
      require("codecompanion.interactions.chat.tools.approvals"):set_mode(chat.bufnr, { mode = M.codecompanion_approval(exact) })
    end)
  end
  -- a imitação do chat é a aprovação dele (ask/auto/yolo), mais grossa que a
  -- do painel: o aviso diz o que acontece de fato
  local name = vim.tbl_get(conn, "adapter", "formatted_name") or "O agente do chat"
  M.warn_emulated(name .. " (chat)", exact, nil, ({
    acceptEdits = "o chat pergunta tudo, inclusive as edições",
    auto = "o chat aprova leituras, edições e comandos que ele considera seguros, e pergunta o resto",
  })[M.current()])
end

---Liga o CodeCompanion: toda sessão ACP nova (ou retomada) nasce no modo atual.
function M.setup_codecompanion()
  local ok, Connection = pcall(require, "codecompanion.acp")
  if not ok or type(Connection) ~= "table" or type(Connection._establish_session) ~= "function" then return end
  if Connection._astrograph_mode then return end
  Connection._astrograph_mode = true
  local orig = Connection._establish_session
  Connection._establish_session = function(self, ...)
    local res = orig(self, ...)
    if res then
      local okm, err = pcall(M.apply_codecompanion, self)
      if not okm then notify("Modo do chat: " .. tostring(err), vim.log.levels.WARN) end
    end
    return res
  end
end

---Coloca a sessão ACP do avante no modo atual e ajusta a aprovação dele.
---@param client table avante.acp.ACPClient
---@param session_id string
---@param cb? fun()
function M.apply_avante(client, session_id, cb)
  cb = cb or function() end
  local Config = require "avante.config"
  local opt = mode_option(client.config_options)
  local target, exact = M.resolve(opt and option_values(opt) or {})
  local function done(ok)
    Config.behaviour.auto_approve_tool_permissions = M.avante_auto_approve(ok and exact)
    M.warn_emulated(Config.provider or "O agente do avante", ok and exact)
    cb()
  end
  if not (opt and target and target ~= opt.currentValue) then return done(true) end
  -- até o agente confirmar a troca, pergunta tudo (nunca aprova no modo velho)
  Config.behaviour.auto_approve_tool_permissions = false
  local finished = false
  local function after(_, err)
    vim.schedule(function()
      if finished then return end
      finished = true
      if err then notify(("Não consegui pôr o avante no modo %s."):format(M.label()), vim.log.levels.WARN) end
      done(not err)
    end)
  end
  -- agente que não responde à troca não pode prender a conversa: em 5 s segue
  -- no modo dele, com a imitação (avisada)
  vim.defer_fn(function() after(nil, { message = "sem resposta" }) end, M.switch_timeout)
  if client._legacy_api then
    client:set_mode(session_id, target, after)
  else
    client:set_config_option(session_id, opt.id, target, after)
  end
end

---Liga o avante: sessão ACP nova nasce no modo atual (antes do primeiro
---pedido) e a aprovação automática dele (que por padrão aprova TUDO) passa a
---seguir o modo.
function M.setup_avante()
  local okc, Config = pcall(require, "avante.config")
  if okc and Config.behaviour then Config.behaviour.auto_approve_tool_permissions = M.avante_auto_approve(false) end
  local ok, ACPClient = pcall(require, "avante.libs.acp_client")
  if not ok or type(ACPClient) ~= "table" or type(ACPClient.create_session) ~= "function" then return end
  if ACPClient._astrograph_mode then return end
  ACPClient._astrograph_mode = true
  ---Segura o retorno da sessão até ela estar no modo (falha no modo: segue
  ---igual). O retorno do avante roda uma vez só, e fora do pcall.
  local function hold(client, sid, callback, a, b)
    local called = false
    local function once()
      if called then return end
      called = true
      callback(a, b)
    end
    local okm = pcall(M.apply_avante, client, sid, function() vim.schedule(once) end)
    if not okm then
      if okc and Config.behaviour then Config.behaviour.auto_approve_tool_permissions = false end
      once()
    end
  end
  local create = ACPClient.create_session
  ACPClient.create_session = function(self, cwd, mcp, callback, ...)
    callback = callback or function() end
    return create(self, cwd, mcp, function(session_id, err)
      if err or not session_id then return callback(session_id, err) end
      hold(self, session_id, callback, session_id, err)
    end, ...)
  end
  -- conversa retomada (<Leader>AS): também volta no modo escolhido
  for _, name in ipairs { "load_session", "resume_session" } do
    local orig = ACPClient[name]
    if type(orig) == "function" then
      ACPClient[name] = function(self, session_id, cwd, mcp, callback, ...)
        callback = callback or function() end
        return orig(self, session_id, cwd, mcp, function(result, err)
          if err or not result then return callback(result, err) end
          hold(self, session_id, callback, result, err)
        end, ...)
      end
    end
  end
end

---Leva a troca para as sessões já abertas do chat e do avante.
local function apply_open_sessions()
  if package.loaded["codecompanion"] then
    pcall(function()
      for _, e in ipairs(require("codecompanion.interactions.chat").buf_get_chat()) do
        local conn = e.chat and e.chat.acp_connection
        if conn and conn.session_id then M.apply_codecompanion(conn) end
      end
    end)
    pcall(function()
      local cfg = require "codecompanion.config"
      for bin, spec in pairs(vim.tbl_get(cfg, "interactions", "cli", "agents") or {}) do
        local a = agents().by_bin(bin)
        if a and type(spec) == "table" then spec.cmd = (M.terminal_cmd(a)) end
      end
    end)
  end
  if package.loaded["avante"] then
    pcall(function()
      local Config = require "avante.config"
      local sidebar = require("avante").get()
      local client = sidebar and sidebar.acp_client
      local sid = sidebar and sidebar.chat_history and sidebar.chat_history.acp_session_id
      if client and sid and sid ~= "" then
        M.apply_avante(client, sid) -- a aprovação só muda quando a sessão confirmar
      else
        Config.behaviour.auto_approve_tool_permissions = M.avante_auto_approve(false)
      end
    end)
  end
end

---Troca o modo de todas as telas de agente e salva a escolha. "Sem
---permissões" pede confirmação antes (salvo com `confirmed`).
---@param id string
---@param opts? { quiet?: boolean, confirmed?: boolean }
---@return boolean ok
function M.set(id, opts)
  opts = opts or {}
  if not M.get(id) then
    notify(("Modo desconhecido: %s (use %s)"):format(tostring(id), table.concat(vim.tbl_map(function(m) return m.id end, M.list), ", ")), vim.log.levels.WARN)
    return false
  end
  if id == "bypass" and not opts.confirmed and M.current() ~= "bypass" then
    vim.ui.select({ "Sim, sem permissões em todas as telas", "Não" }, {
      prompt = "Sem permissões: o agente edita e executa tudo sem perguntar, e fica salvo. Confirmar?",
    }, function(choice, idx)
      if idx == 1 then M.set(id, vim.tbl_extend("force", opts, { confirmed = true })) end
    end)
    return false
  end
  session_choice = id
  agents().write_config { agent_mode = id }
  warned = {}
  apply_open_sessions()
  vim.api.nvim_exec_autocmds("User", { pattern = "AstrographModeChanged", data = { mode = id } })
  if not opts.quiet then
    local extra = id == "bypass" and "\nCuidado: o agente faz tudo sem perguntar." or ""
    notify(("Modo do agente: %s. Já vale no painel, no chat e no avante; o terminal abre nele da próxima vez.%s"):format(M.label(id), extra))
  end
  return true
end

---Lembra na abertura quando o modo salvo é "Sem permissões" (ele vale em
---todas as telas sem perguntar nada, então não pode ficar esquecido).
function M.setup()
  vim.api.nvim_create_autocmd("User", {
    pattern = "VeryLazy",
    once = true,
    group = vim.api.nvim_create_augroup("astrograph_modes", { clear = true }),
    desc = "Avisar do modo Sem permissões salvo",
    callback = function()
      if M.current() == "bypass" then
        notify("Modo do agente salvo: Sem permissões — o agente edita e executa sem perguntar. Troque com <Leader>Aw.", vim.log.levels.WARN)
      end
    end,
  })
end

---Seletor do modo (marca o atual).
function M.pick()
  local cur = M.current()
  vim.ui.select(M.list, {
    prompt = "Modo do agente (vale para painel, chat, avante e terminal)",
    format_item = function(m) return ("%s%s — %s"):format(m.id == cur and "● " or "  ", m.label, m.desc) end,
  }, function(m)
    if m then M.set(m.id) end
  end)
end

return M
