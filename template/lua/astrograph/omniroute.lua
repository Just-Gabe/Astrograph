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
--   ▸ mudando modelo para <modelo> via OmniRoute (antes: <modelo>)
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
local pending

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

---Diretório de log de hoje e de ontem (o dia pode virar no meio do turno).
local function day_dirs(at)
  local dirs = {}
  for _, off in ipairs { 0, -86400 } do
    local t = os.date("*t", at + off)
    dirs[#dirs + 1] = fmt("%s/%04d-%02d-%02d", M.logs_dir(), t.year, t.month, t.day)
  end
  return dirs
end

---Logs gravados depois de `since` (epoch), do mais novo para o mais antigo.
local function recent_logs(since)
  local found = {}
  for _, dir in ipairs(day_dirs(os.time())) do
    if uv.fs_stat(dir) then
      local ok, names = pcall(vim.fn.readdir, dir)
      if ok then
        for _, name in ipairs(names) do
          if name:sub(-5) == ".json" then
            local p = dir .. "/" .. name
            local stt = uv.fs_stat(p)
            local m = stt and stt.mtime.sec
            if m and m >= since then found[#found + 1] = { path = p, name = name } end
          end
        end
      end
    end
  end
  -- o nome começa com o timestamp ISO (ms): ordena melhor que o mtime (em segundos)
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
local heads = {} ---@type table<string, table|false> resumo por caminho (o log não muda depois de gravado)

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
  if not at then
    heads[path] = false
    return nil
  end
  head = head:sub(at)
  local s = {
    model = field(head, "model"),
    status = tonumber(field(head, "status")),
    combo = field(head, "comboName"),
    provider = field(head, "provider"),
    requested = field(head, "requestedModel"),
  }
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
    if type(m) == "table" and m.role == "user" and type(m.content) == "string" and m.content ~= "" then
      return vim.fn.sha256(m.content)
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

---Marca o começo de um turno (chamado ao enviar um pedido).
function M.mark()
  pending = os.time()
end

---Depois de um turno, procura o modelo no registro de chamadas e chama `cb`
---com a mudança — se houve. O arquivo é gravado ao fim da resposta, então
---tentamos algumas vezes antes de desistir.
---@param cb fun(change: { from: string, to: string, provider: string?, combo: string? })
function M.after_turn(cb)
  if not uv.fs_stat(M.logs_dir()) then return end -- OmniRoute não está instalado
  local now = os.time()
  local since = pending
  pending = nil
  -- sem marca (ou marca velha demais): assume que a chamada acabou de ocorrer
  if not since or since > now or now - since > 900 then since = now - 5 end

  local tries = 0
  local function attempt()
    tries = tries + 1
    local info
    for _, log in ipairs(recent_logs(since - 2)) do
      info = read_log(log.path)
      if info then break end
    end
    if not info then
      if tries < 8 then return vim.defer_fn(attempt, 150) end
      return
    end
    local change = M.evaluate(info)
    if change then vim.schedule(function() cb(change) end) end
  end
  attempt()
end

-- ------------------------------------------------------------------ desenho

---Cor do aviso: nome do modelo em verde, faixa amarela no aviso inteiro.
function M.setup()
  vim.api.nvim_set_hl(0, HL_MODEL, { fg = "#0b6e0b", bold = true })
  vim.api.nvim_set_hl(0, HL_NOTICE, { bg = "#f2d16b", fg = "#3b3b2a" })
end

---Liga os destaques ao chat do CodeCompanion (`ChatDone`/`ChatSubmitted`) e
---deixa as cores definidas. Chamado uma vez, na carga do plugin.
function M.setup_chat()
  M.setup()
  vim.api.nvim_create_autocmd("ColorScheme", { group = vim.api.nvim_create_augroup("astrograph_omniroute", {}), callback = M.setup })
  vim.api.nvim_create_user_command("OmniRoute", function()
    vim.notify(table.concat(M.report(), "\n"), vim.log.levels.INFO, { title = "OmniRoute" })
  end, { desc = "Estado do OmniRoute (modelo por conversa)" })

  local group = vim.api.nvim_create_augroup("astrograph_omniroute_chat", {})
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionChatSubmitted",
    callback = function() M.mark() end,
  })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionChatDone",
    callback = function(args)
      local buf = args.data and args.data.bufnr
      if not buf then return end
      M.after_turn(function(change)
        local ok, cc = pcall(require, "codecompanion")
        if not ok then return end
        local chat = cc.buf_get_chat(buf)
        if not chat or not vim.api.nvim_buf_is_valid(chat.bufnr) then return end
        -- o aviso sai como um bloco só dele, sem entrar no histórico do modelo
        -- (nada é reenviado ao LLM no próximo turno)
        local last_line = chat:add_buf_message { content = "\n" .. M.text(change) }
        if last_line then M.decorate(chat.bufnr, last_line - 1, change) end
      end)
    end,
  })
end

---O texto do aviso (uma linha).
function M.text(change)
  local t = fmt("▸ mudando modelo para %s via OmniRoute", change.to)
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

---Avisa no painel do agente nativo: escreve a linha e pinta. O `write` do
---painel devolve a linha (0-based) onde o texto parou.
---@param write fun(text: string, kind: string): integer
---@param buf integer buffer do painel
function M.notice_panel(write, buf)
  M.after_turn(function(change)
    local row = write(M.text(change), "info")
    M.decorate(buf, row, change)
  end)
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
  return lines
end

return M
