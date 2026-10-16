-- OmniRoute como provedor: registrado no chat (CodeCompanion) e no avante
-- quando instalado, mas NUNCA escolhido como padrão, mesmo sendo o único
-- provedor disponível; o aviso de troca só roda quando ele é o escolhido.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")

-- agentes falsos: só o OmniRoute instalado, nenhum outro provedor nem chave
for _, k in ipairs { "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY", "ASTROGRAPH_LOCAL_URL" } do vim.env[k] = nil end
local current = nil
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

-- avante
local av = spec("avante").opts(nil, {})
H.ok(av.providers.omniroute and av.providers.omniroute.endpoint == "http://localhost:20128/v1", "avante: provedor omniroute registrado")
H.ok(av.provider ~= "omniroute", "avante: omniroute não é o padrão")

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
local cfg = vim.fs.normalize(vim.fn.tempname())
vim.fn.mkdir(cfg .. "/opencode", "p")
vim.env.XDG_CONFIG_HOME = cfg
vim.fn.writefile({ '{ "model": "omniroute/gratis" }' }, cfg .. "/opencode/opencode.jsonc")
current = { bin = "claude" }
H.ok(not O.panel_selected(), "painel: Claude Code não passa pelo OmniRoute")
current = { bin = "opencode" }
H.ok(O.panel_selected(), "painel: opencode com modelo omniroute/...")
H.done()
