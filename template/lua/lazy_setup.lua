-- Astrograph: usa o core LOCAL do monorepo quando existir (`../core` ou
-- $ASTROGRAPH_CORE), permitindo testar mudanças no core sem publicar nada.
-- Caso contrário, cai para o AstroNvim oficial do GitHub.
local function astrograph_core()
  local this = debug.getinfo(1, "S").source:sub(2)
  local root = vim.fn.fnamemodify(this, ":h:h:h")
  -- EVITAR nil no início da lista: ipairs pararia antes de começar
  local candidates = {}
  local env = vim.env.ASTROGRAPH_CORE
  if env and env ~= "" then table.insert(candidates, env) end
  table.insert(candidates, root .. "/core")
  table.insert(candidates, vim.fn.expand "~/Astrograph/core")

  local uv = vim.uv or vim.loop
  for _, dir in ipairs(candidates) do
    if dir and dir ~= "" and uv.fs_stat(dir .. "/lua/astronvim/init.lua") then
      return { dir = dir, name = "AstroNvim" } --[[@as LazySpec]]
    end
  end
  return { "AstroNvim/AstroNvim", version = "^6" } --[[@as LazySpec]]
end

local astro = astrograph_core()
astro.import = "astronvim.plugins"
astro.opts = { -- AstroNvim options must be set here with the `import` key
  mapleader = " ", -- This ensures the leader key must be configured before Lazy is set up
  maplocalleader = ",", -- This ensures the localleader key must be configured before Lazy is set up
  icons_enabled = true, -- Set to false to disable icons (if no Nerd Font is available)
  pin_plugins = nil, -- Default will pin plugins when tracking `version` of AstroNvim, set to true/false to override
  update_notifications = true, -- Enable/disable notification about running `:Lazy update` twice to update pinned plugins
}

require("lazy").setup({
  astro,
  { import = "community" },
  { import = "plugins" },
} --[[@as LazySpec]], {
  -- Configure any other `lazy.nvim` configuration options here
  install = { colorscheme = { "astrotheme", "habamax" } },
  ui = { backdrop = 100 },
  performance = {
    rtp = {
      -- disable some rtp plugins, add more to your liking
      disabled_plugins = {
        "gzip",
        "netrwPlugin",
        "tarPlugin",
        "tohtml",
        "zipPlugin",
      },
    },
  },
} --[[@as LazyConfig]])
