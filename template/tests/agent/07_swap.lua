-- Swap existente (E325): o seguro resolve sozinho; texto de sessão que caiu vai
-- para o agente no modo agêntico; outro nvim aberto segue a regra do nvim (W325).
local H = dofile(vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(debug.getinfo(1, "S").source:sub(2)))) .. "/helpers.lua")
local holder_script = vim.fn.tempname() .. ".lua"
vim.fn.writefile({
  "vim.cmd.edit(vim.env.HF)",
  'if vim.env.DIRTY == "1" then vim.api.nvim_buf_set_lines(0, 0, 1, false, { "TEXTO NAO SALVO" }); vim.cmd "preserve" end',
  "vim.fn.writefile({ vim.fn.swapname '%' }, vim.env.HOUT)",
  "vim.wait(600000, function() return false end)",
}, holder_script)

---Sobe um nvim que deixa swap de `file`; devolve o processo e o caminho do swap.
local function holder(file, dirty)
  local out = vim.fn.tempname()
  local p = vim.system({ vim.v.progpath, "--clean", "--headless", "-c", "luafile " .. holder_script },
    { env = { HF = file, DIRTY = dirty and "1" or "0", HOUT = out } })
  vim.wait(10000, function() return vim.uv.fs_stat(out) ~= nil and #vim.fn.readfile(out) > 0 end, 50)
  return p, vim.fn.readfile(out)[1]
end

-- `nvim -l` liga como `-n` (updatecount=0): sem isso o swap não é checado ao abrir
vim.o.updatecount = 200
local ag = H.agent()
local function open(file)
  local ok, err = pcall(vim.cmd.edit, file)
  vim.wait(100)
  return ok, err
end

-- sessão que caiu sem nada pendente: apaga o swap e abre limpo
local d1 = H.workdir { ["c.txt"] = "c1\nc2" }
local p, swap = holder(d1 .. "/c.txt", false)
p:kill(9); p:wait()
H.ok(open(d1 .. "/c.txt"), "abriu sem pergunta (swap limpo)")
H.eq(vim.bo.modified, false, "buffer limpo")
-- o swap do nvim que caiu sumiu (o nome pode ser reusado pelo swap desta sessão)
H.ok(vim.fn.swapinfo(swap).pid ~= p.pid, "swap da sessão que caiu apagado")
vim.cmd "bwipeout!"

-- outro nvim aberto com texto: não recupera nem manda ao agente (regra do nvim)
ag._state.job, ag._state.ready = -1, true
local d2 = H.workdir { ["c.txt"] = "c1\nc2" }
p, swap = holder(d2 .. "/c.txt", true)
H.ok(open(d2 .. "/c.txt"), "abriu sem pergunta (nvim vivo)")
H.eq(H.lines(), "c1|c2", "não recuperou texto de sessão viva")
H.eq(#ag._state.queue + (ag._state.turn_conflict and 1 or 0), 0, "nada para o agente")
p:kill(9); p:wait()
vim.cmd "bwipeout!"
vim.fn.delete(swap)

-- sessão que caiu com texto, modo agêntico: recupera e manda ao agente
local d3 = H.workdir { ["c.txt"] = "c1\nc2" }
p, swap = holder(d3 .. "/c.txt", true)
p:kill(9); p:wait()
H.ok(open(d3 .. "/c.txt"), "abriu sem pergunta (recuperação)")
H.eq(H.lines(), "TEXTO NAO SALVO|c2", "texto recuperado")
H.ok(ag._state.turn_conflict or #ag._state.queue > 0, "conflito enviado ao agente")
ag._state.job = nil
H.done()
