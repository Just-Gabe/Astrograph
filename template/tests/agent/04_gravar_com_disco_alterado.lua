-- :w com o disco alterado desde a leitura: nunca o "(y/n)" do nvim.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local dir = H.workdir { ["c.txt"] = "c1\nc2" }
local f = dir .. "/c.txt"
H.agent()

-- com alteração sua: gravação suspensa, disco preservado, menu
vim.cmd.edit(f)
vim.api.nvim_buf_set_lines(0, 0, 1, false, { "MEU" })
H.disk_write(f, { "c1", "OUTRO" })
local ok, err = pcall(vim.cmd, "write")
vim.wait(50)
H.ok(not ok and tostring(err):find("Astrograph", 1, true), "gravação suspensa")
H.eq(H.disk(f), "c1|OUTRO", "disco preservado")
H.eq(#H.menus, 1, "menu para o usuário (fora do modo agêntico)")

-- :w! grava por cima de propósito
vim.cmd "write!"
H.eq(H.disk(f), "MEU|c2", ":w! grava")

-- sem alteração sua: carrega a versão do disco
H.disk_write(f, { "c1", "DE FORA" })
ok = pcall(vim.cmd, "write")
vim.wait(50)
H.ok(not ok, "gravação sem alteração suspensa")
H.eq(H.lines(), "c1|DE FORA", "buffer com a versão do disco")
H.eq(vim.bo.modified, false, "buffer limpo")
H.done()
