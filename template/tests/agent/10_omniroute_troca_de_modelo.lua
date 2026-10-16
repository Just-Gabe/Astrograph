-- OmniRoute: o aviso de troca de modelo usa a tentativa que respondeu (2xx),
-- não o log agregado do combo nem as tentativas que falharam.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local home = vim.fs.normalize(vim.fn.tempname())
vim.env.OMNIROUTE_HOME = home
local t = os.date("*t")
local day = ("%s/call_logs/%04d-%02d-%02d"):format(home, t.year, t.month, t.day)
vim.fn.mkdir(day, "p")

local seq = 0
local function log(model, status)
  seq = seq + 1
  local name = ("%04d-%02d-%02dT10-00-%02d.000Z_%d.json"):format(t.year, t.month, t.day, seq, seq)
  vim.fn.writefile({ vim.json.encode {
    summary = { model = model, status = status, comboName = "gratis", provider = "ddgw" },
    requestBody = { messages = { { role = "user", content = "primeiro pedido" } } },
  } }, day .. "/" .. name)
end

local O = require "astrograph.omniroute"
local changes = {}
local function turn()
  O.mark()
  O.after_turn(function(c) changes[#changes + 1] = c end)
  vim.wait(300, function() return false end)
end

-- turno 1: gpt-4o respondeu; o agregado do combo (model = "gratis") vem depois
log("gpt-4o", 200)
log("gratis", 200)
turn()
H.eq(#changes, 0, "primeiro turno não é troca")
H.eq(O.state()[vim.fn.sha256 "primeiro pedido"], "gpt-4o", "modelo real, não o nome do combo")

-- turno 2: gpt-4o falhou (400), gpt-4o-mini respondeu, depois o agregado
log("gpt-4o", 400)
log("gpt-4o-mini", 200)
log("gratis", 200)
turn()
H.eq(#changes, 1, "troca detectada")
H.eq(changes[1].from, "gpt-4o", "de")
H.eq(changes[1].to, "gpt-4o-mini", "para (a tentativa que respondeu)")

-- turno 3: tudo falhou (o caso de hoje: 400 em todos os passos): sem aviso falso
log("gpt-4o", 400)
log("gpt-4o-mini", 400)
log("gratis", 400)
turn()
H.eq(#changes, 1, "falha geral não vira troca")
H.done()
