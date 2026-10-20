-- Preparo em segundo plano: detecta o projeto, roda o git sem escrever nada,
-- mostra o ícone só enquanto há trabalho e some ao acabar.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
vim.g.astrograph_prepare_native = false
local P = require "astrograph.prepare"

local function icon_wins()
  local n = 0
  for _, w in ipairs(vim.api.nvim_list_wins()) do
    local c = vim.api.nvim_win_get_config(w)
    if c.relative == "editor" and c.zindex == 250 then n = n + 1 end
  end
  return n
end

-- projeto: linguagens pela raiz e pacotes do Mason
local proj = H.workdir { ["Cargo.toml"] = "[package]", ["package.json"] = "{}", ["init.lua"] = "" }
local langs = P.detect(proj)
for _, l in ipairs { "rust", "toml", "javascript", "json", "lua" } do
  H.ok(vim.tbl_contains(langs, l), "detectou " .. l)
end
H.ok(not vim.tbl_contains(langs, "python"), "não inventa python")
local pk = P.mason_packages { "rust", "typescript", "javascript", "lua" }
H.eq(#pk, 2, "pacotes sem repetição (rust-analyzer + typescript-language-server)")
H.eq(pk[1], "rust-analyzer", "pacote do rust")

-- ícone: só depois de SHOW_AFTER, e some quando libera
vim.o.winborder = "rounded" -- o ícone não pode herdar borda
P.SHOW_AFTER = 50
local release = P.track "teste"
H.eq(icon_wins(), 0, "ícone não aparece de imediato")
H.ok(vim.wait(2000, function() return icon_wins() == 1 end, 20), "ícone aparece enquanto ocupado")
local c = vim.api.nvim_win_get_config(vim.tbl_filter(function(w) return vim.api.nvim_win_get_config(w).zindex == 250 end, vim.api.nvim_list_wins())[1])
H.eq(c.border == nil or c.border == "none" or (type(c.border) == "table" and c.border[1] == ""), true, "sem borda")
H.ok(c.col >= vim.o.columns - 4, "ícone no canto direito")
H.ok(c.row >= vim.o.lines - 4, "ícone embaixo")
H.eq(P.running()[1], "teste", "tarefa listada")
release()
release() -- idempotente
H.ok(vim.wait(2000, function() return icon_wins() == 0 end, 20), "ícone some quando acaba")
H.eq(#P.running(), 0, "nada em andamento")

-- trabalho curto não pisca: libera antes de SHOW_AFTER
P.SHOW_AFTER = 500
P.track "curto"()
vim.wait(700, function() return false end, 50)
H.eq(icon_wins(), 0, "tarefa curta não mostra ícone")
P.SHOW_AFTER = 50

-- git: repositório real, sem escrever nele
local repo = H.workdir { ["a.txt"] = "x" }
local function sh(...) vim.system({ "git", "-C", repo, ... }, { text = true }):wait() end
sh("init", "-q", "-b", "principal")
sh("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "i")
local before = vim.fn.glob(repo .. "/.git/*", true, true)
P.project(repo)
H.ok(vim.wait(5000, function() return P.state.git and P.state.git.head ~= nil and #P.running() == 0 end, 20), "git resolvido")
H.eq(P.state.git.branch, "principal", "ramo")
H.eq(P.state.git.dirty, 1, "a.txt não rastreado conta como alteração")
H.ok(P.state.git.refs >= 1, "refs lidas")
H.eq(#vim.fn.glob(repo .. "/.git/*", true, true), #before, "git do usuário não ganhou arquivos")
H.ok(vim.tbl_contains(P.state.done and vim.tbl_keys(P.state.done) or {}, vim.fs.normalize(repo)), "pasta marcada como preparada")

-- repositório recém-criado, sem commits
local novo = H.workdir {}
vim.system({ "git", "-C", novo, "init", "-q", "-b", "principal" }):wait()
P.project(novo)
H.ok(vim.wait(5000, function() return P.state.git and P.state.git.branch ~= nil and #P.running() == 0 end, 20), "git novo resolvido")
H.eq(P.state.git.branch, "principal", "ramo sem commits")

-- fora de repositório: sem git, sem erro
local plain = H.workdir {}
P.project(plain)
H.ok(vim.wait(5000, function() return #P.running() == 0 end, 20), "pasta simples termina")
H.eq(P.state.git, nil, "fora de repo")

-- home: não varre nem roda git
vim.env.HOME = plain
P.project(plain, true)
H.eq(#P.state.langs, 0, "home sem detecção")
H.eq(P.state.git, nil, "home sem git")
H.ok(P.report():find("Preparo em segundo plano", 1, true) ~= nil, "relatório")
H.done()
