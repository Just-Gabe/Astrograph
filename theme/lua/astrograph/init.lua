--- Astrograph — colorscheme próprio do Astrograph (rebrand do AstroNvim).
--- Reaproveita o motor do `astrotheme`, injetando as paletas
--- `astrograph` (dark) e `astrograph-light`.
local M = {}

local PALETTES = { "astrograph", "astrograph-light" }

--- Registra as paletas no motor, que as resolve via `require("astrotheme.palettes.<nome>")`.
function M.register()
  for _, name in ipairs(PALETTES) do
    package.preload["astrotheme.palettes." .. name] = function()
      return require("astrograph.palettes." .. name)
    end
  end
end

--- Overrides que o motor não expressa só com os slots da paleta: `Operator` e
--- `Identifier` compartilham `syntax.text`, e `Macro`/`Number` compartilham
--- `syntax.orange`. As cores vêm de slots extras da própria paleta (fonte única).
---@param name string
---@return table<string, table>
function M.overrides(name)
  local s = require("astrograph.palettes." .. name).syntax
  return {
    Operator = { fg = s.operator },
    Macro = { fg = s.macro },
    CursorLineNr = { fg = s.cursorline, bold = true },
    Label = { fg = s.parameter },
    ["@variable.parameter"] = { fg = s.parameter },
  }
end

--- Aplica o tema astrograph. `vim.o.background` decide a variante (dark/light).
---@param opts? AstroThemeOpts
function M.setup(opts)
  M.register()

  local background = { dark = "astrograph", light = "astrograph-light" }
  local highlights, palettes = {}, {}
  for _, name in ipairs(PALETTES) do
    highlights[name] = M.overrides(name)
    palettes[name] = {} -- o motor exige a chave em `config.palettes`
  end

  require("astrotheme").setup(vim.tbl_deep_extend("force", {
    palette = background[vim.o.background] or "astrograph",
    background = background,
    palettes = palettes,
    highlights = highlights,
  }, opts or {}))

  require("astrotheme").load()
end

M.PALETTES = PALETTES

return M
