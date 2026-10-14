-- Launcher de agentes-CLI em terminal flutuante (Snacks).
--
-- Roda o TUI nativo do agente (claude/opencode/codex/gemini) no root do projeto.
-- Os keymaps existem sempre; o picker lista só os agentes presentes no PATH
-- (nenhum custo de detecção no boot).
--
--   <Leader>Al  lançar agente-CLI (picker)   <Leader>AT  alternar o último terminal
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

---Lançar um agente (picker se houver mais de um).
function M.launch()
  local present = require("astrograph.agents").available()
  if #present == 0 then
    vim.notify("Astrograph: nenhum agente-CLI no PATH (claude/opencode/codex/gemini)", vim.log.levels.WARN)
    return
  end
  if #present == 1 then
    open_agent(present[1])
    return
  end
  vim.ui.select(present, {
    prompt = "Agente:",
    format_item = function(a) return a.label end,
  }, function(choice)
    if choice then open_agent(choice) end
  end)
end

---Alternar o último terminal de agente (ou lançar, se não houver).
function M.toggle()
  if not last.cmd then
    M.launch()
    return
  end
  pcall(function()
    require("snacks").terminal.toggle(last.cmd, { cwd = require("astrograph.agents").root(), win = win_opts() })
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
      { "<Leader>Al", M.launch, desc = "Lançar agente-CLI" },
      { "<Leader>AT", M.toggle, desc = "Alternar terminal do agente" },
      { "<Leader>A.", M.send, desc = "Enviar arquivo atual ao agente" },
    },
  },
}
