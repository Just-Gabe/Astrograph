-- Durante a junção de um conflito, o que o usuário digita depois do envio não
-- pode ser descartado quando a versão do agente chega (#13).
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local ag = H.agent()
ag._state.job, ag._state.ready = -1, true -- sessão aberta falsa (modo agêntico)

---Conflito enviado ao agente (alteração sua × disco) e a junção dele no disco.
local function scenario(name)
  local dir = H.workdir { [name] = "um\ndois" }
  local f = dir .. "/" .. name
  vim.cmd.edit(f)
  local b = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_set_lines(b, 0, 1, false, { "UM DO USUARIO" })
  H.disk_write(f, { "um", "dois", "linha do agente" })
  vim.cmd "checktime"
  vim.wait(100)
  H.ok(ag._state.turn_conflict, "conflito enviado ao agente")
  return f, b
end

-- caminho feliz: nada digitado depois → carrega a junção do agente
local f, b = scenario "a.txt"
H.disk_write(f, { "UM DO USUARIO", "dois", "linha do agente" })
H.event(ag, { ev = "edited", path = f })
H.eq(H.lines(b), "UM DO USUARIO|dois|linha do agente", "junção carregada")
H.eq(vim.bo[b].modified, false, "buffer limpo depois da junção")
H.event(ag, { ev = "done", stop = "end_turn" })

-- digitou depois do envio → não carrega por cima; o conflito reabre
f, b = scenario "b.txt"
vim.api.nvim_buf_set_lines(b, -1, -1, false, { "DIGITADO DEPOIS DO ENVIO" })
H.disk_write(f, { "UM DO USUARIO", "dois", "linha do agente" })
H.event(ag, { ev = "edited", path = f })
H.ok(H.lines(b):find("DIGITADO DEPOIS DO ENVIO", 1, true), "digitação depois do envio preservada: " .. H.lines(b))
H.eq(vim.bo[b].modified, true, "buffer continua com as alterações do usuário")
H.eq(#ag._state.queue, 1, "conflito reaberto na fila")
H.event(ag, { ev = "done", stop = "end_turn" })
H.eq(ag._state.turn_conflict, H.key(f), "conflito reaberto é o turno seguinte")
ag._state.job = nil
H.done()
