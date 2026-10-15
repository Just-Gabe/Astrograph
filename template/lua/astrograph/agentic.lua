-- Ações agênticas do Astrograph: o que você pede ao agente atual (o mesmo do
-- terminal, do chat e da edição inline, escolhido em <Leader>Ag).
--
--   <Leader>Ax  corrigir o problema da linha (diagnóstico)
--   <Leader>Ai  explicar a seleção (ou o arquivo)
--   <Leader>Ar  revisar as alterações ainda não commitadas
--   <Leader>Am  escrever a mensagem de commit
--   <Leader>Ad  ver as alterações (do agente ou suas), trecho a trecho
--   <Leader>A.  mencionar o arquivo/seleção no terminal do agente
--
-- E o que deixa o modo agentic confiável:
--   * login checado em segundo plano: abrir o chat com um agente sem login
--     oferece entrar, trocar para um agente logado ou seguir assim mesmo, em vez
--     de uma conversa que nunca responde;
--   * aviso quando o agente passa de 40 s sem responder;
--   * indicador na barra de status enquanto o agente trabalha.

local M = {}

local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Astrograph" }) end

local function agents() return require "astrograph.agents" end

---------------------------------------------------------------------------
-- Login (cacheado, checado fora do caminho da tecla)
---------------------------------------------------------------------------

---@type table<string, boolean?>
M.login = {}
local checking = false

---Atualiza o login de todos os agentes em segundo plano (os CLIs levam ~1 s).
---@param cb? fun()
function M.refresh_login(cb)
  if checking then
    if cb then vim.defer_fn(function() M.refresh_login(cb) end, 200) end
    return
  end
  checking = true
  agents().status(function(list)
    checking = false
    for _, s in ipairs(list) do
      M.login[s.agent.bin] = s.login
    end
    if cb then cb() end
  end)
end

---Garante que o agente atual vai responder antes de mandar algo para ele.
---Sem login, oferece entrar, usar outro agente já logado ou seguir mesmo assim.
---@param cb fun(a: AstrographAgent)
function M.ensure_ready(cb)
  local a = agents().current()
  if not a then
    notify("Nenhum agente instalado. Rode :Astrograph configurar.", vim.log.levels.WARN)
    return
  end
  -- sem resposta no cache ainda: segue (o terminal/chat mostra o erro do proprio
  -- agente) e checa para a proxima vez
  if M.login[a.bin] ~= false then
    if M.login[a.bin] == nil then M.refresh_login() end
    return cb(a)
  end
  local choices = { { label = "Entrar no " .. a.label, act = "login" } }
  for _, other in ipairs(agents().available()) do
    if other.bin ~= a.bin and M.login[other.bin] then
      choices[#choices + 1] = { label = "Usar o " .. other.label .. " (já logado)", act = "use", agent = other }
    end
  end
  choices[#choices + 1] = { label = "Continuar assim mesmo", act = "go" }
  vim.ui.select(choices, {
    prompt = a.label .. " está sem login e não vai responder",
    format_item = function(c) return c.label end,
  }, function(c)
    if not c then return end
    if c.act == "login" then
      require("astrograph.commands").login(a.bin)
    elseif c.act == "use" then
      agents().set_current(c.agent.bin)
      cb(c.agent)
    else
      cb(a)
    end
  end)
end

---------------------------------------------------------------------------
-- Conversas prontas (chat do agente atual, já enviadas)
---------------------------------------------------------------------------

---Abre um chat novo com o agente atual e envia `prompt`.
---@param prompt string
function M.ask(prompt)
  M.ensure_ready(function()
    local ok, err = pcall(function()
      require("codecompanion").chat {
        messages = { { role = "user", content = prompt } },
        auto_submit = true,
      }
    end)
    if not ok then notify("Não consegui abrir o chat: " .. tostring(err), vim.log.levels.ERROR) end
  end)
end

---Caminho do buffer relativo ao projeto (como o agente enxerga).
---@param buf? integer
---@return string
local function relpath(buf)
  local name = vim.api.nvim_buf_get_name(buf or 0)
  if name == "" then return "[sem nome]" end
  local root = agents().root()
  local rel = vim.fs.relpath and vim.fs.relpath(root, name) or nil
  return (rel or vim.fn.fnamemodify(name, ":.")):gsub("\\", "/")
end

---Linhas `[a, b]` (1-based) do buffer atual como bloco de código.
local function code_block(a, b)
  local lines = vim.api.nvim_buf_get_lines(0, a - 1, b, false)
  return ("```%s\n%s\n```"):format(vim.bo.filetype, table.concat(lines, "\n"))
end

---Seleção visual atual (ou `nil` fora do modo visual).
---@return integer?, integer?
local function visual_range()
  local mode = vim.fn.mode()
  if not mode:match "[vV\22]" then return nil end
  local a, b = vim.fn.line "v", vim.fn.line "."
  if a > b then a, b = b, a end
  vim.api.nvim_feedkeys(vim.keycode "<Esc>", "nx", false)
  return a, b
end

---Corrige o problema (diagnóstico) da linha do cursor.
function M.fix()
  local lnum = vim.fn.line "." - 1
  local diags = vim.diagnostic.get(0, { lnum = lnum })
  if #diags == 0 then
    notify "Nenhum problema nesta linha (os diagnósticos do LSP aparecem como sublinhado)."
    return
  end
  table.sort(diags, function(x, y) return x.severity < y.severity end)
  local d = diags[1]
  local first = math.max(1, d.lnum + 1 - 12)
  local last = math.min(vim.api.nvim_buf_line_count(0), (d.end_lnum or d.lnum) + 1 + 12)
  local problems = {}
  for _, x in ipairs(diags) do
    problems[#problems + 1] = ("- %s%s"):format(x.message:gsub("\n", " "), x.source and (" (" .. x.source .. ")") or "")
  end
  M.ask(table.concat({
    ("Corrija o problema em `%s`, linha %d:"):format(relpath(), d.lnum + 1),
    "",
    table.concat(problems, "\n"),
    "",
    ("Trecho (linhas %d–%d):"):format(first, last),
    code_block(first, last),
    "",
    "Explique a causa em uma frase e aplique a correção no arquivo.",
  }, "\n"))
end

---Explica a seleção (modo visual) ou o arquivo inteiro.
function M.explain()
  local a, b = visual_range()
  local where
  if a then
    where = ("o trecho de `%s` (linhas %d–%d):\n%s"):format(relpath(), a, b, code_block(a, b))
  else
    where = ("o arquivo `%s`."):format(relpath())
  end
  M.ask("Explique " .. where .. "\n\nSeja direto: o que faz, como e por quê. Em português.")
end

---Diff do repositório (preparado, senão o que ainda não foi preparado).
---@return string? diff, string? label
local function repo_diff()
  local root = agents().root()
  local function git(args)
    local cmd = { "git", "-C", root }
    vim.list_extend(cmd, args)
    local ok, res = pcall(function() return vim.system(cmd, { text = true }):wait(5000) end)
    if ok and res and res.code == 0 then return res.stdout or "" end
    return nil
  end
  local staged = git { "diff", "--cached", "--stat", "--patch", "--no-color" }
  if staged == nil then return nil, nil end
  if staged ~= "" then return staged, "preparadas (git add)" end
  local work = git { "diff", "--stat", "--patch", "--no-color" }
  if work and work ~= "" then return work, "ainda não preparadas" end
  return "", nil
end

local MAX_DIFF = 60000 -- caracteres: diffs enormes viram resumo

---@param diff string
local function clip(diff)
  if #diff <= MAX_DIFF then return diff end
  return diff:sub(1, MAX_DIFF) .. "\n\n[diff cortado: mostre só o que der para avaliar]"
end

---Pede ao agente uma revisão das alterações não commitadas.
function M.review()
  local diff, label = repo_diff()
  if diff == nil then return notify("Esta pasta não é um repositório git.", vim.log.levels.WARN) end
  if diff == "" then return notify "Nada para revisar: não há alterações." end
  M.ask(table.concat({
    ("Revise as alterações %s abaixo como um revisor experiente."):format(label),
    "Aponte bugs, riscos e casos esquecidos, do mais grave ao menos grave, com arquivo e linha.",
    "Se estiver tudo certo, diga isso em uma frase. Não reescreva o código inteiro.",
    "",
    "```diff",
    clip(diff),
    "```",
  }, "\n"))
end

---Pede a mensagem de commit das alterações.
function M.commit_message()
  local diff, label = repo_diff()
  if diff == nil then return notify("Esta pasta não é um repositório git.", vim.log.levels.WARN) end
  if diff == "" then return notify "Nada para commitar: não há alterações." end
  M.ask(table.concat({
    ("Escreva a mensagem de commit para as alterações %s abaixo."):format(label),
    "Formato: conventional commits em português (ex.: `feat: ...`, `fix: ...`), título com até 72",
    "caracteres e, se ajudar, um corpo curto com o porquê. Responda só com a mensagem, num bloco de código.",
    "",
    "```diff",
    clip(diff),
    "```",
  }, "\n"))
end

---Lista as alterações (do agente ou suas) trecho a trecho, com prévia do diff.
function M.changes()
  local diff = repo_diff()
  if diff == nil then
    -- fora do git: os arquivos que o agente do chat editou nesta sessão
    local ok = pcall(function() require("codecompanion").changes() end)
    if not ok then notify("Esta pasta não é um repositório git.", vim.log.levels.WARN) end
    return
  end
  if diff == "" then return notify "Nenhuma alteração desde o último commit." end
  require("snacks").picker.git_diff { title = "Alterações (Enter abre o trecho)" }
end

---------------------------------------------------------------------------
-- Terminal do agente
---------------------------------------------------------------------------

---Texto que menciona o arquivo (e as linhas da seleção) para o agente:
---`@caminho` ou `@caminho#L10-20` (Claude Code, opencode e Codex entendem).
---@return string?
function M.mention()
  local buf = vim.api.nvim_get_current_buf()
  if vim.bo[buf].buftype ~= "" then
    -- no próprio terminal: o arquivo de onde você veio
    local alt = vim.fn.bufnr "#"
    if alt <= 0 or vim.bo[alt].buftype ~= "" then return nil end
    buf = alt
  end
  local ref = "@" .. relpath(buf)
  local a, b = visual_range()
  if a then ref = a == b and ("%s#L%d"):format(ref, a) or ("%s#L%d-%d"):format(ref, a, b) end
  return ref .. " "
end

---------------------------------------------------------------------------
-- Pontes ACP órfãs (Windows)
---------------------------------------------------------------------------
--
-- No Windows os filhos não morrem com o pai: se o Neovim é encerrado de forma
-- abrupta (terminal fechado, travamento), a ponte ACP do chat (node, ~150 MB com
-- o CLI do agente) fica rodando para sempre, e isso acumula a cada vez. Aqui
-- encerramos só pontes cujo processo-pai não existe mais: as de um Neovim (ou
-- outro editor) aberto têm pai vivo e não são tocadas.

local ORPHAN_SCRIPT = [[
$bridges = Get-CimInstance Win32_Process -Filter "Name='node.exe'" |
  Where-Object { $_.CommandLine -match 'claude-agent-acp|codex-acp' }
$n = 0
foreach ($p in $bridges) {
  if (-not (Get-Process -Id $p.ParentProcessId -ErrorAction SilentlyContinue)) {
    taskkill /F /T /PID $p.ProcessId *> $null
    $n++
  }
}
Write-Output $n
]]

---Encerra pontes ACP órfãs (só no Windows; assíncrono).
---@param cb? fun(killed: integer)
function M.cleanup_orphans(cb)
  if vim.fn.has "win32" == 0 then return cb and cb(0) end
  local ok = pcall(vim.system, { "powershell", "-NoProfile", "-NonInteractive", "-Command", ORPHAN_SCRIPT }, { text = true }, function(res)
    local n = tonumber(vim.trim(res.stdout or "")) or 0
    if cb then vim.schedule(function() cb(n) end) end
  end)
  if not ok and cb then cb(0) end
end

---------------------------------------------------------------------------
-- Atividade: indicador na barra de status e aviso de demora
---------------------------------------------------------------------------

M.busy = 0
M.busy_label = nil ---@type string?
local SLOW_MS = 40000
local spinner = { "⋯", "·⋯", "··⋯", "···" }
local frame, timer, slow = 1, nil, nil

---Texto do indicador (vazio quando o agente está parado).
---@return string
function M.status()
  if M.busy <= 0 then return "" end
  return ("✦ %s %s"):format(M.busy_label or "Agente", spinner[frame])
end

local function stop_timers()
  if timer then
    timer:stop()
    timer:close()
    timer = nil
  end
  if slow then
    slow:stop()
    slow:close()
    slow = nil
  end
end

function M.setup()
  local group = vim.api.nvim_create_augroup("astrograph_agentic", { clear = true })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionRequestStarted",
    desc = "Indicador do agente trabalhando",
    callback = function(ev)
      M.busy = M.busy + 1
      local adapter = vim.tbl_get(ev, "data", "adapter")
      M.busy_label = adapter and (adapter.formatted_name or adapter.name) or M.busy_label
      if not timer then
        timer = vim.uv.new_timer()
        timer:start(0, 250, vim.schedule_wrap(function()
          frame = frame % #spinner + 1
          vim.cmd.redrawstatus()
        end))
      end
      if slow then slow:stop() else slow = vim.uv.new_timer() end
      slow:start(SLOW_MS, 0, vim.schedule_wrap(function()
        if M.busy > 0 then
          notify(
            ("%s ainda não respondeu (40 s). Se nada aparecer, confira o login em :Astrograph agentes ou troque com <Leader>Ag."):format(
              M.busy_label or "O agente"
            ),
            vim.log.levels.WARN
          )
        end
      end))
    end,
  })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeCompanionRequestFinished",
    callback = function()
      M.busy = math.max(0, M.busy - 1)
      if M.busy == 0 then
        stop_timers()
        vim.cmd.redrawstatus()
      end
    end,
  })
  -- login: checado depois da abertura (sem custo no startup) e ao trocar de agente
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "VeryLazy",
    once = true,
    callback = function()
      vim.defer_fn(function() M.refresh_login() end, 3000)
      vim.defer_fn(function() M.cleanup_orphans() end, 8000)
    end,
  })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "AstrographAgentChanged",
    callback = function() M.refresh_login() end,
  })
end

return M
