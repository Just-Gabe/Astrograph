-- :Astrograph atualizar pelo serviço em Rust (executável falso que devolve o JSON
-- do contrato) e a reserva em Lua quando ele não existe.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
if vim.fn.has "win32" == 1 then
  H.done() -- o executável falso é um script sh
  return
end
local U = require "astrograph.update"
local root = vim.fs.normalize(vim.fn.tempname())
vim.fn.mkdir(root .. "/bin", "p")

H.eq(U.servicos(root), nil, "sem executável: reserva em Lua")

local function falso(corpo)
  local p = root .. "/bin/astrograph-servicos"
  vim.fn.writefile(vim.split("#!/bin/sh\n" .. corpo, "\n"), p)
  vim.uv.fs_chmod(p, tonumber("755", 8))
  return p
end
local function rodar(bin)
  local res
  U.run_rust(bin, root, function(r) res = r end)
  H.ok(vim.wait(10000, function() return res ~= nil end, 20), "terminou")
  return res
end

local bin = falso([[echo '{"status":"atualizado","mensagem":"Astrograph atualizado (a -> b).","antes":"a","depois":"b","stash":null,"backup":null,"recompilados":["astrograph-ui"]}']])
H.eq(U.servicos(root), bin, "acha o executável")
local r = rodar(bin)
H.eq(r.status, "atualizado", "status")
H.eq(r.new, "b", "versão nova")
H.eq(r.rebuilt[1], "astrograph-ui", "recompilados")

-- recebe a raiz e o nvim em uso
bin = falso([[echo "{\"status\":\"ja-atualizado\",\"mensagem\":\"$*\"}"]])
r = rodar(bin)
H.eq(r.status, "ja-atualizado", "outro status")
H.ok(r.message:find("atualizar --raiz " .. root, 1, true) ~= nil, "argumentos: " .. r.message)
H.ok(r.message:find("--nvim", 1, true) ~= nil, "passa o nvim")

-- sem JSON (o serviço caiu): erro com o que ele disse, sem cair em silêncio
bin = falso([[echo "panic: boom" >&2; exit 101]])
r = rodar(bin)
H.eq(r.status, "erro", "sem resultado vira erro")
H.ok(r.message:find("panic: boom", 1, true) ~= nil, "mostra o motivo")

-- ASTROGRAPH_ATUALIZAR=lua força a reserva
vim.env.ASTROGRAPH_ATUALIZAR = "lua"
H.eq(U.servicos(root), nil, "variável força o Lua")
H.done()
