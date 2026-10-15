-- Astrograph: decoração de texto própria — cores (`#rrggbb`, `rgb()`, `hsl()`…)
-- e comentários TODO/FIX/HACK/WARN/PERF/NOTE/TEST.
--
-- Substitui o nvim-highlight-colors e o todo-comments. A diferença está na
-- técnica: em vez de reagir a eventos (rolar, digitar) e reprocessar o texto,
-- usa o provedor de decoração nativo do Neovim (`nvim_set_decoration_provider`),
-- o mesmo mecanismo do destaque do Treesitter. O Neovim só pede a decoração das
-- linhas que está redesenhando; rolar a tela não dispara nenhum autocmd e cada
-- linha já vista fica em cache pelo próprio texto.

local M = {}

local api = vim.api
local ns = api.nvim_create_namespace "astronvim_decor"

M.enabled = { colors = true, todo = true }

---@type table<string, {color:string, alt?:string[]}>
M.keywords = {
  FIX = { color = "error", alt = { "FIXME", "BUG", "FIXIT", "ISSUE" } },
  TODO = { color = "info" },
  HACK = { color = "warning" },
  WARN = { color = "warning", alt = { "WARNING", "XXX" } },
  PERF = { color = "default", alt = { "OPTIM", "PERFORMANCE", "OPTIMIZE" } },
  NOTE = { color = "hint", alt = { "INFO" } },
  TEST = { color = "test", alt = { "TESTING", "PASSED", "FAILED" } },
}

local palette = {
  error = { "DiagnosticError", "ErrorMsg" },
  warning = { "DiagnosticWarn", "WarningMsg" },
  info = { "DiagnosticInfo" },
  hint = { "DiagnosticHint" },
  default = { "Identifier" },
  test = { "Identifier" },
}

local MAX_LINE = 400 -- linhas mais longas são ignoradas (minificados, dados)

-- palavra -> palavra principal (ex.: FIXME -> FIX)
local word_to_kw = {}
local function index_keywords()
  word_to_kw = {}
  for kw, def in pairs(M.keywords) do
    word_to_kw[kw] = kw
    for _, alt in ipairs(def.alt or {}) do
      word_to_kw[alt] = kw
    end
  end
end

---------------------------------------------------------------------------
-- Grupos de destaque (criados sob demanda e recriados ao trocar de tema)
---------------------------------------------------------------------------

local color_groups = {} ---@type table<string, string>

local function hex_to_rgb(hex) return tonumber(hex:sub(1, 2), 16), tonumber(hex:sub(3, 4), 16), tonumber(hex:sub(5, 6), 16) end

---Grupo com fundo na cor e texto preto/branco conforme a luminância.
---@param hex string `rrggbb` minúsculo
local function color_group(hex)
  local name = color_groups[hex]
  if name then return name end
  name = "AstroDecorColor_" .. hex
  local r, g, b = hex_to_rgb(hex)
  local luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255
  api.nvim_set_hl(0, name, { bg = "#" .. hex, fg = luminance > 0.5 and "#000000" or "#ffffff" })
  color_groups[hex] = name
  return name
end

local function hl_color(group_list, attr)
  for _, group in ipairs(group_list) do
    local hl = api.nvim_get_hl(0, { name = group, link = false })
    if hl[attr] then return hl[attr] end
  end
end

local todo_groups = {}
local function define_todo_groups()
  todo_groups = {}
  local normal_bg = api.nvim_get_hl(0, { name = "Normal", link = false }).bg
  for kw, def in pairs(M.keywords) do
    local fg = hl_color(palette[def.color] or palette.default, "fg") or 0xdcdcdc
    api.nvim_set_hl(0, "AstroDecorTodoBg" .. kw, { bg = fg, fg = normal_bg or 0x000000, bold = true })
    api.nvim_set_hl(0, "AstroDecorTodoFg" .. kw, { fg = fg })
    todo_groups[kw] = { bg = "AstroDecorTodoBg" .. kw, fg = "AstroDecorTodoFg" .. kw }
  end
end

---------------------------------------------------------------------------
-- Cores
---------------------------------------------------------------------------

local function clamp(v) return math.max(0, math.min(255, math.floor(v + 0.5))) end

local function hsl_to_hex(h, s, l)
  h, s, l = (h % 360) / 360, s / 100, l / 100
  local function hue(p, q, t)
    if t < 0 then t = t + 1 end
    if t > 1 then t = t - 1 end
    if t < 1 / 6 then return p + (q - p) * 6 * t end
    if t < 1 / 2 then return q end
    if t < 2 / 3 then return p + (q - p) * (2 / 3 - t) * 6 end
    return p
  end
  local r, g, b
  if s == 0 then
    r, g, b = l, l, l
  else
    local q = l < 0.5 and l * (1 + s) or l + s - l * s
    local p = 2 * l - q
    r, g, b = hue(p, q, h + 1 / 3), hue(p, q, h), hue(p, q, h - 1 / 3)
  end
  return string.format("%02x%02x%02x", clamp(r * 255), clamp(g * 255), clamp(b * 255))
end

---Cores encontradas numa linha: lista de `{ col_inicio, col_fim, hex }`.
---@param line string
---@return {[1]:integer,[2]:integer,[3]:string}[]
function M.find_colors(line)
  local out = {}
  if not (line:find("#", 1, true) or line:find("rgb", 1, true) or line:find("hsl", 1, true)) then return out end
  -- #rgb, #rrggbb, #rrggbbaa (não pode estar colado em letra/dígito)
  local init = 1
  while true do
    local s, e, hex = line:find("#(%x+)", init)
    if not s then break end
    local before = s > 1 and line:sub(s - 1, s - 1) or ""
    local after = line:sub(e + 1, e + 1)
    local len = #hex
    if not before:match "[%w_&]" and not after:match "[%w_]" and (len == 3 or len == 6 or len == 8) then
      local full = len == 3 and (hex:sub(1, 1):rep(2) .. hex:sub(2, 2):rep(2) .. hex:sub(3, 3):rep(2)) or hex:sub(1, 6)
      out[#out + 1] = { s - 1, e, full:lower() }
    end
    init = e + 1
  end
  -- rgb()/rgba()
  init = 1
  while true do
    local s, e, r, g, b = line:find("rgba?%(%s*(%d+)%s*,%s*(%d+)%s*,%s*(%d+)[^)]*%)", init)
    if not s then break end
    out[#out + 1] = { s - 1, e, string.format("%02x%02x%02x", clamp(r), clamp(g), clamp(b)) }
    init = e + 1
  end
  -- hsl()/hsla()
  init = 1
  while true do
    local s, e, h, sat, l = line:find("hsla?%(%s*([%d.]+)%s*,%s*([%d.]+)%%%s*,%s*([%d.]+)%%[^)]*%)", init)
    if not s then break end
    out[#out + 1] = { s - 1, e, hsl_to_hex(tonumber(h), tonumber(sat), tonumber(l)) }
    init = e + 1
  end
  return out
end

---Grupo de destaque da primeira cor de um texto (usado no menu de completação).
---@param text string
---@return string?
function M.color_hl(text)
  local found = M.find_colors(text or "")[1]
  return found and color_group(found[3]) or nil
end

---------------------------------------------------------------------------
-- TODO / FIX / …
---------------------------------------------------------------------------

---Palavras-chave numa linha: `{ col_inicio, col_fim, palavra_principal }`.
---@param line string
function M.find_todos(line)
  local out = {}
  if not line:find "%u%u" then return out end
  local init = 1
  while true do
    local s, e, word = line:find("(%u+)%s*:", init)
    if not s then break end
    local before = s > 1 and line:sub(s - 1, s - 1) or ""
    local kw = word_to_kw[word]
    if kw and not before:match "[%w_]" then out[#out + 1] = { s - 1, s - 1 + #word, kw } end
    init = e + 1
  end
  return out
end

---A posição está dentro de um comentário? Treesitter quando houver parser;
---senão, o `commentstring` aparecer antes da palavra.
local function in_comment(buf, row, col, line)
  local ok, node = pcall(vim.treesitter.get_node, { bufnr = buf, pos = { row, col }, ignore_injections = false })
  if ok and node then
    while node do
      if node:type():find "comment" then return true end
      node = node:parent()
    end
    return false
  end
  local cs = vim.bo[buf].commentstring
  local prefix = vim.trim((cs or ""):match "^(.-)%%s" or "")
  return prefix ~= "" and line:sub(1, col):find(prefix, 1, true) ~= nil
end

---------------------------------------------------------------------------
-- Provedor de decoração
---------------------------------------------------------------------------

-- cache por buffer: texto da linha -> decoração calculada (o texto é a chave,
-- então editar a linha invalida sozinho)
local cache = {} ---@type table<integer, {n:integer, lines:table<string, table>}>
local active = {} ---@type table<integer, boolean>

local function buf_enabled(buf)
  local v = active[buf]
  if v ~= nil then return v end
  v = vim.bo[buf].buftype == "" and vim.bo[buf].buflisted
  if v then
    local ok, buffer = pcall(require, "astrocore.buffer")
    if ok and buffer.is_large(buf) then v = false end
  end
  active[buf] = v
  return v
end

local function decorate(buf, line)
  local c = cache[buf]
  if not c then
    c = { n = 0, lines = {} }
    cache[buf] = c
  end
  local d = c.lines[line]
  if d then return d end
  d = {
    colors = M.enabled.colors and M.find_colors(line) or {},
    todos = M.enabled.todo and M.find_todos(line) or {},
  }
  if c.n > 4000 then c.n, c.lines = 0, {} end
  c.n = c.n + 1
  c.lines[line] = d
  return d
end

local function set(buf, row, col, end_col, hl, priority)
  pcall(api.nvim_buf_set_extmark, buf, ns, row, col, {
    end_row = row,
    end_col = end_col,
    hl_group = hl,
    ephemeral = true,
    priority = priority,
  })
end

local function on_line(_, _, buf, row)
  local line = api.nvim_buf_get_lines(buf, row, row + 1, false)[1]
  if not line or line == "" or #line > MAX_LINE then return end
  local d = decorate(buf, line)
  for _, m in ipairs(d.colors) do
    set(buf, row, m[1], m[2], color_group(m[3]), 150)
  end
  for _, m in ipairs(d.todos) do
    if in_comment(buf, row, m[1], line) then
      local groups = todo_groups[m[3]]
      if groups then
        -- estilo "wide": a palavra com um espaço de cada lado, e o texto em cor
        set(buf, row, math.max(0, m[1] - 1), math.min(#line, m[2] + 1), groups.bg, 160)
        set(buf, row, math.min(#line, m[2] + 1), #line, groups.fg, 155)
      end
    end
  end
end

function M.redraw() pcall(vim.cmd.redraw, { bang = true }) end

---Liga/desliga uma das decorações (`"colors"` ou `"todo"`).
---@param kind "colors"|"todo"
function M.toggle(kind)
  M.enabled[kind] = not M.enabled[kind]
  cache = {}
  M.redraw()
  local label = kind == "colors" and "Destaque de cores" or "Destaque de TODOs"
  vim.notify(("%s %s"):format(label, M.enabled[kind] and "ligado" or "desligado"), vim.log.levels.INFO, { title = "Astrograph" })
end

---------------------------------------------------------------------------
-- Navegação e busca de TODOs
---------------------------------------------------------------------------

local function vim_regex()
  local words = vim.tbl_keys(word_to_kw)
  table.sort(words)
  return [[\v<(]] .. table.concat(words, "|") .. [[)\s*:]]
end

---Pula para o próximo (`1`) ou anterior (`-1`) TODO dentro de comentário.
---@param dir 1|-1
function M.jump(dir)
  local buf = api.nvim_get_current_buf()
  local pattern = vim_regex()
  local flags = dir == 1 and "W" or "bW"
  local view = vim.fn.winsaveview()
  for _ = 1, 500 do
    local lnum, col = unpack(vim.fn.searchpos(pattern, flags))
    if lnum == 0 then break end
    local line = api.nvim_buf_get_lines(buf, lnum - 1, lnum, false)[1] or ""
    if in_comment(buf, lnum - 1, col - 1, line) then return end
  end
  vim.fn.winrestview(view)
  vim.notify("Nenhum TODO " .. (dir == 1 and "abaixo" or "acima"), vim.log.levels.INFO, { title = "Astrograph" })
end

---Busca todos os TODOs do projeto (ripgrep pelo picker do snacks).
function M.pick()
  local words = vim.tbl_keys(word_to_kw)
  table.sort(words)
  require("snacks").picker.grep {
    title = "TODOs",
    search = [[\b(]] .. table.concat(words, "|") .. [[)\s*:]],
    regex = true,
    live = false,
  }
end

---------------------------------------------------------------------------

local did_setup = false
function M.setup()
  if did_setup then return end
  did_setup = true
  index_keywords()
  define_todo_groups()
  api.nvim_set_decoration_provider(ns, {
    on_win = function(_, _, buf)
      if not (M.enabled.colors or M.enabled.todo) then return false end
      return buf_enabled(buf)
    end,
    on_line = on_line,
  })
  local group = api.nvim_create_augroup("astronvim_decor", { clear = true })
  api.nvim_create_autocmd("ColorScheme", {
    group = group,
    desc = "Recriar cores da decoração ao trocar de tema",
    callback = function()
      color_groups = {}
      define_todo_groups()
    end,
  })
  api.nvim_create_autocmd({ "BufWipeout", "BufFilePost" }, {
    group = group,
    desc = "Limpar cache da decoração",
    callback = function(args)
      cache[args.buf], active[args.buf] = nil, nil
    end,
  })
  api.nvim_create_autocmd("OptionSet", {
    group = group,
    pattern = { "buftype", "buflisted" },
    desc = "Reavaliar se o buffer recebe decoração",
    callback = function() active[api.nvim_get_current_buf()] = nil end,
  })
  M.redraw()
end

return M
