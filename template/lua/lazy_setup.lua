-- Astrograph: usa o core LOCAL do monorepo quando existir (`../core` ou
-- $ASTROGRAPH_CORE), permitindo testar mudanças no core sem publicar nada.
-- Caso contrário, cai para o AstroNvim oficial do GitHub.
local function astrograph_core()
  local uv = vim.uv or vim.loop
  local this = debug.getinfo(1, "S").source:sub(2)
  -- resolve symlinks/junctions: o config pode ser uma junction para o template,
  -- então o caminho real é `Astrograph/template/...` → irmão `../core`.
  local resolved = uv.fs_realpath(this) or this
  local root = vim.fn.fnamemodify(resolved, ":h:h:h")
  -- EVITAR nil no início da lista: ipairs pararia antes de começar
  local candidates = {}
  local env = vim.env.ASTROGRAPH_CORE
  if env and env ~= "" then table.insert(candidates, env) end
  table.insert(candidates, root .. "/core")
  table.insert(candidates, vim.fn.expand "~/Astrograph/core")

  for _, dir in ipairs(candidates) do
    if dir and dir ~= "" and uv.fs_stat(dir .. "/lua/astronvim/init.lua") then
      return { dir = dir, name = "AstroNvim" } --[[@as LazySpec]]
    end
  end
  return { "AstroNvim/AstroNvim", version = "^6" } --[[@as LazySpec]]
end

-- Astrograph: põe o tema LOCAL do monorepo (`../theme` ou $ASTROGRAPH_THEME) no
-- runtimepath quando existir, habilitando `:colorscheme astrograph` / `astrograph-light`.
local function astrograph_theme()
  local uv = vim.uv or vim.loop
  local this = debug.getinfo(1, "S").source:sub(2)
  local resolved = uv.fs_realpath(this) or this
  local root = vim.fn.fnamemodify(resolved, ":h:h:h")
  local candidates = {}
  local env = vim.env.ASTROGRAPH_THEME
  if env and env ~= "" then table.insert(candidates, env) end
  table.insert(candidates, root .. "/theme")
  table.insert(candidates, vim.fn.expand "~/Astrograph/theme")

  for _, dir in ipairs(candidates) do
    if dir and dir ~= "" and uv.fs_stat(dir .. "/colors/astrograph.lua") then return dir end
  end
end

local theme = astrograph_theme()
vim.g.astrograph_theme = theme -- exposto p/ o astroui escolher colorscheme (fallback)

local astro = astrograph_core()
astro.import = "astronvim.plugins"
astro.opts = { -- AstroNvim options must be set here with the `import` key
  mapleader = " ", -- This ensures the leader key must be configured before Lazy is set up
  maplocalleader = ",", -- This ensures the localleader key must be configured before Lazy is set up
  icons_enabled = true, -- Set to false to disable icons (if no Nerd Font is available)
  pin_plugins = nil, -- Default will pin plugins when tracking `version` of AstroNvim, set to true/false to override
  update_notifications = true, -- Enable/disable notification about running `:Lazy update` twice to update pinned plugins
}

local specs = {
  astro,
  { import = "community" },
  { import = "plugins" },
}

-- Tema local primeiro (prioridade alta) para o rtp/`colors/` existir antes do
-- astroui aplicar `colorscheme = "astrograph"`.
if theme then table.insert(specs, 1, { dir = theme, name = "astrograph-theme", priority = 1000 }) end

-- Plugins locais desta máquina, fora do git (integrações internas, etc.):
-- $ASTROGRAPH_LOCAL ou <pasta de dados>/astrograph-local (no Windows,
-- %LOCALAPPDATA%\astrograph-local). Módulos em `lua/`, specs do lazy em `specs/*.lua`.
local function astrograph_local()
  local uv = vim.uv or vim.loop
  local dir = vim.env.ASTROGRAPH_LOCAL
  if not dir or dir == "" then dir = vim.fs.joinpath(vim.fs.dirname(vim.fn.stdpath "data"), "astrograph-local") end
  if not uv.fs_stat(dir .. "/specs") then return end
  table.insert(specs, { dir = dir, name = "astrograph-local", lazy = false })
  -- specs (ex.: importados de uma config antiga) podem dar `require` nos
  -- módulos de `lua/` já na leitura, antes do lazy pôr a pasta no runtimepath
  package.path = ("%s/lua/?.lua;%s/lua/?/init.lua;"):format(dir, dir) .. package.path
  for name, kind in vim.fs.dir(dir .. "/specs") do
    if kind == "file" and name:match "%.lua$" then
      local ok, spec = pcall(dofile, dir .. "/specs/" .. name)
      if ok and type(spec) == "table" then
        table.insert(specs, spec)
      elseif not ok then
        vim.schedule(function() vim.notify(("Plugin local %s: %s"):format(name, spec), vim.log.levels.ERROR) end)
      end
    end
  end
end
astrograph_local()

require("lazy").setup(specs --[[@as LazySpec]], {
  -- Configure any other `lazy.nvim` configuration options here
  install = { colorscheme = { "astrotheme", "habamax" } },
  ui = { backdrop = 100 },
  performance = {
    rtp = {
      -- disable some rtp plugins, add more to your liking
      disabled_plugins = {
        "gzip",
        "netrwPlugin",
        "rplugin", -- remote plugins (python/node): nenhum plugin do Astrograph usa
        "tarPlugin",
        "tohtml",
        "tutor",
        "zipPlugin",
      },
    },
  },
} --[[@as LazyConfig]])
