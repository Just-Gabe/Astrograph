-- `:Astrograph <subcomando>` — atalhos do dia a dia, em português.
--
--   :Astrograph atualizar   baixa a versão nova do Astrograph e sincroniza os plugins
--   :Astrograph saude       abre o `:checkhealth astrograph`
--   :Astrograph agentes     mostra os agentes de IA: instalado, login e chat (ACP)
--   :Astrograph configurar  instala as pontes do chat e faz os logins que faltam
--   :Astrograph login [ag]  abre o login de um agente (claude, opencode, codex, gemini)
--   :Astrograph agente [ag] escolhe o agente de todo o modo agentic (fica salvo)
--   :Astrograph atalhos     lista todos os atalhos (busca)
--   :Astrograph docs        abre a documentação no navegador

local M = {}

M.docs_url = "https://just-gabe.github.io/Astrograph/"

local function notify(msg, level) vim.notify(msg, level or vim.log.levels.INFO, { title = "Astrograph" }) end

---Raiz do clone do Astrograph (pai de `template/`), seguindo a junction/symlink da config.
---@return string?
function M.root()
  local uv = vim.uv or vim.loop
  local config = uv.fs_realpath(vim.fn.stdpath "config") or vim.fn.stdpath "config"
  local root = vim.fn.fnamemodify(config, ":h")
  if uv.fs_stat(root .. "/.git") then return root end
end

function M.atualizar()
  local root = M.root()
  if not root then
    notify("Não achei o repositório do Astrograph (a config não aponta para um clone git).", vim.log.levels.WARN)
    return
  end
  notify("Atualizando " .. root .. " …")
  -- o `:Lazy update` reescreve o lazy-lock.json versionado; descarta antes do pull
  vim.system({ "git", "-C", root, "checkout", "--", "template/lazy-lock.json" }, {}, function()
    vim.system({ "git", "-C", root, "pull", "--ff-only" }, { text = true }, function(res)
      vim.schedule(function()
        if res.code ~= 0 then
          notify("Falha no git pull:\n" .. (res.stderr or ""), vim.log.levels.ERROR)
          return
        end
        local out = vim.trim(res.stdout or "")
        if out:match "Already up to date" or out:match "Já está atualizado" then
          notify "O Astrograph já está na última versão."
          return
        end
        notify "Astrograph atualizado. Sincronizando plugins com o lazy-lock…"
        require("lazy").restore { wait = false }
        notify "Pronto. Reinicie o Neovim para carregar a versão nova."
      end)
    end)
  end)
end

function M.saude() vim.cmd "checkhealth astrograph" end

---Abre um comando interativo num terminal flutuante (logins, npm install).
---@param cmd string
---@param title string
---@param on_close? fun()
local function run_interactive(cmd, title, on_close)
  local ok, snacks = pcall(require, "snacks")
  if not ok then
    vim.cmd("botright split | terminal " .. cmd)
    return
  end
  local term = snacks.terminal.open(cmd, {
    cwd = vim.fn.getcwd(),
    interactive = true,
    auto_close = false,
    win = { position = "float", border = "rounded", title = " " .. title .. " ", title_pos = "center" },
  })
  if on_close and term and term.buf then
    vim.api.nvim_create_autocmd("TermClose", {
      buffer = term.buf,
      once = true,
      callback = function() vim.schedule(on_close) end,
    })
  end
end

---Junta comandos para o shell do Neovim (`&&`; o PowerShell 5.1 não tem `&&`).
---@param cmds string[]
---@return string
local function chain(cmds)
  local shell = vim.fn.fnamemodify(vim.o.shell, ":t"):lower()
  local sep = shell:match "^powershell" and "; " or " && "
  return table.concat(cmds, sep)
end

---Comandos para instalar a ponte ACP de um agente (remove o pacote antigo antes).
---@param a AstrographAgent
---@return string[]
local function acp_install_cmds(a)
  local agents = require "astrograph.agents"
  local cmds = {}
  local legacy = agents.legacy_npm[a.bin]
  if legacy then cmds[#cmds + 1] = "npm uninstall -g " .. legacy end
  cmds[#cmds + 1] = "npm install -g " .. a.acp_npm
  return cmds
end

local function finished()
  require("astrograph.agents").detect(true)
  -- o chat deixa de avisar "sem login" assim que o login entra
  pcall(function() require("astrograph.agentic").refresh_login() end)
  notify "Pronto. Reinicie o Neovim para os agentes novos aparecerem no chat (<Leader>Ac)."
end

function M.agentes()
  local agents = require "astrograph.agents"
  local info = agents.detect(true)
  notify "Verificando os agentes…"
  agents.status(function(list)
    local lines = { "Agentes de IA:" }
    local by_bin = {}
    for _, item in ipairs(list) do
      by_bin[item.agent.bin] = item
    end
    local todo = false
    for _, a in ipairs(agents.agents) do
      local item = by_bin[a.bin]
      if not item then
        lines[#lines + 1] = ("  · %-12s não instalado"):format(a.label)
      else
        local login = item.login == true and "logado" or item.login == false and "SEM LOGIN" or "login ?"
        local chat = not item.acp and "chat: falta a ponte"
          or agents.legacy_acp(a) and "chat ok (ponte antiga)"
          or "chat ok"
        local cur = agents.current()
        local mark = cur and cur.bin == a.bin and "●" or "✓"
        lines[#lines + 1] = ("  %s %-12s %s · %s"):format(mark, a.label, login, chat)
        todo = todo or item.login == false or not item.acp or agents.legacy_acp(a)
      end
    end
    lines[#lines + 1] = ("  %s Ollama%s"):format(
      info.ollama and "✓" or "·",
      info.local_model and (" (" .. info.local_model .. ")") or (info.ollama and " (sem modelo)" or " (fora do ar)")
    )
    lines[#lines + 1] = ""
    lines[#lines + 1] = "● agente atual (troque com <Leader>Ag ou :Astrograph agente)"
    lines[#lines + 1] = todo and "Rode :Astrograph configurar para resolver o que falta."
      or "<Leader>Ac chat · <Leader>Al terminal do agente · <Leader>AE avante"
    notify(table.concat(lines, "\n"), todo and vim.log.levels.WARN or vim.log.levels.INFO)
  end)
end

---@param name? string
function M.login(name)
  local agents = require "astrograph.agents"
  local list = vim.tbl_filter(function(a) return agents.has(a.bin) end, agents.agents)
  if #list == 0 then
    notify(
      "Nenhum agente instalado. Rode :Astrograph configurar ou o instalador.",
      vim.log.levels.WARN
    )
    return
  end
  local function go(a)
    local hint = a.bin == "gemini" and " (entre e saia com /quit)" or ""
    run_interactive(a.login, "Login: " .. a.label .. hint, finished)
  end
  if name and name ~= "" then
    for _, a in ipairs(list) do
      if a.bin == name then return go(a) end
    end
    notify("Agente não instalado: " .. name, vim.log.levels.WARN)
    return
  end
  vim.ui.select(list, { prompt = "Login em qual agente?", format_item = function(a) return a.label end }, function(a)
    if a then go(a) end
  end)
end

---Instala as pontes ACP e faz os logins que faltam, num terminal só.
function M.configurar()
  local agents = require "astrograph.agents"
  agents.detect(true)
  notify "Verificando o que falta…"
  agents.status(function(list)
    local actions = {}
    for _, item in ipairs(list) do
      local a = item.agent
      if item.acp and agents.legacy_acp(a) then
        actions[#actions + 1] = {
          label = a.label .. ": atualizar a ponte do chat (pacote antigo foi renomeado)",
          cmds = acp_install_cmds(a),
        }
      elseif not item.acp and a.acp_npm then
        actions[#actions + 1] = {
          label = a.label .. ": instalar a ponte do chat (" .. a.acp_npm .. ")",
          cmds = acp_install_cmds(a),
        }
      end
      if item.login == false then
        actions[#actions + 1] = { label = a.label .. ": fazer login", cmds = { a.login } }
      end
    end
    local required = #actions
    if agents.has "npm" then
      for _, a in ipairs(agents.agents) do
        if not agents.has(a.bin) then
          local cmds = { "npm install -g " .. a.npm }
          if a.acp_npm then vim.list_extend(cmds, acp_install_cmds(a)) end
          cmds[#cmds + 1] = a.login
          actions[#actions + 1] = { label = "Instalar " .. a.label .. " (opcional)", cmds = cmds }
        end
      end
    end
    if #actions == 0 then
      notify "Tudo certo: os agentes instalados estão logados e com o chat pronto."
      return
    end
    local choices = {}
    if required > 1 then
      local all = {}
      for i = 1, required do
        vim.list_extend(all, actions[i].cmds)
      end
      choices[1] = { label = "Resolver tudo que falta (" .. required .. " itens)", cmds = all }
    end
    vim.list_extend(choices, actions)
    vim.ui.select(choices, {
      prompt = required == 0 and "Tudo certo. Instalar mais algum agente?" or "Configurar agentes",
      format_item = function(x) return x.label end,
    }, function(x)
      if x then run_interactive(chain(x.cmds), "Astrograph: configurar agentes", finished) end
    end)
  end)
end

function M.atalhos()
  local ok, snacks = pcall(require, "snacks")
  if ok and snacks.picker then
    snacks.picker.keymaps()
  else
    vim.cmd "map"
  end
end

function M.docs() vim.ui.open(M.docs_url) end

local subcommands = {
  atualizar = M.atualizar,
  saude = M.saude,
  agentes = M.agentes,
  configurar = M.configurar,
  login = M.login,
  agente = function(name)
    if name and name ~= "" then
      require("astrograph.agents").set_current(name)
    else
      require("astrograph.agents").pick()
    end
  end,
  atalhos = M.atalhos,
  docs = M.docs,
}
local names = vim.tbl_keys(subcommands)
table.sort(names)

function M.setup()
  vim.api.nvim_create_user_command("Astrograph", function(args)
    local sub, rest = (args.args or ""):match "^(%S*)%s*(.-)$"
    local fn = subcommands[sub ~= "" and sub or "saude"]
    if not fn then
      notify("Subcomando desconhecido: " .. sub .. "\nUse: " .. table.concat(names, ", "), vim.log.levels.WARN)
      return
    end
    fn(rest)
  end, {
    nargs = "*",
    desc = "Astrograph: atualizar, saude, agentes, agente, configurar, login, atalhos, docs",
    complete = function(lead, line)
      if line:match "^%S+%s+login%s+" or line:match "^%S+%s+agente%s+" then
        local bins = vim.tbl_map(function(a) return a.bin end, require("astrograph.agents").agents)
        return vim.tbl_filter(function(c) return c:find(lead, 1, true) == 1 end, bins)
      end
      return vim.tbl_filter(function(c) return c:find(lead, 1, true) == 1 end, names)
    end,
  })
end

return M
