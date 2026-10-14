--- Valida o contraste de cada cor de sintaxe contra a base da paleta.
--- Regra: fg de sintaxe >= 4.5:1; comentário/mute (de-emphasized) >= 3:1.
---
--- Uso: nvim --headless -u NONE -l tools/check_contrast.lua
local script = debug.getinfo(1, "S").source:sub(2)
local root = vim.fn.fnamemodify(script, ":h:h")
vim.opt.rtp:prepend(root)

local function rgb(hex)
  hex = hex:gsub("#", "")
  return tonumber(hex:sub(1, 2), 16) / 255, tonumber(hex:sub(3, 4), 16) / 255, tonumber(hex:sub(5, 6), 16) / 255
end

local function channel(c) return c <= 0.03928 and c / 12.92 or ((c + 0.055) / 1.055) ^ 2.4 end

local function luminance(hex)
  local r, g, b = rgb(hex)
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
end

local function contrast(a, b)
  local l1, l2 = luminance(a), luminance(b)
  if l1 < l2 then l1, l2 = l2, l1 end
  return (l1 + 0.05) / (l2 + 0.05)
end

local EXEMPT = { comment = 3.0, mute = 3.0 }
local MIN = 4.5

local failed = false
for _, name in ipairs(require("astrograph").PALETTES) do
  local p = require("astrograph.palettes." .. name)
  local base = p.ui.base
  print(string.format("\n== %s (base %s) ==", name, base))
  local keys = vim.tbl_keys(p.syntax)
  table.sort(keys)
  for _, slot in ipairs(keys) do
    local fg = p.syntax[slot]
    if type(fg) == "string" and fg:match "^#" then
      local ratio = contrast(fg, base)
      local min = EXEMPT[slot] or MIN
      local ok = ratio >= min
      if not ok then failed = true end
      print(string.format("  %-10s %-8s = %5.2f  %s", slot, fg, ratio, ok and "ok" or "FALHA (< " .. min .. ")"))
    end
  end
end

if failed then
  print "\nContraste FALHOU."
  vim.cmd "cq"
else
  print "\nContraste OK."
  vim.cmd "q"
end
