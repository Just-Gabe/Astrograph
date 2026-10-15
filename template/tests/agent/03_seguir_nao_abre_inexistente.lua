-- Seguir o agente não abre arquivo que ele ainda vai criar (o buffer vazio
-- virava o aviso W13 "created after editing started").
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local dir = H.workdir { ["b.txt"] = "b" }
vim.cmd.edit(dir .. "/b.txt")
local ag = H.agent()
H.event(ag, { ev = "tool", id = "r1", title = "Read", kind = "read", status = "pending", locations = { { path = dir .. "/novo.md" } } })
for _, b in ipairs(vim.api.nvim_list_bufs()) do
  H.ok(not vim.api.nvim_buf_get_name(b):match "novo%.md$", "não abriu arquivo inexistente")
end
H.done()
