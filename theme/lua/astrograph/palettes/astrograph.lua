--- Paleta Astrograph (dark) — inspirada no Xcode e nas cores de sistema da Apple.
---
--- Principios: fundo grafite neutro (nada de azul-neon), texto quase branco mas
--- nao puro, cores de sintaxe suaves e pouco saturadas (dias inteiros de leitura
--- sem cansar) e um unico destaque azul. A interface (barras, laterais,
--- janelas flutuantes) se separa do editor por tom de superficie, nao por linhas.
---
--- Mapeamento para os slots do astrotheme:
---   statement/keyword -> purple, funcao/tipo/special -> blue, string -> green,
---   constante/float/struct -> yellow, numero/boolean/macro -> orange, identificador -> text.
--- Slots extras (operator/macro/cursorline/parameter) nao existem no motor e sao
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
--- Syntax (tons do Xcode, um pouco mais calmos)
--------------------------------
c.syntax.red = "#FF8A80" -- tags, erros de sintaxe
c.syntax.orange = "#D9C97C" -- numeros e booleanos
c.syntax.yellow = "#D9C97C" -- constantes
c.syntax.green = "#FC8B7A" -- strings (o "salmao" do Xcode)
c.syntax.cyan = "#9ED9CC"
c.syntax.blue = "#7CC7E8" -- funcoes e tipos
c.syntax.purple = "#F38DBA" -- palavras-chave
c.syntax.text = "#DCDCE0"
c.syntax.comment = "#7F8C98"
c.syntax.mute = "#7F8C98"

-- extras astrograph (aplicados via overrides de highlight)
c.syntax.operator = "#C2C2C8"
c.syntax.macro = "#F2A86F"
c.syntax.cursorline = "#DCDCE0"
c.syntax.parameter = "#C9B6F2"

--------------------------------
--- UI (cores de sistema da Apple, modo escuro)
--------------------------------
c.ui.red = "#FF6961"
c.ui.orange = "#FFB340"
c.ui.yellow = "#FFD426"
c.ui.green = "#5DD879"
c.ui.cyan = "#70D7FF"
c.ui.blue = "#409CFF"
c.ui.purple = "#C38BF0"

c.ui.accent = c.ui.blue

-- superficies: editor (base) mais escuro; barras e laterais um tom acima;
-- janelas flutuantes mais acima ainda (elevacao), como no macOS
c.ui.base = "#1F1F24"
c.ui.inactive_base = "#1F1F24"
c.ui.tabline = "#26262C"
c.ui.statusline = "#26262C"
c.ui.tool = "#232328"
c.ui.split = "#2E2E34"
c.ui.float = "#2A2A30"
c.ui.title = c.ui.accent
c.ui.border = "#3C3C44"
c.ui.current_line = "#25262B"
c.ui.scrollbar = "#4A4A52"
c.ui.selection = "#3A4A62"
-- TODO: combine menu_selection and selection
c.ui.menu_selection = "#34507A"
c.ui.highlight = "#2E3036"
c.ui.none_text = "#3C3C44"
c.ui.winbar = "#8E8E93"
c.ui.text = "#C8C8CE"
c.ui.text_active = "#F2F2F7"
c.ui.text_inactive = "#6E6E76"
c.ui.text_match = "#FFFFFF"

c.ui.prompt = "#2A2A30"

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
