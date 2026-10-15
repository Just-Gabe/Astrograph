-- Pré-aquecimento no ocioso: deixa a *primeira* vez de cada ação tão rápida
-- quanto as seguintes.
--
-- Depois do startup (e da animação da tela inicial), carrega em segundo plano,
-- um item por vez, o que costuma ser usado logo: os módulos da busca, o
-- explorador e os executáveis `fd`/`rg`/`git` (no Windows, a primeira execução
-- de cada um é várias vezes mais lenta: o sistema e o antivírus os examinam).
--
-- Nunca disputa com você: um item por tick, e só quando não há tecla na fila
-- nem modo de inserção/comando ativo. Desligue com `vim.g.astrograph_warmup = false`.

local M = {}

local function idle()
  local mode = vim.api.nvim_get_mode()
  if mode.blocking then return false end
  local m = mode.mode:sub(1, 1)
  if m == "i" or m == "c" or m == "R" then return false end
  return vim.fn.getchar(1) == 0
end

---Processo em segundo plano, sem esperar o resultado.
---@param cmd string[]
local function spawn(cmd)
  return function()
    if vim.fn.executable(cmd[1]) == 1 then pcall(vim.system, cmd, { text = true }) end
  end
end

---@param mods string[]
local function require_all(mods)
  return function()
    for _, mod in ipairs(mods) do
      pcall(require, mod)
    end
  end
end

---@param name string
local function load_plugin(name)
  return function()
    local plugin = vim.tbl_get(require("lazy.core.config").plugins, name)
    if plugin and not plugin._.loaded then pcall(require("lazy").load, { plugins = { name } }) end
  end
end

M.tasks = {
  spawn { "fd", "--version" },
  spawn { "rg", "--version" },
  spawn { "git", "--version" },
  require_all {
    "snacks.picker",
    "snacks.picker.core.picker",
    "snacks.picker.core.list",
    "snacks.picker.core.input",
    "snacks.picker.core.preview",
    "snacks.picker.core.matcher",
    "snacks.picker.source.files",
    "snacks.picker.source.grep",
    "snacks.picker.format",
    "snacks.layout",
  },
  load_plugin "neo-tree.nvim",
}

---Roda as tarefas uma por vez, só quando o editor está ocioso.
function M.start()
  if vim.g.astrograph_warmup == false then return end
  local i = 0
  local function step()
    i = i + 1
    local task = M.tasks[i]
    if not task then return end
    if not idle() then
      i = i - 1
      return vim.defer_fn(step, 250)
    end
    task()
    vim.defer_fn(step, 60)
  end
  step()
end

function M.setup()
  vim.api.nvim_create_autocmd("User", {
    pattern = "VeryLazy",
    once = true,
    group = vim.api.nvim_create_augroup("astrograph_warmup", { clear = true }),
    -- depois da animação de entrada e das checagens adiadas do core
    callback = function() vim.defer_fn(M.start, 1200) end,
  })
end

return M
