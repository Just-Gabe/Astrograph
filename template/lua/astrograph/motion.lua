-- Movimento da interface do Astrograph:
--
--   * janelas flutuantes e menus surgem com um fade curto;
--   * o explorador lateral abre e fecha deslizando a largura;
--   * `dd` apaga na hora e a linha apagada colapsa verticalmente nela mesma e some.
--
-- Busca, hover do LSP, diagnósticos, notificações, menu de atalhos e chat
-- aparecem em ~140 ms, desacelerando no fim (outCubic), em vez de "estalar" na
-- tela. Usa o `winblend` (transparência nativa do Neovim): parte de 100
-- (invisível) até a transparência que a janela já teria.
--
-- O relógio é de tempo real: se o editor engasgar, a animação pula quadros em
-- vez de pausar. Menus que surgem enquanto você digita (completação, atalhos)
-- usam um fade mais curto; a linha de comando fica de fora (precisa ser imediata).
-- Segue o liga/desliga global das animações (`vim.g.snacks_animate`, <Leader>um).

local M = {}

M.duration = 0.14 -- segundos

-- menus que aparecem enquanto você digita: fade mais curto
M.fast_ft = {
  ["blink-cmp-menu"] = 0.09,
  ["blink-cmp-documentation"] = 0.09,
  ["blink-cmp-signature"] = 0.09,
  wk = 0.1,
}

-- tipos de janela que não animam
M.skip_ft = {
  ["smear-cursor"] = true,
  cmd = true, -- ui2: linha de comando
  pager = true,
  dialog = true,
  snacks_dashboard = true,
  ["snacks_terminal"] = true,
}

local function ease_out_cubic(x) return 1 - (1 - x) ^ 3 end

local function enabled() return vim.g.snacks_animate ~= false and not (vim.env.SSH_CONNECTION or vim.env.SSH_TTY) end

---Relógio de quadros em tempo real (~60 fps). `on_frame(progresso 0..1)` roda
---no loop principal; se o editor estiver ocupado, o quadro seguinte não é
---enfileirado e o progresso salta para o ponto certo no tempo.
---@param duration number segundos
---@param on_frame fun(x:number):boolean? retorne `false` para parar antes
---@param on_done? fun()
local function animate(duration, on_frame, on_done)
  local timer = vim.uv.new_timer()
  local t0, pending, stopped = vim.uv.hrtime(), false, false
  local function stop()
    if stopped then return end
    stopped = true
    pcall(timer.stop, timer)
    pcall(timer.close, timer)
    if on_done then on_done() end
  end
  -- 60 fps no terminal; ~120 na UI do Astrograph (quadro custa centesimos de ms)
  timer:start(0, vim.g.astrograph_gui and 8 or 16, function()
    if pending or stopped then return end
    pending = true
    vim.schedule(function()
      pending = false
      if stopped then return end
      local x = math.min(1, (vim.uv.hrtime() - t0) / 1e9 / duration)
      if on_frame(x) == false or x >= 1 then stop() end
    end)
  end)
  return stop
end
M.animate = animate

---@param win integer
---@param duration? number
local function fade_in(win, duration)
  local ok, target = pcall(function() return vim.wo[win].winblend end)
  if not ok then return end
  target = target or 0
  if target >= 100 then return end
  vim.wo[win].winblend = 100
  animate(duration or M.duration, function(x)
    if not vim.api.nvim_win_is_valid(win) then return false end
    local blend = math.floor(100 - (100 - target) * ease_out_cubic(x) + 0.5)
    pcall(function() vim.wo[win].winblend = blend end)
  end)
end

local function scan()
  if not enabled() then return end
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local cfg = vim.api.nvim_win_get_config(win)
    if cfg.relative ~= "" and not vim.w[win].astrograph_motion then
      vim.w[win].astrograph_motion = true
      local buf = vim.api.nvim_win_get_buf(win)
      local ft = vim.bo[buf].filetype
      local tiny = (cfg.width or 0) <= 2 and (cfg.height or 0) <= 1
      if not M.skip_ft[ft] and not tiny and not cfg.hide then fade_in(win, M.fast_ft[ft]) end
    end
  end
end

---------------------------------------------------------------------------
-- Explorador lateral: abre e fecha deslizando a largura
---------------------------------------------------------------------------
--
-- O neo-tree mede a largura da janela a cada desenho e, por padrão, ainda a
-- consulta a cada 50 ms para redesenhar quando ela muda. Durante um deslize
-- isso cortava os nomes para a largura do momento ("rea…") e eles só voltavam
-- depois. Aqui:
--   * durante o deslize, o neo-tree "enxerga" a largura final: a árvore é
--     desenhada uma vez, completa, e a janela só revela o que já está pronto;
--   * o polling de 50 ms é desligado (resize_timer_interval = -1) e trocado pelo
--     evento nativo WinResized: só redesenha quando a largura muda de verdade.

M.explorer_duration = 0.16

local sliding = {} ---@type table<integer, integer> janela -> largura interna fixa
local explorer_anim ---@type fun()? animação em curso (cancelável)

---@return integer?
local function explorer_win()
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    local buf = vim.api.nvim_win_get_buf(win)
    if vim.bo[buf].filetype == "neo-tree" and vim.api.nvim_win_get_config(win).relative == "" then return win end
  end
end

local function inner_width(win)
  local info = vim.fn.getwininfo(win)[1]
  return info and (info.width - info.textoff) or vim.api.nvim_win_get_width(win)
end

local patched = false
local function patch_neotree()
  if patched then return end
  local ok, utils = pcall(require, "neo-tree.utils")
  if not ok then return end
  patched = true
  local orig = utils.get_inner_win_width
  utils.get_inner_win_width = function(winid)
    local fixed = winid and sliding[winid]
    if fixed then return fixed end
    return orig(winid)
  end
  -- redesenho por evento (no lugar do polling de 50 ms)
  vim.api.nvim_create_autocmd("WinResized", {
    group = vim.api.nvim_create_augroup("astrograph_neotree_resize", { clear = true }),
    desc = "Redesenhar o explorador quando a largura muda",
    callback = function()
      local wins = vim.v.event.windows or {}
      local ok_m, manager = pcall(require, "neo-tree.sources.manager")
      if not ok_m then return end
      for _, win in ipairs(wins) do
        if vim.api.nvim_win_is_valid(win) and not sliding[win] then
          pcall(manager._for_each_state, nil, function(state)
            if state.winid == win then
              local w = inner_width(win)
              if state.win_width ~= w then
                state.win_width = w
                require("neo-tree.ui.renderer").redraw(state)
              end
            end
          end)
        end
      end
    end,
  })
end

---@param win integer
---@param from integer
---@param to integer
---@param fixed_inner integer largura interna que o neo-tree "enxerga" no deslize
---@param on_done? fun()
local function slide_width(win, from, to, fixed_inner, on_done)
  if explorer_anim then explorer_anim() end
  sliding[win] = fixed_inner
  pcall(vim.api.nvim_win_set_width, win, from)
  explorer_anim = animate(M.explorer_duration, function(x)
    if not vim.api.nvim_win_is_valid(win) then return false end
    local w = math.floor(from + (to - from) * ease_out_cubic(x) + 0.5)
    pcall(vim.api.nvim_win_set_width, win, math.max(1, w))
  end, function()
    sliding[win] = nil
    explorer_anim = nil
    if on_done then on_done() end
  end)
end

---Chamado quando o explorador abre: cresce da borda até a largura dele.
function M.explorer_opened()
  patch_neotree()
  if not enabled() then return end
  local win = explorer_win()
  if not win or vim.w[win].astrograph_slid then return end
  vim.w[win].astrograph_slid = true
  local target = vim.api.nvim_win_get_width(win)
  if target > 2 then slide_width(win, 1, target, inner_width(win)) end
end

---Alterna o explorador; ao fechar, encolhe antes de sumir.
function M.toggle_explorer()
  local win = explorer_win()
  if not win then
    vim.cmd "Neotree show"
    return
  end
  if not enabled() then
    vim.cmd "Neotree close"
    return
  end
  local width = vim.api.nvim_win_get_width(win)
  slide_width(win, width, 1, inner_width(win), function()
    vim.schedule(function() pcall(vim.cmd, "Neotree close") end)
  end)
end

---------------------------------------------------------------------------
-- dd: a linha apagada colapsa dentro dela mesma e some
---------------------------------------------------------------------------

M.delete_duration = 0.16
M.delete_max_lines = 30

local ghost_ns = vim.api.nvim_create_namespace "astrograph_motion_ghost"

-- estágios do colapso: texto original -> metade de baixo -> fina -> mais fina
local STAGES = { false, "▄", "▂", "▁" }

---Destaque (grupo) de cada caractere das linhas, lido da árvore do Treesitter
---que o buffer original já tem pronta (sem novo parse).
---@return (string|false)[][] por linha, um grupo por caractere
local function line_highlights(buf, first, last, lines)
  local out = {}
  for i, line in ipairs(lines) do
    local bytes = {}
    out[i] = { bytes = bytes, line = line }
  end
  local ok, parser = pcall(vim.treesitter.get_parser, buf, nil, { error = false })
  if not ok or not parser then return out end
  local lang = parser:lang()
  local query = vim.treesitter.query.get(lang, "highlights")
  local tree = parser:parse({ first, last + 1 }) and parser:trees()[1]
  if not (query and tree) then return out end
  for id, node in query:iter_captures(tree:root(), buf, first, last + 1) do
    local sr, sc, er, ec = node:range()
    local name = "@" .. query.captures[id] .. "." .. lang
    for row = math.max(sr, first), math.min(er, last) do
      local item = out[row - first + 1]
      if item then
        local from = row == sr and sc or 0
        local to = row == er and ec or #item.line
        for b = from + 1, to do
          item.bytes[b] = name
        end
      end
    end
  end
  return out
end

---Monta a linha do estágio (`glyph` = false mantém o texto) e os trechos de cor.
local function build_frame(hl_lines, glyph, tabstop)
  local text, marks = {}, {}
  for i, item in ipairs(hl_lines) do
    local parts, runs, col = {}, {}, 0
    local b = 1
    local line = item.line
    while b <= #line do
      local ch = vim.fn.strcharpart(line:sub(b), 0, 1)
      if ch == "" then break end
      local hl = item.bytes[b]
      local out
      if ch == "\t" then
        out = string.rep("\u{00A0}", tabstop)
      elseif ch == " " then
        out = "\u{00A0}" -- não transparente: o texto de baixo não vaza pelos espaços
      else
        out = glyph or ch
      end
      parts[#parts + 1] = out
      if hl and ch ~= " " and ch ~= "\t" then
        local last = runs[#runs]
        if last and last.hl == hl and last.e == col then
          last.e = col + #out
        else
          runs[#runs + 1] = { s = col, e = col + #out, hl = hl }
        end
      end
      col = col + #out
      b = b + #ch
    end
    text[i] = table.concat(parts)
    marks[i] = runs
  end
  return text, marks
end

---A linha apagada colapsa verticalmente dentro dela mesma e some.
local function ghost(win, snap)
  local buf = vim.api.nvim_create_buf(false, true)
  local win_h = vim.api.nvim_win_get_height(win)
  local height = math.max(1, math.min(snap.height, win_h - snap.row))
  local ok, ghost_win = pcall(vim.api.nvim_open_win, buf, false, {
    relative = "win",
    win = win,
    row = snap.row,
    col = snap.textoff,
    width = math.max(1, snap.width),
    height = height,
    style = "minimal",
    border = "none", -- sem isso herda o `winborder` global (arredondado)
    focusable = false,
    noautocmd = true,
    zindex = 40,
  })
  if not ok then
    pcall(vim.api.nvim_buf_delete, buf, { force = true })
    return
  end
  vim.w[ghost_win].astrograph_motion = true -- o fade de entrada não se aplica
  vim.wo[ghost_win].winhighlight = "Normal:Normal,NormalFloat:Normal"
  vim.wo[ghost_win].wrap = snap.wrap
  vim.wo[ghost_win].list = false
  if snap.leftcol > 0 then
    vim.api.nvim_win_call(ghost_win, function() vim.fn.winrestview { leftcol = snap.leftcol } end)
  end

  local stage = 0
  local function show(n)
    if n == stage then return end
    stage = n
    local text, marks = build_frame(snap.hl, STAGES[n], snap.tabstop)
    vim.api.nvim_buf_set_lines(buf, 0, -1, false, text)
    vim.api.nvim_buf_clear_namespace(buf, ghost_ns, 0, -1)
    for row, runs in ipairs(marks) do
      for _, r in ipairs(runs) do
        pcall(vim.api.nvim_buf_set_extmark, buf, ghost_ns, row - 1, r.s, { end_col = r.e, hl_group = r.hl })
      end
    end
  end
  show(1)

  animate(M.delete_duration, function(x)
    if not (vim.api.nvim_win_is_valid(ghost_win) and vim.api.nvim_win_is_valid(win)) then return false end
    -- 4 estágios iguais; o fade acelera no fim (snappy, mas sem corte seco)
    show(math.min(#STAGES, math.floor(x * #STAGES) + 1))
    pcall(function() vim.wo[ghost_win].winblend = math.floor(100 * x * x + 0.5) end)
  end, function()
    vim.schedule(function()
      pcall(vim.api.nvim_win_close, ghost_win, true)
      pcall(vim.api.nvim_buf_delete, buf, { force = true })
    end)
  end)
end

---`dd` com animação: apaga na hora (o conteúdo nunca espera a animação) e a
---linha apagada colapsa dentro dela mesma. Mantém contagem e registrador.
function M.delete_lines()
  local count, reg = vim.v.count1, vim.v.register
  local win = vim.api.nvim_get_current_win()
  local lnum = vim.fn.line "."
  local snap
  if
    enabled()
    and count <= M.delete_max_lines
    and vim.bo.buftype == ""
    and vim.fn.foldclosed(lnum) == -1
    and vim.api.nvim_win_get_config(win).relative == ""
  then
    local last = math.min(lnum + count - 1, vim.fn.line "$")
    local info = vim.fn.getwininfo(win)[1]
    local ok_h, text_h = pcall(vim.api.nvim_win_text_height, win, { start_row = lnum - 1, end_row = last - 1 })
    local lines = vim.api.nvim_buf_get_lines(0, lnum - 1, last, false)
    snap = {
      hl = line_highlights(0, lnum - 1, last - 1, lines),
      row = vim.fn.winline() - 1,
      textoff = info.textoff,
      width = info.width - info.textoff,
      height = ok_h and text_h.all or (last - lnum + 1),
      wrap = vim.wo[win].wrap,
      tabstop = vim.bo.tabstop,
      leftcol = vim.fn.winsaveview().leftcol,
    }
  end
  vim.cmd(('normal! "%s%ddd'):format(reg, count))
  if snap then ghost(win, snap) end
end

function M.setup()
  local group = vim.api.nvim_create_augroup("astrograph_motion", { clear = true })
  vim.api.nvim_create_autocmd("FileType", {
    group = group,
    pattern = "neo-tree",
    desc = "Explorador abre deslizando a largura",
    callback = function() vim.schedule(M.explorer_opened) end,
  })
  local pending = false
  vim.api.nvim_create_autocmd("WinNew", {
    group = group,
    desc = "Fade de entrada das janelas flutuantes",
    callback = function()
      if pending then return end
      pending = true
      -- no tick seguinte a janela já tem buffer, filetype e winblend definidos
      vim.schedule(function()
        pending = false
        scan()
      end)
    end,
  })
end

return M
