--- Tema lualine derivado 1:1 da paleta astrograph-light. Sem cores hardcoded.
local p = require "astrograph.palettes.astrograph-light"
local ui = p.ui

local function mode(bg)
  return {
    a = { fg = "#FFFFFF", bg = bg, gui = "bold" },
    b = { fg = ui.text_active, bg = ui.base },
    c = { fg = ui.winbar, bg = ui.statusline },
  }
end

return {
  normal = mode(ui.accent),
  insert = mode(ui.green),
  visual = mode(ui.purple),
  replace = mode(ui.red),
  command = mode(ui.yellow),
  terminal = mode(ui.green),
  inactive = {
    a = { fg = ui.winbar, bg = ui.statusline, gui = "bold" },
    b = { fg = ui.text_inactive, bg = ui.base },
    c = { fg = ui.winbar, bg = ui.statusline },
  },
}
