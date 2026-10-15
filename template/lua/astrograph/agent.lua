-- Agente nativo do Astrograph: o Neovim conversa com o agente atual (o mesmo
-- de <Leader>Ag) pelo `astrograph-agent` (Rust, protocolo ACP) e mostra o
-- trabalho dele enquanto acontece:
--
--   * segue o agente: o arquivo que ele lê ou edita abre na janela de código,
--     na linha certa, sem tirar o cursor de onde você está;
--   * antes de editar, o diff proposto aparece dentro do próprio arquivo
--     (removido em vermelho, novo em verde) e você aprova ou recusa ali;
--   * depois de editar, o buffer recarrega e as linhas alteradas ficam
--     destacadas até o próximo pedido (`u` desfaz como qualquer edição).
--
--   <Leader>Ae  pedir ao agente (na seleção: manda o trecho junto)
--   <Leader>Ao  painel do agente (abrir/fechar)
--   <Leader>Ak  interromper
--   <Leader>Aw  modo (Manual, Aceitar edições, Plano, Auto)
--   <Leader>Av  seguir o agente (liga/desliga)

local M = {}

local ns_preview = vim.api.nvim_create_namespace "astrograph_agent_preview"
local ns_changes = vim.api.nvim_create_namespace "astrograph_agent_changes"

local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Agente" }) end
local function agents() return require "astrograph.agents" end

M.follow = true

local st = {
  job = nil, ---@type integer?
  agent = nil, ---@type AstrographAgent?
  ready = false,
  busy = false,
  mode = nil, ---@type string?
  modes = {}, ---@type {id:string, name:string}[]
  queue = {}, ---@type table[] pedidos esperando a sessão abrir
  buf = nil, ---@type integer?
  last = nil, ---@type string? tipo da última coisa escrita no painel
  tools = {}, ---@type table<string, integer> ferramenta -> linha no painel (0-based)
  before = {}, ---@type table<string, string[]> conteúdo antes da edição, por arquivo
  perms = {}, ---@type table[] permissões na fila (uma na tela por vez)
  asking = false,
  code_win = nil, ---@type integer?
}

---------------------------------------------------------------------------
-- Binário
---------------------------------------------------------------------------

local function binary()
  local name = vim.fn.has "win32" == 1 and "astrograph-agent.exe" or "astrograph-agent"
  -- a config é uma junction para <instalação>/template; o binário fica em <instalação>/bin
  local cfg = vim.uv.fs_realpath(vim.fn.stdpath "config") or vim.fn.stdpath "config"
  local root = vim.fs.dirname(cfg)
  for _, p in ipairs {
    vim.fs.joinpath(root, "bin", name),
    vim.fs.joinpath(root, "prototype", "astrograph-agent", "target", "release", name),
  } do
    if vim.uv.fs_stat(p) then return p end
  end
  local p = vim.fn.exepath "astrograph-agent"
  return p ~= "" and p or nil
end

---------------------------------------------------------------------------
-- Painel
---------------------------------------------------------------------------

local function panel_win()
  if not (st.buf and vim.api.nvim_buf_is_valid(st.buf)) then return nil end
  for _, w in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if vim.api.nvim_win_get_buf(w) == st.buf then return w end
  end
end

local function ensure_buf()
  if st.buf and vim.api.nvim_buf_is_valid(st.buf) then return st.buf end
  st.buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_name(st.buf, "Astrograph://agente")
  vim.bo[st.buf].filetype = "markdown"
  vim.bo[st.buf].modifiable = false
  local map = function(lhs, fn, desc) vim.keymap.set("n", lhs, fn, { buffer = st.buf, desc = desc }) end
  map("q", function() M.toggle() end, "Fechar o painel")
  map("i", function() M.ask() end, "Pedir ao agente")
  map("<CR>", function() M.ask() end, "Pedir ao agente")
  map("<C-c>", function() M.cancel() end, "Interromper")
  return st.buf
end

local function scroll_panel()
  local w = panel_win()
  if w and vim.api.nvim_get_current_win() ~= w then
    vim.api.nvim_win_set_cursor(w, { vim.api.nvim_buf_line_count(st.buf), 0 })
  end
end

---Escreve no fim do painel. `continue` emenda na última linha (texto em fluxo).
local function write(text, kind, continue)
  local buf = ensure_buf()
  local lines = vim.split(text, "\n", { plain = true })
  vim.bo[buf].modifiable = true
  local n = vim.api.nvim_buf_line_count(buf)
  local empty = n == 1 and vim.api.nvim_buf_get_lines(buf, 0, 1, false)[1] == ""
  if continue and not empty then
    local last = vim.api.nvim_buf_get_lines(buf, n - 1, n, false)[1]
    lines[1] = last .. lines[1]
    vim.api.nvim_buf_set_lines(buf, n - 1, n, false, lines)
  else
    if not empty and kind ~= "tool" and st.last ~= kind then table.insert(lines, 1, "") end
    vim.api.nvim_buf_set_lines(buf, empty and 0 or n, empty and 1 or n, false, lines)
  end
  vim.bo[buf].modifiable = false
  st.last = kind
  scroll_panel()
  return vim.api.nvim_buf_line_count(buf) - 1
end

function M.open()
  local buf = ensure_buf()
  if panel_win() then return end
  local cur = vim.api.nvim_get_current_win()
  vim.cmd("botright vsplit")
  local w = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_buf(w, buf)
  vim.api.nvim_win_set_width(w, math.max(50, math.floor(vim.o.columns * 0.33)))
  vim.wo[w].wrap, vim.wo[w].linebreak = true, true
  vim.wo[w].number, vim.wo[w].relativenumber, vim.wo[w].signcolumn = false, false, "no"
  vim.wo[w].winfixwidth = true
  if vim.api.nvim_win_is_valid(cur) then vim.api.nvim_set_current_win(cur) end
  scroll_panel()
end

function M.toggle()
  local w = panel_win()
  if w and #vim.api.nvim_tabpage_list_wins(0) > 1 then return vim.api.nvim_win_close(w, false) end
  M.open()
end

---------------------------------------------------------------------------
-- Janela de código (onde o agente é seguido)
---------------------------------------------------------------------------

local function is_code_win(w)
  if not (w and vim.api.nvim_win_is_valid(w)) then return false end
  if vim.api.nvim_win_get_config(w).relative ~= "" then return false end
  local b = vim.api.nvim_win_get_buf(w)
  return b ~= st.buf and vim.bo[b].buftype == ""
end

local function code_win()
  if is_code_win(st.code_win) then return st.code_win end
  for _, w in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if is_code_win(w) then
      st.code_win = w
      return w
    end
  end
end

local function same_file(a, b)
  -- realpath: no Windows o mesmo arquivo chega como GABRIE~1 (8.3) ou nome longo
  local norm = function(p)
    p = vim.fn.fnamemodify(p, ":p")
    return (vim.fs.normalize(vim.uv.fs_realpath(p) or p):lower())
  end
  return norm(a) == norm(b)
end

local function buf_for(path)
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    local name = vim.api.nvim_buf_get_name(b)
    if name ~= "" and vim.api.nvim_buf_is_loaded(b) and same_file(name, path) then return b end
  end
end

---Mostra `path` na janela de código, na linha `line`, sem mudar o foco.
---@return integer? buf
local function show(path, line)
  local w = code_win()
  if not w then return end
  local ok, buf = pcall(vim.api.nvim_win_call, w, function()
    if not same_file(vim.api.nvim_buf_get_name(0), path) then
      local b = buf_for(path)
      if b then
        vim.api.nvim_win_set_buf(0, b)
      else
        vim.cmd.edit(vim.fn.fnameescape(path))
      end
    end
    if line and line > 0 then
      pcall(vim.api.nvim_win_set_cursor, 0, { math.min(line, vim.api.nvim_buf_line_count(0)), 0 })
      vim.cmd "normal! zz"
    end
    return vim.api.nvim_get_current_buf()
  end)
  return ok and buf or nil
end

local function read_lines(path)
  local b = buf_for(path)
  if b then return vim.api.nvim_buf_get_lines(b, 0, -1, false) end
  local ok, lines = pcall(vim.fn.readfile, path)
  return ok and lines or {}
end

---------------------------------------------------------------------------
-- Prévia do diff (antes) e destaque das alterações (depois)
---------------------------------------------------------------------------

local function clear_preview()
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(b) then vim.api.nvim_buf_clear_namespace(b, ns_preview, 0, -1) end
  end
end

---Marca no arquivo o trecho que sai (vermelho) e mostra o que entra (verde).
local function preview(diff)
  local path, old, new = diff.path, diff.old, diff.new or ""
  if type(path) ~= "string" then return end
  st.before[path] = st.before[path] or read_lines(path)
  local buf = show(path)
  if not buf then return end
  local new_lines = vim.split(new, "\n", { plain = true })
  local virt = vim.tbl_map(function(l) return { { "+ " .. l, "DiffAdd" } } end, new_lines)
  if type(old) ~= "string" or old == "" then
    -- arquivo novo ou reescrito por inteiro: mostra o conteúdo no topo
    vim.api.nvim_buf_set_extmark(buf, ns_preview, 0, 0, { virt_lines = virt, virt_lines_above = true })
    return
  end
  local text = table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n")
  local s = text:find(old, 1, true)
  if not s then return end
  local first = select(2, text:sub(1, s - 1):gsub("\n", "")) -- linha (0-based) onde começa
  local count = select(2, old:gsub("\n", "")) + 1
  local last = first + count - 1
  vim.api.nvim_buf_set_extmark(buf, ns_preview, last, 0, { virt_lines = virt })
  for l = first, last do
    vim.api.nvim_buf_set_extmark(buf, ns_preview, l, 0, { line_hl_group = "DiffDelete" })
  end
  local w = code_win()
  if w then pcall(vim.api.nvim_win_set_cursor, w, { first + 1, 0 }) end
end

---Recarrega o arquivo editado e destaca as linhas novas/alteradas.
local function on_edited(path)
  local before = st.before[path]
  st.before[path] = nil
  local buf = buf_for(path)
  if buf then
    if vim.bo[buf].modified then
      notify(("%s tem alterações suas não salvas; o agente editou o arquivo no disco. Use :e! para ver a versão dele."):format(vim.fn.fnamemodify(path, ":~:.")), vim.log.levels.WARN)
      return
    end
    vim.api.nvim_buf_call(buf, function() vim.cmd "checktime" end)
  elseif M.follow then
    buf = show(path)
  end
  if not buf then return end
  local after = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  if not before then return end
  local hunks = vim.diff(table.concat(before, "\n") .. "\n", table.concat(after, "\n") .. "\n", { result_type = "indices" })
  local first
  for _, h in ipairs(hunks or {}) do
    local start_b, count_b = h[3], h[4]
    for l = start_b, start_b + count_b - 1 do
      pcall(vim.api.nvim_buf_set_extmark, buf, ns_changes, l - 1, 0, { line_hl_group = "DiffAdd", sign_text = "▎", sign_hl_group = "DiffAdd" })
    end
    first = first or start_b
  end
  if first and M.follow then show(path, first) end
end

local function clear_changes()
  for _, b in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(b) then vim.api.nvim_buf_clear_namespace(b, ns_changes, 0, -1) end
  end
end

---------------------------------------------------------------------------
-- Eventos do astrograph-agent
---------------------------------------------------------------------------

local function send(op)
  if st.job then pcall(vim.fn.chansend, st.job, vim.json.encode(op) .. "\n") end
end

local icons = { pending = "◌", in_progress = "◐", completed = "✓", failed = "✗" }

local function rel(p) return vim.fn.fnamemodify(p, ":~:.") end

local function tool_line(ev)
  local t = ev.title ~= vim.NIL and ev.title or "ferramenta"
  return ("  %s %s"):format(icons[ev.status] or "•", t)
end

local next_permission

local handlers = {}

function handlers.ready(ev)
  st.ready = true
  st.mode = ev.mode ~= vim.NIL and ev.mode or nil
  st.modes = type(ev.modes) == "table" and ev.modes or {}
  write(("_%s pronto · modo %s_"):format(st.agent.label, st.mode or "?"), "info")
  local q = st.queue
  st.queue = {}
  for _, op in ipairs(q) do M.submit(op) end
end

function handlers.text(ev)
  if ev.kind ~= "agent" then return end
  write(ev.text, "agent", st.last == "agent")
end

---Troca a linha da ferramenta no painel (ou cria). O painel só cresce, então
---o número da linha guardado continua valendo.
local function set_tool_line(id, text)
  -- títulos podem trazer quebras (ex.: comando de terminal com várias linhas):
  -- a linha da ferramenta precisa ser uma só, para ser trocada no lugar
  text = text:gsub("%s+$", ""):gsub("%s*[\r\n]+%s*", " ⏎ ")
  if vim.fn.strchars(text) > 120 then text = vim.fn.strcharpart(text, 0, 119) .. "…" end
  local buf = ensure_buf()
  local row = id and st.tools[id]
  if row and row < vim.api.nvim_buf_line_count(buf) then
    vim.bo[buf].modifiable = true
    vim.api.nvim_buf_set_lines(buf, row, row + 1, false, { text })
    vim.bo[buf].modifiable = false
  else
    row = write(text, "tool")
    if id then st.tools[id] = row end
  end
end

function handlers.tool(ev)
  set_tool_line(ev.id, tool_line(ev))
  local loc = type(ev.locations) == "table" and ev.locations[1] or nil
  if type(loc) == "table" and type(loc.path) == "string" then
    if ev.kind == "edit" and not st.before[loc.path] then st.before[loc.path] = read_lines(loc.path) end
    if M.follow and ev.status ~= "completed" then show(loc.path, loc.line ~= vim.NIL and loc.line or nil) end
  end
end

function handlers.permission(ev)
  table.insert(st.perms, ev)
  next_permission()
end

function handlers.edited(ev)
  on_edited(ev.path)
  write(("  ✎ %s"):format(rel(ev.path)), "tool")
end

function handlers.plan(ev)
  if type(ev.entries) ~= "table" then return end
  local lines = { "**Plano**" }
  for _, e in ipairs(ev.entries) do
    lines[#lines + 1] = ("- [%s] %s"):format(e.status == "completed" and "x" or " ", e.content or "")
  end
  write(table.concat(lines, "\n"), "plan")
end

function handlers.mode(ev)
  st.mode = ev.mode
  write(("_modo: %s_"):format(ev.mode), "info")
end

function handlers.done(ev)
  st.busy = false
  st.perms, st.asking = {}, false
  clear_preview()
  write(("_— fim (%s)_"):format(ev.stop ~= vim.NIL and ev.stop or "?"), "info")
  if not panel_win() then notify "O agente terminou." end
  vim.cmd.redrawstatus()
end

function handlers.error(ev)
  write(("**Erro:** %s"):format(ev.message), "error")
  notify(ev.message, vim.log.levels.ERROR)
  if ev.fatal then
    st.busy = false
  end
end

next_permission = function()
  if st.asking or #st.perms == 0 then return end
  local ev = table.remove(st.perms, 1)
  st.asking = true
  local title = type(ev.title) == "string" and ev.title or "ação"
  -- a parte visual é opcional: se falhar, a pergunta tem que aparecer mesmo
  -- assim (senão o agente fica esperando uma resposta que nunca vem)
  local ok, err = pcall(function()
    local diffs = type(ev.diffs) == "table" and ev.diffs or {}
    for _, d in ipairs(diffs) do
      preview(d)
    end
    if #diffs == 0 and M.follow and type(ev.locations) == "table" and ev.locations[1] then
      local l = ev.locations[1]
      if type(l.path) == "string" then show(l.path, l.line ~= vim.NIL and l.line or nil) end
    end
    set_tool_line(ev.tool ~= vim.NIL and ev.tool or nil, ("  ? %s — aguardando você"):format(title))
  end)
  if not ok then pcall(write, ("**Erro interno:** %s"):format(err), "error") end
  local opts = type(ev.options) == "table" and ev.options or {}
  title = vim.trim(title):gsub("%s*[\r\n]+%s*", " ⏎ ")
  vim.ui.select(opts, {
    prompt = ("O agente quer: %s"):format(title),
    format_item = function(o) return o.name end,
  }, function(choice)
    clear_preview()
    send { op = "permission", id = ev.id, option = choice and choice.optionId or vim.NIL }
    st.asking = false
    next_permission()
  end)
end

local function on_line(line)
  if line == "" then return end
  local ok, ev = pcall(vim.json.decode, line)
  if not ok or type(ev) ~= "table" then return end
  local h = handlers[ev.ev]
  if h then
    local okh, err = pcall(h, ev)
    if not okh then write(("**Erro interno:** %s"):format(err), "error") end
  end
end

---------------------------------------------------------------------------
-- Processo
---------------------------------------------------------------------------

local function start()
  if st.job then return true end
  local bin = binary()
  if not bin then
    notify("astrograph-agent não encontrado. Rode o instalador do Astrograph de novo.", vim.log.levels.ERROR)
    return false
  end
  local a = agents().current()
  if not (a and agents().has_acp(a)) then
    notify("Nenhum agente com ACP instalado. Rode :Astrograph configurar.", vim.log.levels.WARN)
    return false
  end
  st.agent, st.ready = a, false
  local cmd = { bin, "--" }
  vim.list_extend(cmd, agents().acp_command(a))
  local partial = ""
  st.job = vim.fn.jobstart(cmd, {
    cwd = agents().root(),
    on_stdout = function(_, data)
      -- linhas podem chegar partidas entre dois callbacks
      data[1] = partial .. data[1]
      partial = table.remove(data)
      for _, l in ipairs(data) do on_line(l) end
    end,
    on_exit = function(_, code)
      st.job, st.ready, st.busy = nil, false, false
      if code ~= 0 then write(("_agente encerrou (código %d)_"):format(code), "info") end
    end,
  })
  if st.job <= 0 then
    st.job = nil
    notify("Não consegui iniciar o astrograph-agent.", vim.log.levels.ERROR)
    return false
  end
  write(("_iniciando %s…_"):format(a.label), "info")
  return true
end

function M.stop()
  if st.job then pcall(vim.fn.jobstop, st.job) end
  st.job, st.ready, st.busy = nil, false, false
end

---Manda um pedido (abre a sessão se preciso).
---@param op {op:"prompt", text:string, files?:string[], snippets?:table[]}
function M.submit(op)
  if not start() then return end
  if not st.ready then return table.insert(st.queue, op) end
  clear_changes()
  st.busy = true
  st.last = nil
  write("**Você:** " .. op.text, "user")
  send(op)
  vim.cmd.redrawstatus()
end

---Pergunta o que pedir; com seleção, manda o trecho junto; senão, o arquivo atual.
function M.ask()
  local cur = vim.api.nvim_get_current_buf()
  local name = vim.api.nvim_buf_get_name(cur)
  local is_file = cur ~= st.buf and name ~= "" and vim.bo[cur].buftype == ""
  local snippets, files = {}, {}
  local mode = vim.fn.mode()
  if is_file and mode:match "[vV\22]" then
    local a, b = vim.fn.line "v", vim.fn.line "."
    if a > b then a, b = b, a end
    vim.api.nvim_feedkeys(vim.keycode "<Esc>", "nx", false)
    snippets[1] = {
      path = vim.fn.fnamemodify(name, ":."),
      start = a,
      ["end"] = b,
      text = table.concat(vim.api.nvim_buf_get_lines(cur, a - 1, b, false), "\n"),
    }
  elseif is_file then
    files[1] = name
  end
  if is_file then st.code_win = vim.api.nvim_get_current_win() end
  M.open()
  vim.ui.input({ prompt = "Pedir ao agente: " }, function(text)
    if not text or vim.trim(text) == "" then return end
    M.submit { op = "prompt", text = text, files = files, snippets = snippets }
  end)
end

function M.cancel()
  if not st.busy then return notify "O agente não está trabalhando." end
  send { op = "cancel" }
  st.perms, st.asking = {}, false
  clear_preview()
  write("_interrompido_", "info")
end

function M.pick_mode()
  if not st.ready then return notify "Abra uma conversa com o agente primeiro (<Leader>Ae)." end
  vim.ui.select(st.modes, {
    prompt = "Modo do agente",
    format_item = function(m) return (m.id == st.mode and "● " or "  ") .. (m.name or m.id) end,
  }, function(m)
    if m then send { op = "mode", mode = m.id } end
  end)
end

function M.toggle_follow()
  M.follow = not M.follow
  notify(M.follow and "Seguindo o agente." or "Não sigo mais o agente.")
end

---Texto curto para a barra de status ("" quando parado).
function M.status()
  if not st.job then return "" end
  if #st.perms > 0 or st.asking then return "󰋗 agente aguarda você" end
  return st.busy and "󰚩 agente trabalhando" or ""
end

function M.setup()
  local group = vim.api.nvim_create_augroup("astrograph_agent", { clear = true })
  vim.api.nvim_create_autocmd("WinEnter", {
    group = group,
    callback = function()
      local w = vim.api.nvim_get_current_win()
      if is_code_win(w) then st.code_win = w end
    end,
  })
  vim.api.nvim_create_autocmd("VimLeavePre", { group = group, callback = M.stop })
  vim.api.nvim_create_user_command("AstrographAgent", function(o)
    if o.args ~= "" then
      M.open()
      M.submit { op = "prompt", text = o.args }
    else
      M.ask()
    end
  end, { nargs = "*", desc = "Pedir ao agente nativo do Astrograph" })
end

-- usado pelos testes
M._state = st
M._on_line = on_line

return M
