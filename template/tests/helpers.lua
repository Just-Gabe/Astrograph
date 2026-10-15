-- Apoio dos testes do agente nativo. Cada teste roda num Neovim isolado
-- (`nvim --clean --headless -l <teste>`), disparado por tests/run.lua.
local H = {}

local this = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p")
H.template = vim.fs.dirname(vim.fs.dirname(vim.fs.normalize(this)))
vim.opt.rtp:prepend(H.template)

H.notes, H.menus = {}, {}
vim.notify = function(m) H.notes[#H.notes + 1] = m end
--- menu falso: registra; `H.pick` escolhe a opção (id/kind) se definido
vim.ui.select = function(items, opts, cb)
  H.menus[#H.menus + 1] = opts.prompt
  if H.pick then
    for _, it in ipairs(items) do
      if it.id == H.pick or it.optionId == H.pick or (it.kind and it.kind:match(H.pick)) then return cb(it) end
    end
  end
end

function H.agent()
  local ag = require "astrograph.agent"
  ag.setup()
  return ag
end

---Pasta temporária com arquivos { nome = "conteúdo" }; vira o cwd.
function H.workdir(files)
  local dir = vim.fs.normalize(vim.fn.tempname())
  vim.fn.mkdir(dir, "p")
  for name, text in pairs(files or {}) do
    vim.fn.writefile(vim.split(text, "\n", { plain = true }), dir .. "/" .. name)
  end
  vim.cmd.cd(dir)
  return dir
end

local bump = 0
---Grava no disco "por fora" com mtime à frente (o nvim detecta mudança pelo mtime).
function H.disk_write(path, lines)
  bump = bump + 10
  vim.fn.writefile(lines, path)
  vim.uv.fs_utime(path, os.time() + bump, os.time() + bump)
end

function H.lines(buf) return table.concat(vim.api.nvim_buf_get_lines(buf or 0, 0, -1, false), "|") end
function H.disk(path) return table.concat(vim.fn.readfile(path), "|") end

function H.eq(got, want, what)
  if got ~= want then error(("%s: esperado %s, veio %s"):format(what, vim.inspect(want), vim.inspect(got)), 2) end
end

function H.ok(cond, what)
  if not cond then error(what, 2) end
end

---Fim do teste. Obrigatório: uma pergunta do nvim sem resposta (ex.: o "(y/n)"
---do :w) faz o nvim sair com código 0 no meio do teste; sem o marcador, o
---run.lua conta como falha.
function H.done() io.stdout:write "TESTE_OK\n" end

---Evento do astrograph-agent, como se viesse do processo.
function H.event(ag, ev) ag._on_line(vim.json.encode(ev)) end

---Chave interna de arquivo usada pelo módulo (realpath normalizado).
function H.key(path) return vim.fs.normalize(vim.uv.fs_realpath(path) or path):lower() end

return H
