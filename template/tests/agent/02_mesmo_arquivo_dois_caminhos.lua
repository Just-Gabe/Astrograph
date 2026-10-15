-- O mesmo arquivo chega por caminhos diferentes (8.3 × longo no Windows,
-- junction/symlink): o editor tem de reconhecer o buffer aberto.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local dir = H.workdir { ["a.txt"] = "um\ndois" }
local alias = dir .. "-alias"
if vim.fn.has "win32" == 1 then
  vim.fn.system { "cmd", "/c", "mklink", "/J", (alias:gsub("/", "\\")), (dir:gsub("/", "\\")) }
else
  vim.uv.fs_symlink(dir, alias)
end
H.ok(vim.uv.fs_stat(alias .. "/a.txt"), "alias criado")
vim.cmd.edit(dir .. "/a.txt")
local b = vim.api.nvim_get_current_buf()
local ag = H.agent()
-- sem seguir: o nvim acharia o buffer pelo id do arquivo e esconderia a falha
ag.follow = false
H.event(ag, { ev = "tool", id = "e1", title = "Edit", kind = "edit", status = "pending", locations = { { path = alias .. "/a.txt" } } })
H.disk_write(dir .. "/a.txt", { "um", "DOIS" })
H.event(ag, { ev = "edited", path = alias .. "/a.txt" })
H.eq(H.lines(b), "um|DOIS", "buffer recarregado pelo outro caminho")
local marks = vim.api.nvim_buf_get_extmarks(b, vim.api.nvim_create_namespace "astrograph_agent_changes", 0, -1, {})
H.eq(#marks, 1, "linha alterada destacada")
local files = vim.tbl_filter(function(x) return vim.api.nvim_buf_get_name(x) ~= "" and vim.bo[x].buftype == "" end, vim.api.nvim_list_bufs())
H.eq(#files, 1, "sem buffer duplicado")
H.done()
