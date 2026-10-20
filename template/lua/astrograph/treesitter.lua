-- Parsers do Treesitter: compilar e avisar.
--
-- Compilar um parser (baixa a gramática e roda o gcc) leva alguns segundos. Sem
-- aviso, isso acontecia na primeira vez que um arquivo daquela linguagem era
-- aberto. Aqui o Astrograph:
--
--   * `preinstall()` — compila os parsers de `ensure_installed` (o instalador
--     chama em modo headless, para o primeiro uso não compilar nada);
--   * `setup()` — avisa quando um parser ainda precisa ser compilado durante o
--     uso (uma linguagem fora da lista, ou depois de uma atualização).

local M = {}

local wrapped = false

---Enquanto > 0, a instalação de parsers não avisa (o preparo em segundo plano
---compila sem poluir a tela).
M.quiet = 0

local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Astrograph" }) end

---Linguagens de `ensure_installed` (já mescladas pelo AstroCore), sem repetição.
---@return string[]
function M.ensure_installed()
  local list = vim.tbl_get(require("astrocore").config, "treesitter", "ensure_installed") or {}
  local seen, out = {}, {}
  for _, lang in ipairs(list) do
    if lang ~= "auto" and not seen[lang] then
      seen[lang] = true
      out[#out + 1] = lang
    end
  end
  return out
end

---@return table<string, boolean> # linguagens já instaladas
local function installed()
  local ok, ts = pcall(require, "nvim-treesitter")
  local set = {}
  if ok then
    for _, lang in ipairs(ts.get_installed "parsers") do
      set[lang] = true
    end
  end
  return set
end

---Garante `tree-sitter` (CLI do Mason) e o gcc no PATH (necessário no headless,
---quando o Mason ainda não prependeu o `bin`).
local function prepare_toolchain()
  if vim.fn.exepath "tree-sitter" == "" then
    local mason_bin = vim.fn.stdpath "data" .. "/mason/bin"
    if vim.fn.isdirectory(mason_bin) == 1 then
      vim.env.PATH = mason_bin .. (vim.fn.has "win32" == 1 and ";" or ":") .. (vim.env.PATH or "")
    end
  end
  if vim.fn.has "win32" == 1 and (not vim.env.CC or vim.env.CC == "") then vim.env.CC = "gcc" end
end

---Das linguagens pedidas, as que o nvim-treesitter conhece e ainda não estão
---instaladas.
---@param langs string[]
---@return string[]
function M.missing(langs)
  local have = installed()
  local known
  local ok, ts = pcall(require, "nvim-treesitter")
  if ok and type(ts.get_available) == "function" then
    local aok, list = pcall(ts.get_available)
    if aok and type(list) == "table" then
      known = {}
      for _, l in ipairs(list) do
        known[l] = true
      end
    end
  end
  local seen, out = {}, {}
  for _, lang in ipairs(langs) do
    if lang ~= "auto" and not seen[lang] and not have[lang] and (not known or known[lang]) then
      seen[lang] = true
      out[#out + 1] = lang
    end
  end
  return out
end

---Compila em segundo plano, sem avisos, os parsers que faltarem. `cb` roda
---(no loop principal) quando acaba, ou na hora se não houver o que fazer.
---@param langs string[]
---@param cb? fun(missing: string[])
function M.install_quiet(langs, cb)
  cb = cb or function() end
  prepare_toolchain()
  local plugin = vim.tbl_get(require "lazy.core.config", "plugins", "nvim-treesitter")
  if not plugin or vim.fn.executable "tree-sitter" ~= 1 then return cb {} end
  if not plugin._.loaded then require("lazy").load { plugins = { "nvim-treesitter" } } end
  local missing = M.missing(langs)
  if #missing == 0 then return cb {} end
  M.quiet = M.quiet + 1
  local ok, task = pcall(function() return require("nvim-treesitter").install(missing, { summary = false }) end)
  if not ok or not task then
    M.quiet = M.quiet - 1
    return cb {}
  end
  task:await(function()
    M.quiet = M.quiet - 1
    vim.schedule(function()
      M.refresh(missing)
      cb(missing)
    end)
  end)
end

---Depois de instalar parsers por fora do AstroCore: atualiza o cache dele e liga
---realce/indent nos buffers abertos dessas linguagens (sem isso só valeria depois
---de reabrir o arquivo).
---@param langs string[]
function M.refresh(langs)
  local ok, act = pcall(require, "astrocore.treesitter")
  if not ok then return end
  pcall(act.installed, true)
  local want = {}
  for _, l in ipairs(langs) do
    want[l] = true
  end
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buf) then
      local lang = vim.treesitter.language.get_lang(vim.bo[buf].filetype)
      if lang and want[lang] then pcall(act.enable, buf) end
    end
  end
end

---Compila de forma síncrona os parsers que faltarem (usado pelo instalador).
---@param langs? string[] linguagens (padrão: `ensure_installed`)
---@return string[] # as que foram compiladas
function M.preinstall(langs)
  prepare_toolchain()
  if vim.fn.executable "tree-sitter" ~= 1 then
    notify("`tree-sitter` não encontrado; pulando a pré-compilação dos parsers.", vim.log.levels.WARN)
    return {}
  end

  local plugin = vim.tbl_get(require "lazy.core.config", "plugins", "nvim-treesitter")
  if not plugin then
    notify("nvim-treesitter não instalado; pulando a pré-compilação dos parsers.", vim.log.levels.WARN)
    return {}
  end
  require("lazy").load { plugins = { "nvim-treesitter" } }
  vim.wait(60000, function() return plugin._.loaded == true end)

  local have = installed()
  local missing = vim.tbl_filter(function(lang) return not have[lang] end, langs or M.ensure_installed())
  if #missing == 0 then return {} end

  require("nvim-treesitter").install(missing, { summary = true }):pwait(600000)
  return missing
end

---Avisa quando um parser é compilado durante o uso (deixa claro que é só esperar).
function M.setup()
  require("astrocore").on_load("nvim-treesitter", function()
    if wrapped then return end
    wrapped = true

    local install_mod = require "nvim-treesitter.install"
    local original = install_mod.install
    install_mod.install = function(langs, opts)
      local have = installed()
      local todo = vim.tbl_filter(
        function(lang) return not have[lang] end,
        type(langs) == "table" and langs or { langs }
      )
      local task = original(langs, opts)
      if #todo > 0 and M.quiet == 0 then
        local start = vim.uv.hrtime()
        notify("Preparando o realce de sintaxe (" .. table.concat(todo, ", ") .. ")… aguarde um instante.")
        task:await(function()
          local seconds = (vim.uv.hrtime() - start) / 1e9
          notify(("Realce de sintaxe pronto (%s, %.1fs)."):format(table.concat(todo, ", "), seconds))
        end)
      end
      return task
    end
  end)
end

return M
