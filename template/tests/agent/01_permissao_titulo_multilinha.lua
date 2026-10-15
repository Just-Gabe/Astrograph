-- Título com quebra de linha (comando de terminal) travava a permissão: o
-- agente ficava esperando uma resposta que o editor nunca perguntava.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local ag = H.agent()
local t = 'grep "a|b"\n  C:/x/coletor.rs\n'
H.event(ag, { ev = "tool", id = "t1", title = t, kind = "execute", status = "pending" })
H.event(ag, { ev = "permission", id = 5, tool = "t1", title = t, kind = "execute", diffs = {}, options = { { optionId = "allow-once", name = "Yes", kind = "allow_once" } } })
H.eq(#H.menus, 1, "menu de permissão aparece")
H.ok(not H.menus[1]:find "\n", "título do menu numa linha só")
H.event(ag, { ev = "tool", id = "t1", title = t, kind = "execute", status = "completed" })
local panel = vim.api.nvim_buf_get_lines(ag._state.buf, 0, -1, false)
local tool_lines = vim.tbl_filter(function(l) return l:find("grep", 1, true) end, panel)
H.eq(#tool_lines, 1, "uma linha por ferramenta no painel")
H.ok(tool_lines[1]:find("coletor.rs", 1, true), "título inteiro na mesma linha: " .. tool_lines[1])
for _, l in ipairs(panel) do H.ok(not l:find("Erro interno", 1, true), "sem erro interno no painel: " .. l) end
H.done()
