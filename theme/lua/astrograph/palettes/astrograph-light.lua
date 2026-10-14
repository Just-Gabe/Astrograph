--- Paleta Astrograph (light) — mesmos matizes do dark, escurecidos p/ contraste
--- sobre fundo claro. Mesmo mapeamento de slots do `astrograph`.
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
c.syntax.red = "#C0392B"
c.syntax.orange = "#8A6000"
c.syntax.yellow = "#8A6000"
c.syntax.green = "#2E7D3F"
c.syntax.cyan = "#2A6E68"
c.syntax.blue = "#0E6E8C"
c.syntax.purple = "#2E5FBF"
c.syntax.text = "#3A3F4B"
c.syntax.comment = "#6F7488"
c.syntax.mute = "#6F7488"

-- extras astrograph (aplicados via overrides de highlight)
c.syntax.operator = "#5A6070"
c.syntax.macro = "#8A6A1F"
c.syntax.cursorline = "#8A6000"
c.syntax.parameter = "#5B5FC7"

--------------------------------
--- UI
--------------------------------
c.ui.red = "#C0392B"
c.ui.orange = "#8A6A1F"
c.ui.yellow = "#8A6000"
c.ui.green = "#2E7D3F"
c.ui.cyan = "#21B386"
c.ui.blue = "#2E5FBF"
c.ui.purple = "#C24E8A"

c.ui.accent = c.ui.blue

c.ui.tabline = "#E1E2E4"
c.ui.winbar = "#999FA3"
c.ui.tool = "#F0F0F1"
c.ui.base = "#F7F8F8"
c.ui.inactive_base = "#EAEBEB"
c.ui.statusline = "#E1E2E4"
c.ui.split = "#E8E9EA"
c.ui.float = "#E1E2E3"
c.ui.title = c.ui.accent
c.ui.border = "#C9CCD1"
c.ui.current_line = "#EAEBEB"
c.ui.scrollbar = c.ui.accent
c.ui.selection = "#E7E9EB"
-- TODO: combine menu_selection and selection
c.ui.menu_selection = c.ui.selection
c.ui.highlight = "#DADBDD"
c.ui.none_text = "#B5B9BD"
c.ui.text = "#4A4D52"
c.ui.text_active = "#2B2E33"
c.ui.text_inactive = "#AEB3B6"
c.ui.text_match = "#17191C"

c.ui.prompt = "#F0F0F1"

--------------------------------
--- terminal
--------------------------------
c.term.black = c.ui.tabline
c.term.bright_black = color.new(c.ui.tabline):darken(40):tohex()

c.term.red = c.syntax.red
c.term.bright_red = color.new(c.syntax.red):lighten(30):tohex()

c.term.green = c.syntax.green
c.term.bright_green = color.new(c.syntax.green):lighten(30):tohex()

c.term.yellow = c.syntax.yellow
c.term.bright_yellow = color.new(c.syntax.yellow):lighten(30):tohex()

c.term.blue = c.syntax.blue
c.term.bright_blue = color.new(c.syntax.blue):lighten(30):tohex()

c.term.purple = c.syntax.purple
c.term.bright_purple = color.new(c.syntax.purple):lighten(30):tohex()

c.term.cyan = c.syntax.cyan
c.term.bright_cyan = color.new(c.syntax.cyan):lighten(30):tohex()

c.term.white = c.ui.text
c.term.bright_white = color.new(c.syntax.text):lighten(30):tohex()

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
