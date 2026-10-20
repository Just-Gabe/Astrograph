-- Textobjetos pelo app: com a marca do app no buffer, as leituras de árvore do
-- plugin viram rpcrequest; `false`, erro ou sem marca caem no plugin.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")

-- plugin falso: só o que o módulo troca
local calls = { at = 0, best = 0 }
package.preload["nvim-treesitter-textobjects.shared"] = function()
  return {
    textobject_at_point = function() calls.at = calls.at + 1; return { 9, 9, 99, 9, 12, 102 } end,
    find_best_range = function() calls.best = calls.best + 1; return { 7, 7, 77, 7, 8, 78 } end,
  }
end
local T = require "astrograph.textobjects"
T.patch()
T.patch() -- idempotente
local shared = require "nvim-treesitter-textobjects.shared"

local buf = vim.api.nvim_get_current_buf()

-- sem marca: plugin
local r = shared.textobject_at_point("@function.outer", "textobjects", buf, { 1, 0 })
H.eq(r[1], 9, "sem marca: plugin")
H.eq(calls.at, 1, "plugin chamado")

-- com marca e resposta: usa o app (rpcrequest trocado por um falso)
local answers = {}
local real = vim.rpcrequest
vim.rpcrequest = function(ch, name, ...)
  answers[#answers + 1] = { ch = ch, name = name, args = { ... } }
  return answers.next
end
vim.b[buf].astrograph_textobjects = 7
answers.next = { 2, 0, 20, 5, 3, 60 }
r = shared.textobject_at_point("@function.outer", "textobjects", buf, { 3, 4 }, { lookahead = true })
H.eq(r[1], 2, "com marca: resposta do app")
H.eq(calls.at, 1, "plugin não chamado")
H.eq(answers[1].name, "astrograph_textobject", "método")
H.eq(answers[1].args[3], "@function.outer", "captura")
H.eq(answers[1].args[4], 2, "linha 0-based")
H.eq(answers[1].args[5], 4, "coluna")
H.eq(answers[1].args[6], true, "lookahead repassado")

-- sem objeto (nil): nil, sem cair no plugin
answers.next = vim.NIL
H.eq(shared.textobject_at_point("@function.outer", "textobjects", buf, { 1, 0 }), nil, "nil do app é 'sem objeto'")
H.eq(calls.at, 1, "ainda sem plugin")

-- false (app atrasado/sem a linguagem): plugin
answers.next = false
r = shared.textobject_at_point("@function.outer", "textobjects", buf, { 1, 0 })
H.eq(r[1], 9, "false cai no plugin")
H.eq(calls.at, 2, "plugin chamado de novo")

-- outro grupo de consultas: plugin
answers.next = { 1, 1, 1, 1, 1, 1 }
shared.textobject_at_point("@x", "folds", buf, { 1, 0 })
H.eq(calls.at, 3, "grupo diferente vai ao plugin")

-- erro de RPC (app morreu): plugin
vim.rpcrequest = function() error "canal fechado" end
r = shared.textobject_at_point("@function.outer", "textobjects", buf, { 1, 0 })
H.eq(r[1], 9, "erro de RPC cai no plugin")

-- find_best_range: o app manda os trechos, o filtro e a pontuação são do chamador
vim.rpcrequest = function(_, name) answers.last = name; return { { 1, 0, 10, 2, 0, 20 }, { 5, 0, 50, 6, 0, 60 }, { 9, 0, 90, 10, 0, 100 } } end
local best = shared.find_best_range(buf, "@function.outer", "textobjects", function(rg) return rg[1] > 2 end, function(rg) return -rg[3] end)
H.eq(answers.last, "astrograph_textobject_ranges", "método dos trechos")
H.eq(best[1], 5, "o mais perto depois do cursor vence")
H.eq(shared.find_best_range(buf, "@function.outer", "textobjects", function() return false end, function() return 0 end), nil, "nada passa no filtro")
vim.rpcrequest = function() return false end
H.eq(shared.find_best_range(buf, "@function.outer", "textobjects", function() return true end, function() return 0 end)[1], 7, "false cai no plugin")
vim.rpcrequest = real
H.done()
