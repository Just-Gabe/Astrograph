-- Launcher de agentes-CLI em terminal flutuante (Snacks).
--
-- Roda o TUI nativo do agente atual (claude/opencode/codex/gemini) no root do
-- projeto. O agente é o mesmo do chat e da edição inline: troque com
-- :Astrograph agente ou <Leader>Ag (nenhum custo de detecção no boot), e abre no
-- modo escolhido em <Leader>Aw (ex.: `claude --permission-mode acceptEdits`).
--
--   <Leader>Al  abrir o agente atual         <Leader>AT  alternar o terminal dele
--   <Leader>A.  mencionar o arquivo atual (ou a seleção) no agente: `@arquivo#L1-9`

local M = {}

local last = { cmd = nil, buf = nil }

local function win_opts() return { position = "float", border = "rounded" } end

---Abre um agente num terminal flutuante, no modo escolhido em <Leader>Aw.
---@param a AstrographAgent
local function open_agent(a)
  local modes = require "astrograph.modes"
  local cmd, exact = modes.terminal_cmd(a)
  modes.warn_emulated(a.label .. " (terminal)", exact)
  local ok, term = pcall(function()
    return require("snacks").terminal.open(cmd, {
      cwd = require("astrograph.agents").root(),
      win = win_opts(),
    })
  end)
  if not ok or not term then
    -- Fallback: terminal nativo do Neovim.
    vim.cmd "botright split | terminal"
    vim.fn.chansend(vim.b.terminal_job_id, cmd .. "\r")
    last.cmd = cmd
    return
  end
  last.cmd = cmd
  last.buf = term.buf
end

---Abre o terminal do agente atual (o escolhido em :Astrograph agente / <Leader>Ag).
function M.launch()
  local a = require("astrograph.agents").current()
  if not a then
    vim.notify("Astrograph: nenhum agente instalado. Rode :Astrograph configurar.", vim.log.levels.WARN)
    return
  end
  open_agent(a)
end

---Alterna o terminal do agente atual (abre se ainda não existir).
function M.toggle()
  local a = require("astrograph.agents").current()
  if not a then return M.launch() end
  -- outro agente ou outro modo: terminal novo (o modo vale da abertura do CLI)
  local cmd = require("astrograph.modes").terminal_cmd(a)
  if last.cmd ~= cmd then return M.launch() end
  pcall(function()
    require("snacks").terminal.toggle(cmd, { cwd = require("astrograph.agents").root(), win = win_opts() })
  end)
end

---Menciona o arquivo atual (ou as linhas selecionadas) no terminal do agente,
---como `@caminho#L10-20`. Abre o terminal se ele ainda não existir.
function M.send()
  local ref = require("astrograph.agentic").mention()
  if not ref then
    vim.notify("Astrograph: abra um arquivo para mencioná-lo ao agente", vim.log.levels.WARN)
    return
  end
  local function deliver(tries)
    local buf = last.buf
    local chan = buf and vim.api.nvim_buf_is_valid(buf) and vim.b[buf].terminal_job_id
    if chan then
      vim.api.nvim_chan_send(chan, ref)
      return
    end
    if tries > 0 then vim.defer_fn(function() deliver(tries - 1) end, 150) end
  end
  if not (last.buf and vim.api.nvim_buf_is_valid(last.buf)) then
    M.launch()
    -- o TUI do agente leva um instante para aceitar texto
    vim.defer_fn(function() deliver(20) end, 1200)
  else
    M.toggle_visible()
    deliver(0)
  end
end

---Mostra o terminal do agente (sem fechar se já estiver na tela).
function M.toggle_visible()
  local buf = last.buf
  if buf and vim.fn.bufwinid(buf) == -1 then M.toggle() end
end

---@type LazySpec
return {
  {
    "folke/snacks.nvim",
    keys = {
      { "<Leader>Al", M.launch, desc = "Abrir agente no terminal" },
      { "<Leader>AT", M.toggle, desc = "Alternar terminal do agente" },
      { "<Leader>Ag", function() require("astrograph.agents").pick() end, desc = "Escolher agente (vale para tudo)" },
      { "<Leader>A.", M.send, mode = { "n", "v" }, desc = "Mencionar arquivo/seleção ao agente" },
    },
  },
}
