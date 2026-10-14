-- `:Astrograph <subcomando>` — atalhos do dia a dia, em português.
--
--   :Astrograph atualizar   baixa a versão nova do Astrograph e sincroniza os plugins
--   :Astrograph saude       abre o `:checkhealth astrograph`
--   :Astrograph agentes     mostra os agentes de IA detectados (refaz a detecção)
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

function M.agentes()
  local agents = require "astrograph.agents"
  local info = agents.detect(true)
  local lines = { "Agentes de IA detectados:" }
  for _, a in ipairs(agents.agents) do
    lines[#lines + 1] = ("  %s %s"):format(agents.has(a.bin) and "✓" or "·", a.label)
  end
  lines[#lines + 1] = ("  %s Ollama%s"):format(
    info.ollama and "✓" or "·",
    info.local_model and (" (" .. info.local_model .. ")") or (info.ollama and " (sem modelo)" or " (fora do ar)")
  )
  lines[#lines + 1] = ""
  lines[#lines + 1] = "<Leader>Ac chat · <Leader>Al terminal do agente · <Leader>AE avante"
  notify(table.concat(lines, "\n"))
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
  atalhos = M.atalhos,
  docs = M.docs,
}
local names = vim.tbl_keys(subcommands)
table.sort(names)

function M.setup()
  vim.api.nvim_create_user_command("Astrograph", function(args)
    local fn = subcommands[args.args ~= "" and args.args or "saude"]
    if not fn then
      notify("Subcomando desconhecido: " .. args.args .. "\nUse: " .. table.concat(names, ", "), vim.log.levels.WARN)
      return
    end
    fn()
  end, {
    nargs = "?",
    desc = "Astrograph: atualizar, saude, agentes, atalhos, docs",
    complete = function(lead)
      return vim.tbl_filter(function(c) return c:find(lead, 1, true) == 1 end, names)
    end,
  })
end

return M
