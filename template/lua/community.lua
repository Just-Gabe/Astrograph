-- AstroCommunity: import any community modules here
-- We import this file in `lazy_setup.lua` before the `plugins/` folder.
-- This guarantees that the specs are processed before any user plugins.

---@type LazySpec
return {
  "AstroNvim/astrocommunity",
  { import = "astrocommunity.pack.lua" },
  -- Agentes: configurados em `lua/plugins/` (CodeCompanion + terminais + avante).
  -- O prefixo de IA é `<Leader>A`; o antigo `astrocommunity.ai.claudecode-nvim`
  -- foi substituído pelo CodeCompanion (evita colisão de keymaps).
  -- import/override with your plugins folder
}
