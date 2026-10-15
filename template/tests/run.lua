-- Roda os testes do agente nativo, cada um num Neovim isolado e com tempo-limite
-- (uma pergunta do nvim que trave a tela vira falha, não um teste parado).
--
--   nvim --clean --headless -l template/tests/run.lua [filtro]
local dir = vim.fs.dirname(vim.fs.normalize(vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p")))
local filter = _G.arg and _G.arg[1] or nil
local nvim = vim.v.progpath
local failed, total = 0, 0

local files = {}
for name, kind in vim.fs.dir(dir .. "/agent") do
  if kind == "file" and name:match "%.lua$" and (not filter or name:find(filter, 1, true)) then files[#files + 1] = name end
end
table.sort(files)

for _, name in ipairs(files) do
  total = total + 1
  local t0 = vim.uv.hrtime()
  local res = vim.system({ nvim, "--clean", "--headless", "-l", dir .. "/agent/" .. name }, { text = true }):wait(30000)
  local ms = (vim.uv.hrtime() - t0) / 1e6
  -- só passa com o marcador do fim (H.done): sair com 0 no meio não conta
  local finished = (res.stdout or ""):find("TESTE_OK", 1, true) ~= nil
  local ok = res.code == 0 and res.signal == 0 and finished
  if not ok then failed = failed + 1 end
  io.stdout:write(("%s %-40s %5.0f ms\n"):format(ok and "ok  " or "FALHA", name, ms))
  if not ok then
    local why = (res.stderr or "") .. (res.stdout or "")
    if res.code == 124 or res.signal ~= 0 then why = "tempo esgotado (travou numa pergunta?)\n" .. why end
    if res.code == 0 and not finished then why = "saiu antes do fim (pergunta do nvim sem resposta?)\n" .. why end
    io.stdout:write("      " .. vim.trim(why):gsub("\n", "\n      ") .. "\n")
  end
end

io.stdout:write(("\n%d testes, %d falha(s)\n"):format(total, failed))
os.exit(failed == 0 and 0 or 1)
