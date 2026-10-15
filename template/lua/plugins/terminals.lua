-- Launcher de agentes-CLI em terminal flutuante (Snacks).
--
-- Roda o TUI nativo do agente atual (claude/opencode/codex/gemini) no root do
-- projeto. O agente é o mesmo do chat e da edição inline: troque com
-- :Astrograph agente ou <Leader>Ag (nenhum custo de detecção no boot).
--
--   <Leader>Al  abrir o agente atual         <Leader>AT  alternar o terminal dele
--   <Leader>A.  enviar o arquivo atual ao agente

local M = {}

local last = { cmd = nil, buf = nil }

local function win_opts() return { position = "float", border = "rounded" } end

---Abre um agente num terminal flutuante.
---@param a AstrographAgent
local function open_agent(a)
  local ok, term = pcall(function()
    return require("snacks").terminal.open(a.cmd, {
      cwd = require("astrograph.agents").root(),
      win = win_opts(),
    })
  end)
  if not ok or not term then
    -- Fallback: terminal nativo do Neovim.
    vim.cmd "botright split | terminal"
    vim.fn.chansend(vim.b.terminal_job_id, a.cmd .. "\r")
    last.cmd = a.cmd
    return
  end
  last.cmd = a.cmd
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
  if last.cmd ~= a.cmd then return M.launch() end
  pcall(function()
    require("snacks").terminal.toggle(a.cmd, { cwd = require("astrograph.agents").root(), win = win_opts() })
  end)
end

---Envia o caminho do arquivo atual (buffer alternativo) para o terminal do agente.
function M.send()
  local buf = last.buf
  if not (buf and vim.api.nvim_buf_is_valid(buf)) then
    vim.notify("Astrograph: nenhum terminal de agente aberto", vim.log.levels.WARN)
    return
  end
  local chan = vim.b[buf].terminal_job_id
  if not chan then
    vim.notify("Astrograph: terminal sem job ativo", vim.log.levels.WARN)
    return
  end
  local path = vim.fn.expand "#:p"
  if path == "" then path = vim.fn.getcwd() end
  vim.api.nvim_chan_send(chan, path .. " ")
end

---@type LazySpec
return {
  {
    "folke/snacks.nvim",
    keys = {
      { "<Leader>Al", M.launch, desc = "Abrir agente no terminal" },
      { "<Leader>AT", M.toggle, desc = "Alternar terminal do agente" },
      { "<Leader>Ag", function() require("astrograph.agents").pick() end, desc = "Escolher agente (vale para tudo)" },
      { "<Leader>A.", M.send, desc = "Enviar arquivo atual ao agente" },
    },
  },
}
