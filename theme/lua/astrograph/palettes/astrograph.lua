--- Paleta Astrograph (dark) — identidade do dashboard: noite azul + estrelas douradas.
--- Mapeamento para os slots do astrotheme:
---   statement/keyword -> purple, função/tipo/special -> blue, string -> green,
---   constante/float/struct -> yellow, número/boolean/macro -> orange, identificador -> text.
--- Slots extras (operator/macro/cursorline/parameter) não existem no motor e são
--- aplicados como overrides de highlight em `astrograph/init.lua`.
local color = require "astrograph.lib.color"

---@class AstroThemePalette
local c = {
  none = "NONE",
  syntax = {},
  ui = {},
  term = {},
  icon = {},
}

--------------------------------
--- Syntax
--------------------------------
c.syntax.red = "#FF7E9D"
c.syntax.orange = "#F9E2AF"
c.syntax.yellow = "#F9E2AF"
c.syntax.green = "#7CE38B"
c.syntax.cyan = "#5DE4D0"
c.syntax.blue = "#7DD3FC"
c.syntax.purple = "#8AB4FF"
c.syntax.text = "#D8DCF0"
c.syntax.comment = "#6B6B8A"
c.syntax.mute = "#6B6B8A"

-- extras astrograph (aplicados via overrides de highlight)
c.syntax.operator = "#BFC5D6"
c.syntax.macro = "#E8C982"
c.syntax.cursorline = "#F9E2AF"
c.syntax.parameter = "#C7D2FE"

--------------------------------
--- UI
--------------------------------
c.ui.red = "#FF7E9D"
c.ui.orange = "#E8C982"
c.ui.yellow = "#F9E2AF"
c.ui.green = "#7CE38B"
c.ui.cyan = "#5DE4D0"
c.ui.blue = "#7DD3FC"
c.ui.purple = "#F472B6"

c.ui.accent = c.ui.blue

c.ui.tabline = "#111317"
c.ui.winbar = "#797D87"
c.ui.tool = "#16181D"
c.ui.base = "#1A1D23"
c.ui.inactive_base = "#16181D"
c.ui.statusline = "#111317"
c.ui.split = "#111317"
c.ui.float = "#14161B"
c.ui.title = c.ui.accent
c.ui.border = "#3A3E47"
c.ui.current_line = "#1E222A"
c.ui.scrollbar = c.ui.accent
c.ui.selection = "#243447"
-- TODO: combine menu_selection and selection
c.ui.menu_selection = c.ui.selection
c.ui.highlight = "#23272F"
c.ui.none_text = "#3A3E47"
c.ui.text = "#C3C7D4"
c.ui.text_active = "#D8DCF0"
c.ui.text_inactive = "#545A68"
c.ui.text_match = "#E0E0EE"

c.ui.prompt = "#21242A"

--------------------------------
--- terminal
--------------------------------
c.term.black = c.ui.tabline
c.term.bright_black = color.new(c.ui.tabline):lighten(35):tohex()

c.term.red = c.syntax.red
c.term.bright_red = color.new(c.syntax.red):lighten(35):tohex()

c.term.green = c.syntax.green
c.term.bright_green = color.new(c.syntax.green):lighten(35):tohex()

c.term.yellow = c.syntax.yellow
c.term.bright_yellow = color.new(c.syntax.yellow):lighten(35):tohex()

c.term.blue = c.syntax.blue
c.term.bright_blue = color.new(c.syntax.blue):lighten(35):tohex()

c.term.purple = c.syntax.purple
c.term.bright_purple = color.new(c.syntax.purple):lighten(35):tohex()

c.term.cyan = c.syntax.cyan
c.term.bright_cyan = color.new(c.syntax.cyan):lighten(35):tohex()

c.term.white = c.ui.text
c.term.bright_white = color.new(c.syntax.text):lighten(35):tohex()

c.term.background = c.ui.base
c.term.foreground = c.ui.text

--------------------------------
--- Icons
--------------------------------
c.icon.c = "#519aba"
c.icon.css = "#61afef"
c.icon.deb = "#a1b7ee"
c.icon.docker = "#384d54"
c.icon.html = "#de8c92"
c.icon.jpeg = "#c882e7"
c.icon.jpg = "#c882e7"
c.icon.js = "#ebcb8b"
c.icon.jsx = "#519ab8"
c.icon.kt = "#7bc99c"
c.icon.lock = "#c4c720"
c.icon.lua = "#51a0cf"
c.icon.mp3 = "#d39ede"
c.icon.mp4 = "#9ea3de"
c.icon.out = "#abb2bf"
c.icon.png = "#c882e7"
c.icon.py = "#a3b8ef"
c.icon.rb = "#ff75a0"
c.icon.robots = "#abb2bf"
c.icon.rpm = "#fca2aa"
c.icon.rs = "#dea584"
c.icon.toml = "#39bf39"
c.icon.ts = "#519aba"
c.icon.ttf = "#abb2bf"
c.icon.vue = "#7bc99c"
c.icon.woff = "#abb2bf"
c.icon.woff2 = "#abb2bf"
c.icon.zip = "#f9d71c"
c.icon.md = "#519aba"
c.icon.pkg = "#d39ede"

return c
