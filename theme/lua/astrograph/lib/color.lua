--- Helper de cor mínimo para o tema Astrograph.
--- Auto-contido (não depende do astrotheme) para permitir gerar `palette.json`
--- e validar contraste em modo headless, sem carregar o config inteiro.
local M = {}

local Color = {}
Color.__index = Color

local function clamp(v) return math.max(0, math.min(1, v)) end

function Color:tohex()
  return string.format(
    "#%02X%02X%02X",
    math.floor(clamp(self.r) * 255 + 0.5),
    math.floor(clamp(self.g) * 255 + 0.5),
    math.floor(clamp(self.b) * 255 + 0.5)
  )
end

function Color:lighten(amount)
  amount = amount / 100
  return setmetatable({
    r = self.r + (1 - self.r) * amount,
    g = self.g + (1 - self.g) * amount,
    b = self.b + (1 - self.b) * amount,
  }, Color)
end

function Color:darken(amount)
  amount = amount / 100
  return setmetatable({
    r = self.r * (1 - amount),
    g = self.g * (1 - amount),
    b = self.b * (1 - amount),
  }, Color)
end

function Color:blend(other, amount)
  other = M.new(other)
  return setmetatable({
    r = self.r * (1 - amount) + other.r * amount,
    g = self.g * (1 - amount) + other.g * amount,
    b = self.b * (1 - amount) + other.b * amount,
  }, Color)
end

---@param hex string
---@return Color
function M.new(hex)
  hex = tostring(hex):gsub("#", "")
  return setmetatable({
    r = tonumber(hex:sub(1, 2), 16) / 255,
    g = tonumber(hex:sub(3, 4), 16) / 255,
    b = tonumber(hex:sub(5, 6), 16) / 255,
  }, Color)
end

return M
