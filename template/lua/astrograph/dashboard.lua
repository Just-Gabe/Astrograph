-- Cena de abertura do Astrograph: lua crescente + estrelas + banner ASTROGRAPH.
-- Substitui apenas o cabecalho do dashboard do AstroNvim (o resto permanece).
-- Arte "assada" em `astrograph.art` (gere com `python tools/gen_art.py`).
--
-- Guardas: `vim.g.snacks_animate = false` deixa a cena estatica; qualquer falha
-- cai para o header padrao, nunca quebra a abertura.

local art = require "astrograph.art"

local M = {}

-- Cores da cena (independentes do tema)
local BG = "#1a1d23"
local MOON_FROM = "#3a3a5c"
local MOON_TO = "#c7d2fe"
local STAR_BRIGHT = "#f9e2af"
local STAR_DIM = "#6b6b8a"
local GRAD = { "#7dd3fc", "#818cf8", "#a78bfa", "#c084fc", "#e879f9", "#f472b6" }

local HL = "Astrograph"
local started, timers = {}, {}

local function hex(c)
  return tonumber(c:sub(2), 16)
end

--- Interpola duas cores hexadecimais (`t` em 0..1)
local function lerp(a, b, t)
  local ai, bi = hex(a), hex(b)
  local function mix(shift)
    local av, bv = math.floor(ai / shift) % 256, math.floor(bi / shift) % 256
    return math.floor(av + (bv - av) * t + 0.5)
  end
  return string.format("#%02x%02x%02x", mix(65536), mix(256), mix(1))
end

--- Amostra `n` paradas do gradiente global (azul -> rosa)
local function gradient(n)
  local out = {}
  for i = 1, n do
    local pos = (#GRAD - 1) * (n == 1 and 0 or (i - 1) / (n - 1))
    local lo = math.floor(pos) + 1
    local hi = math.min(lo + 1, #GRAD)
    out[i] = lerp(GRAD[lo], GRAD[hi], pos - (lo - 1))
  end
  return out
end

local function set_fg(name, fg, bold)
  vim.api.nvim_set_hl(0, name, { fg = fg, bold = bold or false })
end

--- Composicao de acordo com a largura da janela
local function choose(cols)
  if cols >= 100 then
    return { moon = art.moon.large, banner = art.banners.ansi_shadow, gap = 2 }
  elseif cols >= 74 then
    return { moon = art.moon.small, banner = art.banners.standard, gap = 2 }
  elseif cols >= 40 then
    return { banner = art.banners.slant }
  end
  return { banner = art.banners.mini }
end

local function define_hl(grad)
  for i, col in ipairs(grad) do
    set_fg(HL .. "Grad" .. i, col)
  end
  set_fg(HL .. "Moon", MOON_TO, true)
  for i = 1, 3 do
    set_fg(HL .. "Star" .. i, STAR_BRIGHT)
  end
end

--- Coloca a arte no grid; `hl_for(r)` (r 0-baseado) define o highlight da linha
local function place(grid, lines, x, y, hl_for)
  for r, line in ipairs(lines) do
    local row = grid[y + r]
    for c, ch in ipairs(vim.fn.split(line, "\\zs")) do
      if ch ~= " " and ch ~= "" then
        row[x + c] = { ch = ch, hl = hl_for(r - 1) }
      end
    end
  end
end

--- Monta a cena e devolve a lista de itens do dashboard + o gradiente usado
local function build(layout)
  local banner, moon = layout.banner, layout.moon
  local bl = banner.lines
  local bh, bw = #bl, banner.width
  local mh = moon and moon.height or 0
  local mw = moon and moon.width or 0
  local maxh = math.max(bh, mh)
  local height = maxh + 5
  local bx = moon and (mw + (layout.gap or 2)) or 0
  local width = bx + bw
  local my = moon and (2 + math.floor((maxh - mh) / 2)) or 0
  local by = 2 + math.floor((maxh - bh) / 2)

  local grid = {}
  for r = 1, height do
    grid[r] = {}
  end

  if moon then
    place(grid, moon.lines, 0, my, function()
      return HL .. "Moon"
    end)
  end
  place(grid, bl, bx, by, function(r)
    return HL .. "Grad" .. (r + 1)
  end)

  for _, st in ipairs(art.stars) do
    if st.y < height and st.x < width and not grid[st.y + 1][st.x + 1] then
      grid[st.y + 1][st.x + 1] = { ch = art.starset[st.s], hl = HL .. "Star" .. st.s }
    end
  end

  local texts = {}
  for r = 1, height do
    local row, chunks, cur, curhl, seen = grid[r], {}, {}, nil, false
    for c = 1, width do
      local cell = row[c]
      local ch = (cell and cell.ch) or " "
      local hl = cell and cell.hl or nil
      if not seen then
        cur, curhl, seen = { ch }, hl, true -- luacheck: ignore
      elseif hl == curhl then
        cur[#cur + 1] = ch
      else
        chunks[#chunks + 1] = { table.concat(cur), hl = curhl }
        cur, curhl = { ch }, hl
      end
    end
    if seen then
      chunks[#chunks + 1] = { table.concat(cur), hl = curhl }
    end
    for i, chunk in ipairs(chunks) do
      if i == #chunks and r < height then
        chunk[1] = chunk[1] .. "\n"
      end
      texts[#texts + 1] = chunk
    end
  end

  return { { text = texts, align = "center" } }, gradient(#bl)
end

--- Animacao de entrada: lua em fade + banner revelado linha a linha
local function play(grad)
  local animate = _G.Snacks and Snacks.animate
  if not animate then
    return
  end
  for i = 1, #grad do
    set_fg(HL .. "Grad" .. i, BG)
  end
  set_fg(HL .. "Moon", MOON_FROM, true)
  pcall(animate, 0, 1, function(v)
    for i = 1, #grad do
      local p = math.min(1, math.max(0, (v - (i - 1) * 0.12) / 0.4))
      set_fg(HL .. "Grad" .. i, lerp(BG, grad[i], p))
    end
    set_fg(HL .. "Moon", lerp(MOON_FROM, MOON_TO, math.min(1, v / 0.5)), true)
  end, { duration = 400, easing = "outQuad" })
end

--- Twinkle continuo das estrelas (8 Hz) enquanto o dashboard existir
local function twinkle(buf)
  local timer = vim.uv.new_timer()
  local t = 0
  timer:start(0, 125, vim.schedule_wrap(function()
    if not vim.api.nvim_buf_is_valid(buf) or vim.g.snacks_animate == false then
      for i = 1, 3 do
        set_fg(HL .. "Star" .. i, STAR_BRIGHT)
      end
      pcall(timer.stop, timer)
      pcall(timer.close, timer)
      if timers[buf] == timer then
        timers[buf] = nil
      end
      return
    end
    t = t + 1
    for i = 1, 3 do
      local w = 0.5 + 0.5 * math.sin(t * 0.45 + i * 2.1)
      set_fg(HL .. "Star" .. i, lerp(STAR_DIM, STAR_BRIGHT, w))
    end
  end))
  timers[buf] = timer
end

--- Liga a animacao uma unica vez por buffer do dashboard
local function start(buf, grad)
  if started[buf] then
    return
  end
  started[buf] = true
  local aug = vim.api.nvim_create_augroup("AstrographDashboard" .. buf, { clear = true })
  vim.api.nvim_create_autocmd({ "BufWipeout", "BufDelete" }, {
    buffer = buf,
    group = aug,
    once = true,
    callback = function()
      started[buf] = nil
      local timer = timers[buf]
      if timer then
        pcall(timer.stop, timer)
        pcall(timer.close, timer)
        timers[buf] = nil
      end
    end,
  })
  if vim.g.snacks_animate == false then
    return
  end
  play(grad)
  twinkle(buf)
end

--- Item de secao do dashboard (Gen). Nunca lanca: cai para o header padrao.
function M.scene(self)
  local ok, items = pcall(function()
    local layout = choose(vim.o.columns)
    local built, grad = build(layout)
    define_hl(grad)
    if self and self.buf and vim.api.nvim_buf_is_valid(self.buf) then
      start(self.buf, grad)
    end
    return built
  end)
  return ok and items or { { section = "header", padding = 5 } }
end

return M
