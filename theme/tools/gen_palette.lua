--- Gera `theme/palette.json` a partir das paletas Lua (fonte única de verdade).
--- Consumido pelo instalador e pelo repo.
---
--- Uso: nvim --headless -u NONE -l tools/gen_palette.lua
local script = debug.getinfo(1, "S").source:sub(2)
local root = vim.fn.fnamemodify(script, ":h:h")
vim.opt.rtp:prepend(root)

local palettes = {}
for _, name in ipairs(require("astrograph").PALETTES) do
  palettes[name] = require("astrograph.palettes." .. name)
end

local data = {
  version = 1,
  generated = os.date "%Y-%m-%d",
  background = { dark = "astrograph", light = "astrograph-light" },
  palettes = palettes,
}

local out = string.format("%s/palette.json", root)
local f = assert(io.open(out, "w"))
f:write(vim.json.encode(data))
f:write "\n"
f:close()

print("palette.json gerado em " .. out)
