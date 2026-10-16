-- OmniRoute: aviso de troca de modelo dentro dos chats.
--
-- O OmniRoute (gateway local de modelos grátis, em http://localhost:20128)
-- troca de modelo sozinho quando o que estava em uso esgota — e o fallback
-- preserva o contexto da conversa. O Neovim não recebe essa informação pelo
-- agente (o protocolo só entrega o texto), então lemos o registro de chamadas
-- que o OmniRoute grava a cada request:
--
--   ~/.omniroute/call_logs/<dia>/<timestamp>_<id>.json
--     { "summary": { "model": "...", "provider": "...", "comboName": "..." },
--       "requestBody": { "messages": [ ... ] }, ... }
--
-- Depois de cada resposta, o modelo servido é comparado com o da resposta
-- anterior daquela conversa; se mudou, avisamos no fim da resposta:
--
--   ▸ mudou para <modelo> via OmniRoute (antes: <modelo>)
--
-- No chat do CodeCompanion o aviso é uma linha virtual (só na tela, nunca
-- enviada ao modelo); no painel do agente, uma linha do próprio painel.
--
-- O nome do modelo sai em verde e o aviso inteiro numa faixa amarela (as
-- cores ficam em `M.setup`). O mesmo aviso vale para os dois chats: o do
-- CodeCompanion (<Leader>Ac, ligado em lua/plugins/codecompanion.lua) e o
-- painel do agente nativo (<Leader>Ao, ligado em lua/astrograph/agent.lua).
--
-- Limites conhecidos: o registro é do OmniRoute inteiro, então um pedido de
-- outro programa (ex.: o opencode no terminal) no mesmo intervalo pode ser
-- lido como o do chat; a chave da conversa reduz isso, mas logs acima de 4 MB
-- ficam sem chave. Só lemos arquivos locais: nada é enviado a lugar nenhum.

local M = {}

---Namespace dos destaques do aviso (nunca limpo por outros módulos).
M.ns = vim.api.nvim_create_namespace "astrograph_omniroute"

local fmt = string.format
local uv = vim.uv or vim.loop

---Cores do aviso: nome do modelo em verde, faixa amarela no aviso.
local HL_MODEL = "AstrographOmniRouteModel"
local HL_NOTICE = "AstrographOmniRouteNotice"

---Último modelo visto por conversa (chave = primeiro pedido do usuário).
local last = {}

---Início do turno atual (epoch), para achar o log que ele gerou.
local pending = {} ---@type table<string, string> carimbo do começo do turno, por chat

-- ---------------------------------------------------------------- localização

---Raiz do estado do OmniRoute (`OMNIROUTE_HOME` sobrescreve o padrão).
function M.home()
  local h = vim.env.OMNIROUTE_HOME
  if h and h ~= "" then return h end
  return vim.fn.expand "~/.omniroute"
end

function M.logs_dir()
  return M.home() .. "/call_logs"
end

---Diretórios de log de hoje e de ontem (o dia pode virar no meio do turno),
---no horário local e em UTC: os nomes dos logs estão em UTC, e no Brasil o
---dia UTC vira às 21h locais.
local function day_dirs(at)
  local dirs, seen = {}, {}
  for _, off in ipairs { 0, -86400 } do
    for _, f in ipairs { "*t", "!*t" } do
      local t = os.date(f, at + off)
      local d = fmt("%s/%04d-%02d-%02d", M.logs_dir(), t.year, t.month, t.day)
      if not seen[d] then
        seen[d] = true
        dirs[#dirs + 1] = d
      end
    end
  end
  return dirs
end

---Instante atual como o começo do nome de um log ("2026-10-16T15-09-32.562Z").
---Os nomes estão em UTC com milissegundos, então comparar o texto é comparar o
---tempo: filtra a pasta sem um fs_stat por arquivo.
function M.stamp(sec, usec)
  if not sec then sec, usec = uv.gettimeofday() end
  return os.date("!%Y-%m-%dT%H-%M-%S", sec) .. fmt(".%03dZ", math.floor((usec or 0) / 1000))
end

---Logs gravados a partir de `since` (carimbo de `M.stamp`), do mais novo para o
---mais antigo.
local function recent_logs(since)
  local found = {}
  for _, dir in ipairs(day_dirs(os.time())) do
    local ok, names = pcall(vim.fn.readdir, dir)
    if ok then
      for _, name in ipairs(names) do
        if name >= since and name:sub(-5) == ".json" then found[#found + 1] = { path = dir .. "/" .. name, name = name } end
      end
    end
  end
  table.sort(found, function(a, b) return a.name > b.name end)
  return found
end

-- Cada pedido grava um log com o corpo inteiro (um turno agêntico chega a
-- centenas de KB por pedido, e um turno faz vários pedidos). Decodificar todos
-- na thread do editor travaria a tela, então lemos só o começo do arquivo, onde
-- fica o `summary` (antes do `requestBody`), e decodificamos o arquivo inteiro
-- apenas no log escolhido, para a chave da conversa.
local HEAD = 16 * 1024
local FULL_MAX = 4 * 1024 * 1024
local heads, nheads = {}, 0 ---@type table<string, table|false>, integer resumo por caminho (o log não muda depois de gravado)
local HEADS_MAX = 2000 -- editor aberto por dias: o cache recomeça em vez de crescer sem fim

local function field(head, name)
  local v = head:match('"' .. name .. '"%s*:%s*"([^"]*)"')
  if v then return v end
  return head:match('"' .. name .. '"%s*:%s*(%d+)')
end

---Resumo de um log a partir do começo do arquivo: modelo, status e combo.
local function summary_of(path)
  if heads[path] ~= nil then return heads[path] or nil end
  local f = io.open(path, "rb")
  if not f then return nil end -- ainda sendo gravado: tenta de novo depois
  local head = f:read(HEAD) or ""
  f:close()
  local at = head:find('"summary"', 1, true)
  -- arquivo ainda sendo gravado: não guarda nada, a próxima tentativa relê
  if not at then return nil end
  -- só o objeto summary: o requestBody também tem "model"
  head = head:sub(at)
  local stop = head:find('"requestBody"', 1, true)
  if stop then head = head:sub(1, stop - 1) end
  local s = {
    model = field(head, "model"),
    status = tonumber(field(head, "status")),
    combo = field(head, "comboName"),
    provider = field(head, "provider"),
    requested = field(head, "requestedModel"),
  }
  if not s.status then return nil end -- resumo incompleto: idem
  if nheads >= HEADS_MAX then heads, nheads = {}, 0 end
  nheads = nheads + 1
  heads[path] = s
  return s
end

---Chave da conversa: hash do primeiro pedido do usuário (não muda entre os
---turnos). Logs grandes demais ficam sem chave (estado global).
local function conversation_key(path)
  local st = uv.fs_stat(path)
  if not st or st.size > FULL_MAX then return nil end
  local f = io.open(path, "rb")
  if not f then return nil end
  local raw = f:read "*a"
  f:close()
  local ok, log = pcall(vim.json.decode, raw)
  local body = ok and type(log) == "table" and log.requestBody or nil
  local msgs = type(body) == "table" and body.messages or nil
  for _, m in ipairs(type(msgs) == "table" and msgs or {}) do
    if type(m) == "table" and m.role == "user" then
      local text = m.content
      -- formato OpenAI com partes: { { type = "text", text = "..." }, ... }
      if type(text) == "table" then
        local parts = {}
        for _, p in ipairs(text) do
          if type(p) == "table" and type(p.text) == "string" then parts[#parts + 1] = p.text end
        end
        text = table.concat(parts, "\n")
      end
      if type(text) == "string" and text ~= "" then return vim.fn.sha256(text) end
    end
  end
  return nil
end

---O modelo que serviu um log, ou nil. Um pedido a um combo grava um log por
---modelo tentado e mais um agregado (model = nome do combo, ou "auto"); só a
---tentativa que respondeu (2xx) diz qual modelo serviu de fato.
local function read_log(path)
  local s = summary_of(path)
  if not s then return nil end
  local model = s.model
  if type(model) ~= "string" or model == "" or model == "keepalive" or model == "auto" then return nil end
  if model == s.combo then return nil end
  if not s.status or s.status < 200 or s.status > 299 then return nil end
  return {
    model = model,
    provider = s.provider,
    requested = s.requested,
    combo = s.combo,
    key = conversation_key(path),
  }
end

M._read_log = read_log -- para os testes

-- ------------------------------------------------------------------- estado

---Compara o modelo com o do turno anterior da mesma conversa e atualiza o
---estado. Devolve a mudança quando o modelo trocou.
function M.evaluate(info)
  if not info or not info.model then return nil end
  local key = info.key or "__omniroute__"
  local prev = last[key]
  last[key] = info.model
  if prev and prev ~= info.model then
    return { from = prev, to = info.model, provider = info.provider, combo = info.combo }
  end
  return nil
end

---Último modelo visto em cada conversa (para `:OmniRoute`).
function M.state()
  return last
end

---O recurso está ligado? `vim.g.astrograph_omniroute = false` desliga.
function M.enabled() return vim.g.astrograph_omniroute ~= false end

---Saúde do detector, para `:OmniRoute`: distingue "nenhuma troca" de "não
---está lendo os logs".
M.health = { turns = 0, found = 0 } ---@type { turns: integer, found: integer, at?: integer, model?: string }

---Marca o começo de um turno (chamado ao enviar um pedido). Cada chat tem a
---sua marca (`src`): o do CodeCompanion e o painel do agente não consomem a
---marca um do outro.
---@param src? string
function M.mark(src)
  pending[src or "default"] = M.stamp()
end

M.SETTLE_MS, M.TRIES = 150, 10

---Depois de um turno, procura o modelo no registro de chamadas e chama `cb`
---com a mudança — se houve. Num turno agêntico há vários pedidos e o log do
---último é gravado logo depois do fim: só decide quando a lista de logs parou
---de mudar entre duas tentativas (ou na última tentativa).
---@param cb fun(change: { from: string, to: string, provider: string?, combo: string? })
---@param src? string
function M.after_turn(cb, src)
  if not M.enabled() or not uv.fs_stat(M.logs_dir()) then return end -- desligado / OmniRoute ausente
  src = src or "default"
  local since = pending[src]
  pending[src] = nil
  -- sem marca: assume que a chamada acabou de ocorrer
  if not since then
    local sec, usec = uv.gettimeofday()
    since = M.stamp(sec - 5, usec)
  end
  M.health.turns = M.health.turns + 1

  local tries, newest = 0, nil
  local function attempt()
    tries = tries + 1
    local logs = recent_logs(since)
    local top = logs[1] and logs[1].name
    local settled = top ~= nil and top == newest
    newest = top
    if not settled and tries < M.TRIES then return vim.defer_fn(attempt, M.SETTLE_MS) end
    local info
    for _, log in ipairs(logs) do
      info = read_log(log.path)
      if info then break end
    end
    if not info then return end
    M.health.found, M.health.at, M.health.model = M.health.found + 1, os.time(), info.model
    local change = M.evaluate(info)
    if change then vim.schedule(function() cb(change) end) end
  end
  vim.defer_fn(attempt, M.SETTLE_MS)
end

-- ------------------------------------------------------------------ desenho

---Cor do aviso: nome do modelo em verde, faixa amarela no aviso inteiro.
function M.setup()
  -- o fundo amarelo também no nome: na linha virtual do chat não há line_hl
  -- contraste sobre o amarelo: verde 6,0:1 e texto 7,6:1 (mínimo AA: 4,5:1)
  vim.api.nvim_set_hl(0, HL_MODEL, { fg = "#08560a", bg = "#f2d16b", bold = true })
  vim.api.nvim_set_hl(0, HL_NOTICE, { bg = "#f2d16b", fg = "#3b3b2a" })
end

---Liga os destaques ao chat do CodeCompanion (`ChatDone`/`ChatSubmitted`) e
---deixa as cores definidas. Chamado uma vez, na carga do plugin.
function M.setup_chat()
  M.setup()
  vim.api.nvim_create_autocmd("ColorScheme", { group = vim.api.nvim_create_augroup("astrograph_omniroute", {}), callback = M.setup })
  vim.api.nvim_create_user_command("OmniRoute", function(args)
    if vim.trim(args.args or "") == "modelo" then return M.choose_model() end
    vim.notify(table.concat(M.report(), "\n"), vim.log.levels.INFO, { title = "OmniRoute" })
  end, {
    nargs = "?",
    complete = function() return { "modelo" } end,
    desc = "OmniRoute: estado do detector; `:OmniRoute modelo` escolhe o modelo/combo (fica salvo)",
  })

  local group = vim.api.nvim_create_augroup("astrograph_omniroute_chat", {})
  -- trocar o modelo no chat (`ga`) também é uma decisão sua: fica salva
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionChatModel",
    callback = function(args)
      local d = args.data or {}
      if type(d.model) ~= "string" or d.model == "" or not d.bufnr then return end
      local ok, cc = pcall(require, "codecompanion")
      if M.chat_selected(ok and cc.buf_get_chat(d.bufnr) or nil) then M.set_model(d.model, { quiet = true }) end
    end,
  })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionChatSubmitted",
    callback = function() M.mark "chat" end,
  })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionChatDone",
    callback = function(args)
      local buf = args.data and args.data.bufnr
      if not buf then return end
      local ok, cc = pcall(require, "codecompanion")
      local chat = ok and cc.buf_get_chat(buf) or nil
      -- só quando ESTE chat está no provedor OmniRoute (escolhido com `ga`)
      if not M.chat_selected(chat) then return end
      M.after_turn(function(change)
        if vim.api.nvim_buf_is_valid(buf) then M.notice_chat(chat, change) end
      end, "chat")
    end,
  })
end

-- ------------------------------------------------------------ modelo (escolha)
--
-- Nenhum modelo é assumido: você escolhe, a escolha fica no config.json do
-- Astrograph e vale entre sessões até você trocar (`:OmniRoute modelo` ou `ga`
-- no chat). `OMNIROUTE_MODEL` no ambiente vence a escolha salva.

---Combos e modelos que o OmniRoute oferece, pelo próprio CLI (não exige a
---chave da API). Guardado por 60 s.
local choices_cache ---@type { at: integer, list: string[] }?
function M.model_choices()
  local now = os.time()
  if choices_cache and now - choices_cache.at < 60 then return choices_cache.list end
  local list, seen = {}, {}
  local agents = require "astrograph.agents"
  local ok, res = pcall(function()
    return vim.system({ agents.exe "omniroute", "combo", "list", "--output", "json" }, { text = true }):wait(15000)
  end)
  local out = ok and res and res.stdout or ""
  local start = out:find("{", 1, true) -- o CLI imprime avisos antes do JSON
  local okj, data = pcall(vim.json.decode, start and out:sub(start) or "")
  for _, c in ipairs(okj and type(data) == "table" and data.combos or {}) do
    if type(c.name) == "string" and not seen[c.name] then
      seen[c.name] = true
      list[#list + 1] = c.name
    end
    for _, m in ipairs(type(c.models) == "table" and c.models or {}) do
      if type(m.model) == "string" and not seen[m.model] then
        seen[m.model] = true
        list[#list + 1] = m.model
      end
    end
  end
  choices_cache = { at = now, list = list }
  return list
end

---Salva a escolha e aplica no que já está aberto (avante: sem reiniciar).
---@param model string
---@param opts? { quiet?: boolean }
function M.set_model(model, opts)
  local agents = require "astrograph.agents"
  if type(model) ~= "string" or model == "" then return end
  if agents.read_config().omniroute_model == model and not (opts and opts.force) then return end
  agents.write_config { omniroute_model = model }
  if package.loaded["avante.config"] and agents.has_omniroute() then
    pcall(function()
      require("avante.config").override { providers = { omniroute = M.avante_provider(model) } }
      rawset(require "avante.providers", "omniroute", nil) -- o avante remonta com o modelo novo
    end)
  end
  if not (opts and opts.quiet) then
    local env = vim.env.OMNIROUTE_MODEL
    local extra = (env and env ~= "" and env ~= model) and (" (atenção: OMNIROUTE_MODEL=%s no ambiente vence)"):format(env) or ""
    vim.notify(("OmniRoute: modelo %s (salvo)%s"):format(model, extra), vim.log.levels.INFO, { title = "Astrograph" })
  end
end

---Abre a escolha do modelo/combo: o que o OmniRoute oferece, mais "digitar outro".
---@param cb? fun(model: string)
function M.choose_model(cb)
  local current = require("astrograph.agents").omniroute_model()
  local list = M.model_choices()
  local items = vim.list_extend({}, list)
  items[#items + 1] = "Digitar outro…"
  local function done(model)
    if not model or model == "" then return end
    M.set_model(model)
    if cb then cb(model) end
  end
  vim.ui.select(items, {
    prompt = "Modelo do OmniRoute" .. (current and (" (atual: " .. current .. ")") or " (nenhum escolhido)"),
    format_item = function(it) return it == current and (it .. "  ✓") or it end,
  }, function(choice)
    if choice == "Digitar outro…" then
      vim.ui.input({ prompt = "Modelo ou combo do OmniRoute: ", default = current or "" }, done)
    else
      done(choice)
    end
  end)
end

---Antes de um pedido sem modelo: em vez de chutar um, pergunta.
function M.ask_model()
  vim.notify(
    "OmniRoute: escolha o modelo (fica salvo; troque quando quiser com :OmniRoute modelo) e reenvie o pedido.",
    vim.log.levels.WARN,
    { title = "Astrograph" }
  )
  M.choose_model()
end

---Provedor do avante com o modelo escolhido.
---@param model string
function M.avante_provider(model)
  local agents = require "astrograph.agents"
  local key = vim.env.OMNIROUTE_API_KEY
  return {
    __inherited_from = "openai",
    endpoint = agents.omniroute_url(),
    -- sem chave no ambiente, nome vazio = sem cabeçalho (como nos provedores locais)
    api_key_name = (key and key ~= "") and "OMNIROUTE_API_KEY" or "",
    model = model,
  }
end

---O chat usa o provedor OmniRoute? (adapter `omniroute` do CodeCompanion)
---@param chat? { adapter?: { name?: string } }
function M.chat_selected(chat)
  local a = chat and chat.adapter
  return type(a) == "table" and a.name == "omniroute"
end

local opencode_cache ---@type { at: integer, uses: boolean }?

---O painel do agente passa pelo OmniRoute? Só quando o agente atual é o
---opencode e o modelo padrão dele é do provedor `omniroute/...` (o opencode é
---o único agente do painel que roteia pelo gateway). Relido a cada 30 s.
function M.panel_selected()
  local ok, agents = pcall(require, "astrograph.agents")
  local cur = ok and agents.current() or nil
  if not cur or cur.bin ~= "opencode" then return false end
  local now = os.time()
  if opencode_cache and now - opencode_cache.at < 30 then return opencode_cache.uses end
  local uses = agents.opencode_omniroute() ~= nil
  opencode_cache = { at = now, uses = uses }
  return uses
end

---O texto do aviso (uma linha).
function M.text(change)
  local t = fmt("▸ mudou para %s via OmniRoute", change.to)
  if change.from and change.from ~= "" then t = t .. fmt(" (antes: %s)", change.from) end
  return t
end

---Pinta a linha do aviso: faixa amarela no aviso inteiro e o nome do modelo
---em verde. A linha é lida do buffer, então a posição do nome é sempre certa.
---@param buf integer
---@param row integer linha 0-based do aviso
---@param change { to: string }
function M.decorate(buf, row, change)
  if type(buf) ~= "number" or type(row) ~= "number" or row < 0 then return end
  if not vim.api.nvim_buf_is_valid(buf) then return end
  if row >= vim.api.nvim_buf_line_count(buf) then return end

  local text = vim.api.nvim_buf_get_lines(buf, row, row + 1, false)[1] or ""
  local ok = pcall(vim.api.nvim_buf_set_extmark, buf, M.ns, row, 0, {
    line_hl_group = HL_NOTICE,
  })
  if not ok then return end

  local s = text:find(change.to, 1, true)
  if s then
    pcall(vim.api.nvim_buf_set_extmark, buf, M.ns, row, s - 1, {
      end_col = s - 1 + #change.to,
      hl_group = HL_MODEL,
      priority = 200,
    })
  end
end

---Avisa no chat do CodeCompanion com uma linha VIRTUAL acima do cabeçalho do
---próximo pedido. Quando o `ChatDone` dispara, o chat já escreveu esse
---cabeçalho (`ready_for_input`), e tudo dali para baixo é lido como o próximo
---pedido do usuário: texto de verdade no buffer seria enviado ao modelo e se
---misturaria com o que você digita. A linha virtual só aparece na tela.
---@param chat { bufnr: integer, header_line?: integer }
---@param change { from?: string, to: string }
function M.notice_chat(chat, change)
  local buf = chat.bufnr
  if type(buf) ~= "number" or not vim.api.nvim_buf_is_valid(buf) then return end
  local n = vim.api.nvim_buf_line_count(buf)
  -- âncora: o último cabeçalho de papel ("## Você") do buffer, que é o do
  -- próximo pedido (o CodeCompanion calcula header_line de jeitos diferentes
  -- conforme o caminho; ele fica só de reserva)
  local row
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  for i = #lines, 1, -1 do
    if lines[i]:match "^##%s" then
      row = i - 1
      break
    end
  end
  row = row or math.min(math.max((tonumber(chat.header_line) or n) - 2, 0), n - 1)
  local chunks = { { "▸ mudou para ", HL_NOTICE }, { change.to, HL_MODEL }, { " via OmniRoute", HL_NOTICE } }
  if change.from and change.from ~= "" then chunks[#chunks + 1] = { fmt(" (antes: %s)", change.from), HL_NOTICE } end
  pcall(vim.api.nvim_buf_set_extmark, buf, M.ns, row, 0, { virt_lines = { chunks }, virt_lines_above = true })
end

---Avisa no painel do agente nativo: escreve a linha e pinta. O `write` do
---painel devolve a linha (0-based) onde o texto parou.
---@param write fun(text: string, kind: string): integer
---@param buf integer buffer do painel
function M.notice_panel(write, buf)
  if not M.panel_selected() then return end
  M.after_turn(function(change)
    local row = write(M.text(change), "info")
    M.decorate(buf, row, change)
  end, "panel")
end

-- Checagem rápida: estado atual, para `:OmniRoute`.
function M.report()
  local lines = {}
  if not uv.fs_stat(M.logs_dir()) then
    lines[#lines + 1] = "OmniRoute não encontrado em " .. M.logs_dir()
  else
    lines[#lines + 1] = "registro de chamadas: " .. M.logs_dir()
  end
  local n = 0
  for _, model in pairs(last) do
    n = n + 1
    lines[#lines + 1] = fmt("  conversa %d: %s", n, model)
  end
  if n == 0 then lines[#lines + 1] = "  nenhuma conversa vista ainda" end
  local h = M.health
  lines[#lines + 1] = fmt("detector: %s; %d turno(s) checado(s), modelo achado em %d", M.enabled() and "ligado" or "desligado (vim.g.astrograph_omniroute = false)", h.turns, h.found)
  if h.at then lines[#lines + 1] = fmt("  último: %s às %s", h.model or "?", os.date("%H:%M:%S", h.at)) end
  if h.turns > 0 and h.found == 0 then lines[#lines + 1] = "  nenhum log com resposta 2xx lido: o formato do log pode ter mudado" end
  return lines
end

return M
