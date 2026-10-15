-- Conflitos no modo agêntico (#14, #15, #16): só o trabalho do agente vai
-- sozinho para ele; sensível, teto, não resolvido e sessão encerrada vão para o
-- usuário; lote num pedido só; texto não salvo por arquivo temporário; :w! não
-- grava por cima durante a junção.
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local ag = H.agent()
local st = ag._state
st.job, st.ready = -1, true -- sessão aberta falsa
local dir = H.workdir { ["a.lua"] = "a1\na2", ["b.lua"] = "b1\nb2", [".env"] = "K=1", ["c.lua"] = "c1", ["d.lua"] = "d1", ["e.lua"] = "e1" }

local function read_by_agent(name)
  H.event(ag, { ev = "tool", id = "r-" .. name, title = "Read", kind = "read", status = "completed", locations = { { path = dir .. "/" .. name } } })
end
---Abre, altera sem salvar e muda o disco por fora: devolve o buffer.
local function make_conflict(name)
  vim.cmd.edit(dir .. "/" .. name)
  local b = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(b, 0, 1, false, { "MEU " .. name })
  H.disk_write(dir .. "/" .. name, { "DE FORA " .. name })
  return b
end
local function flushed() vim.wait(2000, function() return #st.batch == 0 and st.batch_timer == nil end, 50) end
local function check_all()
  vim.cmd "checktime"
  vim.wait(100)
end

-- lote: dois arquivos do agente mudam juntos → um pedido só, com os dois
read_by_agent "a.lua"
read_by_agent "b.lua"
make_conflict "a.lua"
make_conflict "b.lua"
check_all()
vim.cmd "bufdo checktime"
vim.wait(100)
flushed()
H.ok(st.turn_conflicts[H.key(dir .. "/a.lua")] and st.turn_conflicts[H.key(dir .. "/b.lua")], "os dois no mesmo pedido")
local panel = table.concat(vim.api.nvim_buf_get_lines(st.buf, 0, -1, false), "\n")
H.ok(panel:find("a.lua, ", 1, true) or panel:find("b.lua, ", 1, true), "um rótulo com os dois arquivos")

-- texto não salvo vai por temporário (com o conteúdo do editor), não no prompt
local c = st.conflicts[H.key(dir .. "/a.lua")]
H.ok(c and c.tmp and vim.uv.fs_stat(c.tmp), "temporário criado")
H.eq(vim.fn.readfile(c.tmp)[1], "MEU a.lua", "temporário com a versão do editor")

-- :w! durante a junção não grava por cima
vim.cmd.buffer(vim.fn.bufnr(dir .. "/a.lua"))
local ok, err = pcall(vim.cmd, "write!")
H.ok(not ok and tostring(err):find("juntando", 1, true), ":w! bloqueado durante a junção")
H.eq(H.disk(dir .. "/a.lua"), "DE FORA a.lua", "disco intacto")

-- agente termina sem gravar a.lua: sai do automático; temporário apagado
local tmp_a = c.tmp
H.disk_write(dir .. "/b.lua", { "MEU b.lua", "DE FORA b.lua" })
H.event(ag, { ev = "edited", path = dir .. "/b.lua" })
H.event(ag, { ev = "done", stop = "end_turn" })
H.ok(st.unresolved[H.key(dir .. "/a.lua")], "a.lua marcado como não resolvido")
H.ok(not vim.uv.fs_stat(tmp_a), "temporário apagado")
H.eq(next(st.conflicts), nil, "sem conflito pendente")

-- não resolvido antes: o próximo conflito do mesmo arquivo vai para o menu
local menus = #H.menus
H.disk_write(dir .. "/a.lua", { "DE FORA de novo" })
vim.cmd.buffer(vim.fn.bufnr(dir .. "/a.lua"))
check_all()
H.eq(#H.menus, menus + 1, "não resolvido → menu")
H.ok(H.menus[#H.menus]:find("não resolveu", 1, true), "menu diz o motivo")

-- arquivo que o agente nunca tocou → menu (com a opção de pedir ao agente)
menus = #H.menus
make_conflict "c.lua"
check_all()
H.eq(#H.menus, menus + 1, "fora do trabalho do agente → menu")

-- sensível, mesmo tocado pelo agente → menu
read_by_agent ".env"
menus = #H.menus
make_conflict ".env"
check_all()
H.eq(#H.menus, menus + 1, ".env → menu")
H.ok(H.menus[#H.menus]:find("sensível", 1, true), "menu diz que é sensível")

-- teto: depois de 3 automáticos na janela, vai para o menu
st.auto_times = { os.time(), os.time(), os.time() }
read_by_agent "d.lua"
menus = #H.menus
make_conflict "d.lua"
check_all()
H.eq(#H.menus, menus + 1, "teto → menu")
H.ok(H.menus[#H.menus]:find("teto", 1, true), "menu diz que é o teto")

-- escolher "pedir ao agente" no menu manda mesmo assim (decisão do usuário)
H.pick = "agent"
read_by_agent "e.lua"
st.auto_times = { os.time(), os.time(), os.time() }
make_conflict "e.lua"
check_all()
flushed()
H.ok(st.conflicts[H.key(dir .. "/e.lua")], "pedido ao agente pelo menu")
H.pick = nil

-- sessão encerrada com conflito pendente: volta para o usuário (menu)
menus = #H.menus
ag.stop()
H.eq(#H.menus, menus + 1, "conflito pendente volta para o menu no stop")
H.eq(next(st.conflicts), nil, "estado limpo")
H.done()
