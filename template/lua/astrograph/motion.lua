-- Movimento da interface do Astrograph:
--
--   * janelas flutuantes e menus surgem com um fade curto;
--   * o explorador lateral abre e fecha deslizando a largura;
--   * `dd` apaga na hora e a linha apagada desce e some (uma "cópia fantasma").
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
local function ease_in_out_sine(x) return -(math.cos(math.pi * x) - 1) / 2 end

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
  timer:start(0, 16, function()
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

M.explorer_duration = 0.16

---@return integer?
local function explorer_win()
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    local buf = vim.api.nvim_win_get_buf(win)
    if vim.bo[buf].filetype == "neo-tree" and vim.api.nvim_win_get_config(win).relative == "" then return win end
  end
end

---@param win integer
---@param from integer
---@param to integer
---@param on_done? fun()
local function slide_width(win, from, to, on_done)
  pcall(vim.api.nvim_win_set_width, win, from)
  animate(M.explorer_duration, function(x)
    if not vim.api.nvim_win_is_valid(win) then return false end
    local w = math.floor(from + (to - from) * ease_out_cubic(x) + 0.5)
    pcall(vim.api.nvim_win_set_width, win, math.max(1, w))
  end, on_done)
end

---Chamado quando o explorador abre: cresce da borda até a largura dele.
function M.explorer_opened()
  if not enabled() then return end
  local win = explorer_win()
  if not win or vim.w[win].astrograph_slid then return end
  vim.w[win].astrograph_slid = true
  local target = vim.api.nvim_win_get_width(win)
  if target > 2 then slide_width(win, 1, target) end
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
  slide_width(win, width, 1, function()
    vim.schedule(function() pcall(vim.cmd, "Neotree close") end)
  end)
end

---------------------------------------------------------------------------
-- dd: a linha apagada desce e some
---------------------------------------------------------------------------

M.delete_duration = 0.2
M.delete_max_lines = 30

---Desenha a "cópia fantasma" das linhas apagadas e anima: desce 2 linhas
---enquanto some com fade.
local function ghost(win, snap)
  local buf = vim.api.nvim_create_buf(false, true)
  -- com transparência, o Neovim deixa o texto de baixo aparecer pelos espaços
  -- da janela (letras se misturam no fade). Espaço não separável (U+00A0) fica
  -- em branco mas não é transparente.
  local nbsp = "\u{00A0}"
  local tab = string.rep(" ", snap.tabstop)
  local lines = {}
  for i, line in ipairs(snap.lines) do
    lines[i] = (line:gsub("\t", tab):gsub(" ", nbsp))
  end
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  -- destaque de sintaxe direto pelo treesitter (sem disparar LSP/autocmds de filetype)
  local lang = snap.ft ~= "" and vim.treesitter.language.get_lang(snap.ft) or nil
  if lang then pcall(vim.treesitter.start, buf, lang) end
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
  vim.wo[ghost_win].list = false -- não desenhar o U+00A0 como `␣`
  vim.wo[ghost_win].winblend = 0
  if snap.leftcol > 0 then
    vim.api.nvim_win_call(ghost_win, function() vim.fn.winrestview { leftcol = snap.leftcol } end)
  end
  local max_drop = math.max(0, math.min(2, win_h - snap.row - height))
  animate(M.delete_duration, function(x)
    if not (vim.api.nvim_win_is_valid(ghost_win) and vim.api.nvim_win_is_valid(win)) then return false end
    local drop = math.floor(max_drop * ease_in_out_sine(x) + 0.5)
    pcall(vim.api.nvim_win_set_config, ghost_win, { relative = "win", win = win, row = snap.row + drop, col = snap.textoff })
    pcall(function() vim.wo[ghost_win].winblend = math.floor(100 * ease_out_cubic(x) + 0.5) end)
  end, function()
    vim.schedule(function()
      pcall(vim.api.nvim_win_close, ghost_win, true)
      pcall(vim.api.nvim_buf_delete, buf, { force = true })
    end)
  end)
end

---`dd` com animação: apaga na hora (o conteúdo nunca espera a animação) e
---mostra as linhas apagadas descendo e sumindo. Mantém contagem e registrador.
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
    snap = {
      lines = vim.api.nvim_buf_get_lines(0, lnum - 1, last, false),
      row = vim.fn.winline() - 1,
      textoff = info.textoff,
      width = info.width - info.textoff,
      height = ok_h and text_h.all or (last - lnum + 1),
      ft = vim.bo.filetype,
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
