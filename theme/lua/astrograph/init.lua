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
  local p = require("astrograph.palettes." .. name)
  local s, u = p.syntax, p.ui
  local hl = {
    Operator = { fg = s.operator },
    Macro = { fg = s.macro },
    CursorLineNr = { fg = s.cursorline, bold = true },
    Label = { fg = s.parameter },
    ["@variable.parameter"] = { fg = s.parameter },
  }
  if name == "astrograph" then
    -- o conteudo manda: numeros de linha e guias quase somem, a linha atual
    -- aparece so pelo numero claro (sem negrito gritando)
    hl.CursorLineNr = { fg = s.cursorline }
    hl.LineNr = { fg = "#4A4A52" }
    hl.SignColumn = { bg = u.base }
    hl.WinSeparator = { fg = u.split }
    hl.SnacksIndent = { fg = "#2C2C32" }
    hl.SnacksIndentScope = { fg = "#45454D" }
    -- janelas flutuantes: superficie elevada, borda discreta da mesma cor
    hl.NormalFloat = { fg = u.text, bg = u.float }
    hl.FloatBorder = { fg = u.border, bg = u.float }
    hl.FloatTitle = { fg = u.text_active, bg = u.float, bold = true }
    hl.Pmenu = { fg = u.text, bg = u.float }
    hl.PmenuSel = { fg = u.text_match, bg = u.menu_selection }
    hl.PmenuThumb = { bg = u.scrollbar }
    -- palavras-chave sem italico/negrito: leitura calma, como no Xcode
    hl.Keyword = { fg = s.purple }
    hl.Statement = { fg = s.purple }
    hl.Comment = { fg = s.comment, italic = false }
  end
  return hl
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
