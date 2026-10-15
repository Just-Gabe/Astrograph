-- Tela de abertura do Astrograph, no espirito das telas de boas-vindas da Apple:
-- um icone (a estrela), o nome em traco fino, uma linha discreta dizendo o que
-- e e, abaixo, as acoes com o atalho alinhado a direita em cinza, como num menu
-- do macOS. Muito espaco vazio, uma cor de destaque so, nada piscando.
--
-- Entrada: a estrela acende, o nome e as acoes surgem com um fade curto (uma
-- vez, ~0,5 s) e a tela fica parada. `vim.g.snacks_animate = false` mostra tudo
-- direto. Qualquer falha cai para o header padrao, nunca quebra a abertura.
--
-- Cores derivadas do tema ativo (Normal, Title, Comment): funciona no escuro e
-- no claro, e acompanha a troca de tema.

local art = require "astrograph.art"

local M = {}

M.tagline = "Neovim com agentes de IA"
M.icon = "✦"

local HL = "Astrograph"

-- grupos do menu do snacks que entram no fade junto com a cena
local MENU = { "SnacksDashboardIcon", "SnacksDashboardDesc", "SnacksDashboardKey", "SnacksDashboardFooter", "SnacksDashboardSpecial" }

local function hl_color(name, attr)
  local ok, hl = pcall(vim.api.nvim_get_hl, 0, { name = name, link = false })
  local v = ok and hl[attr]
  return v and string.format("#%06x", v) or nil
end

local function hex(c) return tonumber(c:sub(2), 16) end

--- Interpola duas cores hexadecimais (`t` em 0..1)
local function lerp(a, b, t)
  local ai, bi = hex(a), hex(b)
  local function mix(shift)
    local av, bv = math.floor(ai / shift) % 256, math.floor(bi / shift) % 256
    return math.floor(av + (bv - av) * t + 0.5)
  end
  return string.format("#%02x%02x%02x", mix(65536), mix(256), mix(1))
end

--- Cores finais da cena a partir do tema ativo
local function palette()
  local dark = vim.o.background ~= "light"
  local bg = hl_color("Normal", "bg") or (dark and "#1f1f24" or "#f7f8f8")
  local fg = hl_color("Normal", "fg") or (dark and "#dcdce0" or "#3a3f4b")
  local accent = hl_color("Title", "fg") or "#409cff"
  local secondary = lerp(fg, bg, 0.45) -- "secondary label"
  local tertiary = lerp(fg, bg, 0.62) -- "tertiary label"
  local strong = dark and lerp(fg, "#ffffff", 0.5) or lerp(fg, "#000000", 0.3)
  return {
    bg = bg,
    accent = accent,
    -- nome: leve degrade vertical (metal escovado), nao arco-iris
    title = { strong, lerp(strong, secondary, 0.18), lerp(strong, secondary, 0.36) },
    tagline = secondary,
    menu = {
      SnacksDashboardIcon = secondary,
      SnacksDashboardDesc = fg,
      SnacksDashboardKey = tertiary,
      SnacksDashboardFooter = tertiary,
      SnacksDashboardSpecial = secondary,
    },
  }
end

local function set_fg(name, fg) vim.api.nvim_set_hl(0, name, { fg = fg }) end

--- Pinta a cena em `p` (0..1) do caminho da entrada; 1 = cores finais
local function paint(pal, p)
  local function at(start, len) return math.min(1, math.max(0, (p - start) / len)) end
  local function ease(x) return 1 - (1 - x) ^ 3 end
  set_fg(HL .. "Icon", lerp(pal.bg, pal.accent, ease(at(0, 0.45))))
  for i, col in ipairs(pal.title) do
    set_fg(HL .. "Title" .. i, lerp(pal.bg, col, ease(at(0.15 + (i - 1) * 0.08, 0.5))))
  end
  set_fg(HL .. "Tagline", lerp(pal.bg, pal.tagline, ease(at(0.4, 0.45))))
  for _, name in ipairs(MENU) do
    set_fg(name, lerp(pal.bg, pal.menu[name], ease(at(0.45, 0.55))))
  end
end

--- Composicao de acordo com a largura da janela
local function choose(cols)
  if cols >= 40 then return { banner = art.banners.thin, tagline = true } end
  return {}
end

--- Monta os itens do header: icone, nome e linha discreta, centralizados
local function build(layout)
  local lines = {}
  local function add(chunks) lines[#lines + 1] = chunks end
  add { { M.icon, hl = HL .. "Icon" } }
  if layout.banner then
    for i, line in ipairs(layout.banner.lines) do
      add { { line, hl = HL .. "Title" .. math.min(i, 3) } }
    end
  else
    add { { "Astrograph", hl = HL .. "Title1" } }
  end
  if layout.tagline then
    add { { " " } }
    add { { M.tagline, hl = HL .. "Tagline" } }
  end
  local text = {}
  for i, chunks in ipairs(lines) do
    for j, chunk in ipairs(chunks) do
      if j == #chunks and i < #lines then chunk[1] = chunk[1] .. "\n" end
      text[#text + 1] = chunk
    end
  end
  return { { text = text, align = "center", padding = 2 } }
end

-- O relogio da entrada so comeca depois do `VeryLazy`: antes disso o editor
-- ainda carrega plugins e o inicio do fade seria "consumido" sem aparecer.
local ready = vim.g.astrograph_very_lazy == true
local waiting = {}
vim.api.nvim_create_autocmd("User", {
  pattern = "VeryLazy",
  once = true,
  group = vim.api.nvim_create_augroup("AstrographDashboardReady", { clear = true }),
  callback = function()
    vim.g.astrograph_very_lazy = true
    vim.defer_fn(function()
      ready = true
      for _, fn in ipairs(waiting) do
        fn()
      end
      waiting = {}
    end, 30)
  end,
})

local playing = false

--- Entrada (~0,6 s, baseada em tempo real): se o editor engasgar, o fade pula
--- para o ponto certo em vez de pausar.
local function play()
  if playing then return end
  playing = true
  local pal = palette()
  paint(pal, 0)
  local function run()
    local timer = vim.uv.new_timer()
    local t0, pending = vim.uv.hrtime(), false
    local duration = 0.6
    timer:start(0, 16, function()
      if pending then return end
      pending = true
      vim.schedule(function()
        pending = false
        local p = math.min(1, (vim.uv.hrtime() - t0) / 1e9 / duration)
        paint(palette(), p)
        if p >= 1 and not timer:is_closing() then
          timer:stop()
          timer:close()
          playing = false
        end
      end)
    end)
  end
  if ready then run() else waiting[#waiting + 1] = run end
end

-- troca de tema: cores finais recalculadas
vim.api.nvim_create_autocmd("ColorScheme", {
  group = vim.api.nvim_create_augroup("AstrographDashboardColors", { clear = true }),
  callback = function()
    if not playing then paint(palette(), 1) end
  end,
})

local started = {}

--- Rodape discreto: so o tempo de abertura (substitui a secao `startup`)
function M.footer()
  local ok, stats = pcall(function() return require("lazy").stats() end)
  -- o tempo so existe depois do UIEnter; antes disso o rodape fica vazio
  if not ok or stats.startuptime <= 0 then return { align = "center", text = { { " ", hl = "footer" } } } end
  return {
    align = "center",
    text = {
      { ("pronto em %d ms"):format(math.floor(stats.startuptime + 0.5)), hl = "footer" },
    },
  }
end

--- Item de secao do dashboard (Gen). Nunca lanca: cai para o header padrao.
function M.scene(self)
  local ok, items = pcall(function()
    local built = build(choose(vim.o.columns))
    local buf = self and self.buf
    if buf and vim.api.nvim_buf_is_valid(buf) and not started[buf] then
      started[buf] = true
      vim.api.nvim_create_autocmd({ "BufWipeout", "BufDelete" }, {
        buffer = buf,
        once = true,
        callback = function() started[buf] = nil end,
      })
      if vim.g.snacks_animate == false then
        paint(palette(), 1)
      else
        play()
      end
    elseif not playing then
      paint(palette(), 1)
    end
    return built
  end)
  return ok and items or { { section = "header", padding = 5 } }
end

return M
