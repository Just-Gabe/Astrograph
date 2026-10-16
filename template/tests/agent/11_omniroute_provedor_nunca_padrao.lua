-- OmniRoute como provedor: registrado no chat (CodeCompanion) e no avante
-- quando instalado, mas NUNCA escolhido como padrão, mesmo sendo o único
-- provedor disponível; o aviso de troca só roda quando ele é o escolhido.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")

-- agentes falsos: só o OmniRoute instalado, nenhum outro provedor nem chave
for _, k in ipairs { "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY", "ASTROGRAPH_LOCAL_URL" } do vim.env[k] = nil end
local current, combo, chosen = nil, "gratis", nil
-- config.json do teste numa pasta temporária (nunca a escolha real do usuário)
local tmp = vim.fs.normalize(vim.fn.tempname())
vim.fn.mkdir(tmp, "p")
vim.env.APPDATA, vim.env.LOCALAPPDATA, vim.env.XDG_CONFIG_HOME = tmp, tmp, tmp
vim.env.OMNIROUTE_MODEL = nil
-- as funções reais de modelo/config são testadas no fim
local real = dofile(H.template .. "/lua/astrograph/agents.lua")
package.loaded["astrograph.agents"] = {
  agents = {},
  detect = function() return {} end,
  available = function() return {} end,
  current = function() return current end,
  has = function() return false end,
  has_acp = function() return false end,
  local_url = function() return nil end,
  has_omniroute = function() return true end,
  omniroute_url = function() return "http://localhost:20128/v1" end,
  omniroute_model = function() return chosen end,
  opencode_omniroute = function() return combo end,
}

local function spec(name)
  local s = dofile(H.template .. "/lua/plugins/" .. name .. ".lua")
  return s[1] or s
end

-- chat
local cc = spec("codecompanion").opts(nil, {})
H.ok(type(cc.adapters.http.omniroute) == "function", "chat: provedor omniroute registrado")
H.ok(cc.interactions.chat.adapter ~= "omniroute", "chat: omniroute não é o padrão do chat")
for name, v in pairs(cc.interactions) do
  if type(v) == "table" then H.ok(v.adapter ~= "omniroute", "chat: omniroute não é padrão de " .. name) end
end

-- avante: sem modelo escolhido, nada de provedor com modelo chutado
local av = spec("avante").opts(nil, {})
H.eq(av.providers.omniroute, nil, "avante: sem escolha, sem provedor")
chosen = "meu-combo"
av = spec("avante").opts(nil, {})
H.ok(av.providers.omniroute and av.providers.omniroute.endpoint == "http://localhost:20128/v1", "avante: provedor registrado após a escolha")
H.eq(av.providers.omniroute.model, "meu-combo", "avante: com o modelo escolhido")
H.ok(av.provider ~= "omniroute", "avante: omniroute não é o padrão")
chosen = nil

-- sem o OmniRoute instalado, ele nem aparece
package.loaded["astrograph.agents"].has_omniroute = function() return false end
H.ok(spec("codecompanion").opts(nil, {}).adapters.http.omniroute == nil, "chat: sem OmniRoute, sem provedor")
H.ok(spec("avante").opts(nil, {}).providers.omniroute == nil, "avante: sem OmniRoute, sem provedor")

-- aviso: só no chat cujo adapter é o OmniRoute
local O = require "astrograph.omniroute"
H.ok(O.chat_selected { adapter = { name = "omniroute" } }, "aviso: chat no OmniRoute")
H.ok(not O.chat_selected { adapter = { name = "claude_code" } }, "aviso: chat em outro provedor")
H.ok(not O.chat_selected(nil), "aviso: sem chat")

-- painel: só com o opencode configurado para o OmniRoute
current = { bin = "claude" }
H.ok(not O.panel_selected(), "painel: Claude Code não passa pelo OmniRoute")
current = { bin = "opencode" }
H.ok(O.panel_selected(), "painel: opencode com modelo omniroute/...")

-- modelo (funções reais): só o que VOCÊ escolheu; nada de palpite
vim.fn.mkdir(tmp .. "/opencode", "p")
H.eq(real.opencode_omniroute(), nil, "opencode: sem config")
H.eq(real.omniroute_model(), nil, "modelo: sem escolha, nenhum (nem auto)")
vim.fn.writefile({ '{ "model": "omniroute/gratis", "provider": {} }' }, tmp .. "/opencode/opencode.jsonc")
H.eq(real.opencode_omniroute(), "gratis", "opencode: combo lido (para o painel)")
H.eq(real.omniroute_model(), nil, "modelo: o combo do opencode NÃO vira padrão")
-- a escolha fica salva (config.json) e vale até você trocar
package.loaded["astrograph.agents"] = real
O.set_model("combo-a", { quiet = true })
H.eq(real.read_config().omniroute_model, "combo-a", "escolha: salva no config.json")
H.eq(real.omniroute_model(), "combo-a", "escolha: usada")
O.set_model("combo-b", { quiet = true })
H.eq(real.omniroute_model(), "combo-b", "escolha: trocada quando você quer")
H.ok(vim.fn.filereadable(tmp .. "/astrograph/config.json") == 1, "escolha: no config.json da pasta do teste")
vim.env.OMNIROUTE_MODEL = "do-ambiente"
H.eq(real.omniroute_model(), "do-ambiente", "escolha: OMNIROUTE_MODEL vence a salva")
vim.env.OMNIROUTE_MODEL = nil
vim.fn.writefile({ '{ "model": "anthropic/claude" }' }, tmp .. "/opencode/opencode.jsonc")
H.eq(real.opencode_omniroute(), nil, "opencode: em outro provedor")

-- linha virtual ancorada no último cabeçalho, mesmo com header_line errado
local cbuf = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_lines(cbuf, 0, -1, false, { "## Você", "oi", "", "## OmniRoute", "resposta", "", "## Você", "" })
O.notice_chat({ bufnr = cbuf, header_line = 99 }, { to = "gpt-4o" })
local m = vim.api.nvim_buf_get_extmarks(cbuf, O.ns, 0, -1, {})
H.eq(m[1][2], 6, "âncora: o último cabeçalho (o do próximo pedido)")

-- instaladores: o gateway fica fora do "a = todos"
local function read(path) return table.concat(vim.fn.readfile(path), "\n") end
local root = vim.fs.dirname(H.template)
H.ok(read(root .. "/install.ps1"):find("$Items[$i].Id -ne 'omniroute'", 1, true), "install.ps1: omniroute fora do todos")
H.ok(read(root .. "/install.sh"):find('"${AGENT_IDS[$i]}" != omniroute', 1, true), "install.sh: omniroute fora do todos")
H.done()
