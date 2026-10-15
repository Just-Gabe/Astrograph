-- Arquivo mudou no disco com alteração sua: menu (não o aviso que trava),
-- uma vez por mudança.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local dir = H.workdir { ["c.txt"] = "c1\nc2" }
H.agent()
vim.cmd.edit(dir .. "/c.txt")
vim.api.nvim_buf_set_lines(0, 0, 1, false, { "MEU" })
H.disk_write(dir .. "/c.txt", { "c1", "OUTRO" })
vim.cmd "checktime"
vim.wait(50)
vim.cmd "checktime"
vim.wait(50)
H.eq(#H.menus, 1, "um menu só")
H.eq(H.lines(), "MEU|c2", "buffer do usuário mantido")

-- sem alteração sua: recarrega sem perguntar
vim.cmd "edit!"
H.disk_write(dir .. "/c.txt", { "c1", "NOVO" })
vim.cmd "checktime"
H.eq(H.lines(), "c1|NOVO", "recarregou")
H.eq(#H.menus, 1, "sem menu novo")
H.done()
