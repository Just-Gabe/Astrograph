-- OmniRoute: o aviso de troca de modelo usa a tentativa que respondeu (2xx),
-- não o log agregado do combo nem as tentativas que falharam; espera o log do
-- último pedido do turno; e aparece no painel e no chat sem virar texto do
-- pedido seguinte.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local home = vim.fs.normalize(vim.fn.tempname())
vim.env.OMNIROUTE_HOME = home
local u = os.date("!*t")
local day = ("%s/call_logs/%04d-%02d-%02d"):format(home, u.year, u.month, u.day)
vim.fn.mkdir(day, "p")

local O = require "astrograph.omniroute"
O.SETTLE_MS = 20

-- nome como o do OmniRoute: carimbo UTC com ms + id (sempre depois da marca)
local seq = 0
local function log(model, status, opts)
  opts = opts or {}
  seq = seq + 1
  local name = ("%s_%04d.json"):format(O.stamp(), seq)
  local body = vim.json.encode {
    summary = { model = model, status = status, comboName = "gratis", provider = "ddgw" },
    requestBody = { model = "gratis", messages = { { role = "user", content = "primeiro pedido" } } },
  }
  if opts.partial then body = body:sub(1, 10) end -- ainda sendo gravado
  vim.fn.writefile({ body }, day .. "/" .. name)
  vim.wait(3) -- carimbo seguinte com outro ms
  return day .. "/" .. name
end

local changes = {}
local function turn(write_logs, src)
  O.mark(src)
  write_logs()
  O.after_turn(function(c) changes[#changes + 1] = c end, src)
  vim.wait(600, function() return false end)
end

-- turno 1: gpt-4o respondeu; o agregado do combo (model = "gratis") vem depois
turn(function()
  log("gpt-4o", 200)
  log("gratis", 200)
end)
H.eq(#changes, 0, "primeiro turno não é troca")
H.eq(O.state()[vim.fn.sha256 "primeiro pedido"], "gpt-4o", "modelo real, não o nome do combo")

-- turno 2 (agêntico): um pedido antigo do turno com gpt-4o, depois o gpt-4o
-- falha e o gpt-4o-mini responde; o último log chega DEPOIS do fim do turno
O.mark()
log("gpt-4o", 200)
log("gpt-4o", 400)
O.after_turn(function(c) changes[#changes + 1] = c end)
vim.defer_fn(function()
  log("gpt-4o-mini", 200)
  log("gratis", 200)
end, 40)
vim.wait(600, function() return false end)
H.eq(#changes, 1, "troca detectada")
H.eq(changes[1].from, "gpt-4o", "de")
H.eq(changes[1].to, "gpt-4o-mini", "para: o último pedido do turno, mesmo gravado depois do fim")

-- turno 3: tudo falhou (o caso de 16/10: 400 em todos os passos): sem aviso falso
turn(function()
  log("gpt-4o", 400)
  log("gpt-4o-mini", 400)
  log("gratis", 400)
end)
H.eq(#changes, 1, "falha geral não vira troca")

-- leitura no meio da gravação não fica presa no cache
local p = log("gpt-4o", 200, { partial = true })
H.eq(O._read_log(p), nil, "parcial: sem resumo ainda")
vim.fn.writefile({ vim.json.encode { summary = { model = "gpt-4o", status = 200 }, requestBody = {} } }, p)
H.eq((O._read_log(p) or {}).model, "gpt-4o", "parcial: relido depois de completo")

-- painel do agente nativo: o fim de turno de verdade (handlers.done) escreve o
-- aviso quando o agente do painel passa pelo OmniRoute (teste 11 cobre a regra)
O.panel_selected = function() return true end
local ag = H.agent()
O.mark "panel"
log("gpt-4o-mini", 400)
log("gpt-4o", 200)
H.event(ag, { ev = "done", stop = "end_turn" })
local pbuf = ag._state.buf
vim.wait(2000, function() return H.lines(pbuf):find("mudou para gpt-4o ", 1, true) ~= nil end, 20)
H.ok(H.lines(pbuf):find("▸ mudou para gpt-4o via OmniRoute (antes: gpt-4o-mini)", 1, true), "aviso no painel: " .. H.lines(pbuf))

-- chat do CodeCompanion: linha VIRTUAL acima do cabeçalho do próximo pedido;
-- o texto do buffer (que o chat envia ao modelo) não muda
local cbuf = vim.api.nvim_create_buf(false, true)
local before = { "## Assistente", "", "resposta", "", "## Me", "" }
vim.api.nvim_buf_set_lines(cbuf, 0, -1, false, before)
O.notice_chat({ bufnr = cbuf, header_line = 6 }, { from = "gpt-4o", to = "gpt-4o-mini" })
H.eq(H.lines(cbuf), table.concat(before, "|"), "chat: texto do buffer intacto")
local marks = vim.api.nvim_buf_get_extmarks(cbuf, O.ns, 0, -1, { details = true })
H.eq(#marks, 1, "chat: uma marca")
H.eq(marks[1][2], 4, "chat: na linha do cabeçalho do usuário")
H.ok(marks[1][4].virt_lines and marks[1][4].virt_lines_above, "chat: linha virtual acima")
local shown = {}
for _, c in ipairs(marks[1][4].virt_lines[1]) do shown[#shown + 1] = c[1] end
H.eq(table.concat(shown), "▸ mudou para gpt-4o-mini via OmniRoute (antes: gpt-4o)", "chat: texto do aviso")

-- desligado: nada é lido
vim.g.astrograph_omniroute = false
local n = O.health.turns
O.after_turn(function() end)
H.eq(O.health.turns, n, "desligado não checa")
H.done()
