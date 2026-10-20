-- Sessões: conversa que passa do teto manda as mensagens antigas para
-- history-arquivo/ (sem apagar) e arquivos grandes saem de history/.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local proj = H.workdir {}
local storage = vim.fs.normalize(vim.fn.tempname())

package.preload["avante.config"] = function()
  return { provider = "claude-code", acp_providers = { ["claude-code"] = {} }, providers = {}, history = { storage_path = storage } }
end
package.preload["avante.utils"] = function() return { root = { get = function() return proj end } } end

local S = require "astrograph.sessions"
S.CAP, S.KEEP = 10, 4
local dir = S.project_dirname(proj)
local arq = vim.fs.joinpath(storage, "projects", dir, "history-arquivo")

-- dentro do teto: nada muda
local pequena = { filename = "1.json", messages = { {}, {}, {} } }
H.eq(S.cap(dir, pequena), nil, "abaixo do teto não arquiva")
H.eq(#pequena.messages, 3, "abaixo do teto: mensagens intactas")

-- acima do teto: ficam as KEEP recentes; as antigas vão inteiras para o arquivo
local h = { filename = "2.json", title = "longa", messages = {} }
for i = 1, 12 do
  h.messages[i] = { n = i, message = { role = "user" } }
end
H.eq(S.cap(dir, h), 8, "arquivou as mais antigas")
H.eq(#h.messages, 4, "ficaram só as recentes")
H.eq(h.messages[1].n, 9, "a mais antiga que ficou")
local files = vim.fn.readdir(arq)
H.eq(#files, 1, "um arquivo no history-arquivo")
local salvo = vim.json.decode(table.concat(vim.fn.readfile(vim.fs.joinpath(arq, files[1])), "\n"))
H.eq(#salvo.messages, 8, "arquivo guarda as 8 antigas")
H.eq(salvo.messages[1].n, 1, "arquivo começa na primeira mensagem")

-- o corte não separa a resposta de ferramenta da mensagem do usuário
local t = { filename = "3.json", messages = {} }
for i = 1, 12 do
  t.messages[i] = { n = i, message = { role = (i % 3 == 0) and "user" or "assistant" } }
end
S.cap(dir, t)
H.eq(t.messages[1].message.role, "user", "a parte que fica começa numa mensagem do usuário")

-- arquivo grande de antes do teto sai de history/ sem ser apagado
local hist = vim.fs.joinpath(storage, "projects", dir, "history")
vim.fn.mkdir(hist, "p")
S.HEAVY = 100
vim.fn.writefile({ ("x"):rep(500) }, hist .. "/grande.json")
vim.fn.writefile({ "{}" }, hist .. "/pequena.json")
vim.fn.writefile({ "{}" }, hist .. "/metadata.json")
local moved = S.rotate_big(proj)
H.eq(#moved, 1, "só uma foi movida")
H.eq(moved[1], "grande.json", "a movida é a grande")
H.ok(vim.uv.fs_stat(hist .. "/grande.json") == nil, "saiu de history/")
H.ok(vim.uv.fs_stat(hist .. "/pequena.json") ~= nil, "pequena fica")
H.ok(vim.uv.fs_stat(hist .. "/metadata.json") ~= nil, "metadata.json fica")
local achou = false
for _, f in ipairs(vim.fn.readdir(arq)) do
  if f:match "^grande%." then achou = true end
end
H.ok(achou, "grande está em history-arquivo/")
H.done()
