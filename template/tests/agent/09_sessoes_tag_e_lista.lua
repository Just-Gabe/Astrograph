-- Sessões: a gravação do avante ganha a tag do modelo, o índice lista conversas
-- de todos os projetos sem abrir as grandes, e a pasta do projeto é reconhecida
-- com a mesma regra de nome do avante.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local proj = H.workdir {}
local storage = vim.fs.normalize(vim.fn.tempname())

-- avante falso: só o que o módulo usa
local saved = {}
package.preload["avante.config"] = function()
  return {
    provider = "claude-code",
    acp_providers = { ["claude-code"] = {} },
    providers = {},
    history = { storage_path = storage },
  }
end
package.preload["avante"] = function()
  return { get = function() return { acp_client = { config_options = {
    { category = "model", currentValue = "opus", options = { { value = "opus", name = "Opus" } } },
  } } } end }
end
package.preload["avante.utils"] = function() return { root = { get = function() return proj end } } end
package.preload["avante.path"] = function()
  return { history = { save = function(_, h) saved[#saved + 1] = vim.deepcopy(h) end } }
end

local S = require "astrograph.sessions"

-- regra de nome igual à do avante (caminho real desta máquina)
H.eq(S.project_dirname "C:\\Users\\GabrielGonçalvesAzev", "C__Users_GabrielGon__alvesAzev", "nome da pasta do projeto")

S.setup()
local P = require "avante.path"
P.history.save(0, { filename = "1.json", title = "revisar franquia\nsegunda linha", messages = { {}, {} } })
H.eq(saved[1].astrograph.provider, "claude-code", "tag: agente")
H.eq(saved[1].astrograph.model, "Opus", "tag: modelo")
-- trocar de modelo no meio não reescreve a origem: entra em `others`
local Cfg = require "avante.config"
Cfg.provider, Cfg.providers = "ollama", { ollama = { model = "qwen3" } }
P.history.save(0, saved[1])
H.eq(saved[2].astrograph.model, "Opus", "tag: modelo de origem mantido")
H.eq(saved[2].astrograph.others[1], "ollama · qwen3", "tag: modelo trocado registrado")
H.eq(S.tag_label(S.index()[S.project_dirname(proj) .. "/1.json"]), "claude-code · Opus +1", "rótulo com troca")
Cfg.provider, Cfg.providers = "claude-code", {}
S.flush()

local dir = S.project_dirname(proj)
local hist = vim.fs.joinpath(storage, "projects", dir, "history")
vim.fn.mkdir(hist, "p")
vim.fn.writefile({ vim.json.encode(saved[1]) }, hist .. "/1.json")
-- conversa antiga (sem índice), pequena: título lido do arquivo
vim.fn.writefile({ vim.json.encode { title = "antiga", messages = { {} } } }, hist .. "/2.json")
-- conversa antiga grande: não é aberta
S.HEAVY = 100
local big = { title = "enorme", messages = {} }
for i = 1, 50 do big.messages[i] = { content = ("x"):rep(40) } end
vim.fn.writefile({ vim.json.encode(big) }, hist .. "/3.json")
-- a pasta do projeto vem do metadata.json do próprio avante
vim.fn.writefile({ vim.json.encode { latest_filename = "1.json", project_root = proj } }, hist .. "/metadata.json")
vim.cmd.cd(vim.fs.dirname(proj)) -- a pasta atual não ajuda mais

-- índice relido do disco, como numa sessão nova do editor
package.loaded["astrograph.sessions"] = nil
S = require "astrograph.sessions"
S.HEAVY = 100
local by = {}
for _, s in ipairs(S.list()) do by[s.filename] = s end
H.ok(by["metadata.json"] == nil, "metadata.json não é conversa")
H.eq(by["1.json"].title, "revisar franquia", "título do índice (1ª linha)")
H.eq(S.tag_label(by["1.json"]), "claude-code · Opus +1", "rótulo da tag relido do índice")
H.eq(by["1.json"].root, proj, "pasta do projeto do índice")
H.eq(by["2.json"].title, "antiga", "título lido de conversa pequena")
H.eq(by["2.json"].root, proj, "pasta reconhecida pelo metadata.json")
H.eq(S.tag_label(by["2.json"]), "sem tag (antiga)", "conversa antiga sem tag")
H.ok(by["3.json"].size > S.HEAVY, "grande detectada")

-- dois editores abertos: o segundo a gravar não apaga o que o primeiro gravou
local idx_file = vim.fs.joinpath(storage, "astrograph-sessoes.json")
local outro = vim.json.decode(table.concat(vim.fn.readfile(idx_file), "\n"))
outro.sessions["outro/9.json"] = { key = "outro/9.json", root = proj, title = "do outro editor", updated = os.time() + 60 }
vim.fn.writefile({ vim.json.encode(outro) }, idx_file)
P = require "avante.path"
package.loaded["astrograph.sessions"] = nil
S = require "astrograph.sessions"
S.index() -- carregado antes de o outro editor gravar de novo
S.tag(0, { filename = "4.json", title = "deste editor", messages = { {} } })
S.flush()
local final = vim.json.decode(table.concat(vim.fn.readfile(idx_file), "\n")).sessions
H.ok(final["outro/9.json"] ~= nil, "entrada do outro editor preservada")
H.ok(final[dir .. "/4.json"] ~= nil, "entrada deste editor gravada")

-- conversa apagada no disco sai do índice (e não volta pela mesclagem)
os.remove(hist .. "/2.json")
S.list()
S.flush()
final = vim.json.decode(table.concat(vim.fn.readfile(idx_file), "\n")).sessions
H.ok(final[dir .. "/2.json"] == nil, "conversa apagada saiu do índice")
H.done()
