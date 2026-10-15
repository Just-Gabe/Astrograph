-- `:checkhealth astrograph` — diagnóstico do Astrograph em português.
--
-- Confere o que o editor precisa para funcionar (Neovim, git, ripgrep, fd,
-- compilador C), de onde vêm o core e o tema, e o que o modo agentic encontrou
-- (CLIs no PATH, Ollama, modelo local).

local M = {}

local health = vim.health

local function exe(bin) return vim.fn.executable(bin) == 1 end

---@param level "error"|"warn"|"info" gravidade quando o binário falta
local function check_tool(bin, why, level)
  if exe(bin) then
    health.ok(("`%s` encontrado (%s)"):format(bin, why))
  else
    health[level](("`%s` não encontrado — %s"):format(bin, why))
  end
end

function M.check()
  health.start "Astrograph: base"
  if vim.fn.has "nvim-0.11" == 1 then
    health.ok("Neovim " .. tostring(vim.version()))
  else
    health.error("Neovim " .. tostring(vim.version()) .. " — o Astrograph precisa do 0.11 ou mais novo")
  end

  local core = vim.tbl_get(require("lazy.core.config").plugins, "AstroNvim", "dir")
  if core and core:match "[Aa]strograph" then
    health.ok("Core local do Astrograph: " .. vim.fs.normalize(core))
  elseif core then
    health.warn("Core do AstroNvim oficial (" .. core .. ") — o `../core` do Astrograph não foi achado")
  end

  if vim.g.astrograph_theme then
    health.ok(("Tema: %s (de %s)"):format(tostring(vim.g.colors_name), vim.fs.normalize(vim.g.astrograph_theme)))
  else
    health.warn "Pasta `theme/` não encontrada — usando astrodark como fallback"
  end

  health.start "Astrograph: ferramentas"
  check_tool("git", "obrigatório para plugins e atualizações", "error")
  check_tool("rg", "busca de texto: <Leader>fw", "warn")
  check_tool("fd", "busca de arquivos: <Leader>ff", "warn")
  local cc = (exe "gcc" and "gcc") or (exe "cc" and "cc") or (exe "clang" and "clang") or (exe "zig" and "zig")
  if cc then
    health.ok(("Compilador C: `%s` (parsers do Treesitter)"):format(cc))
  else
    health.warn "Nenhum compilador C (gcc/clang/zig) — parsers do Treesitter não vão compilar"
  end
  check_tool("lazygit", "opcional: <Leader>gg", "info")

  health.start "Astrograph: agentes de IA"
  local agents = require "astrograph.agents"
  local info = agents.detect(true)
  local found = agents.available()
  if #found == 0 then
    health.info "Nenhum agente de IA instalado. Rode `:Astrograph configurar` para instalar um."
  else
    for _, a in ipairs(found) do
      local login = agents.logged_in(a)
      if login == false then
        health.warn(("%s instalado, mas sem login — rode `:Astrograph login %s`"):format(a.label, a.bin))
      else
        health.ok(("%s (`%s`)%s — <Leader>Al abre no terminal"):format(
          a.label,
          a.bin,
          login and ", logado" or ""
        ))
      end
      if agents.legacy_acp(a) then
        health.warn(
          ("%s no chat funciona, mas a ponte `%s` é o pacote antigo (renomeado, sem atualizações) — rode `:Astrograph configurar`"):format(
            a.label,
            a.acp_bin
          )
        )
      elseif agents.has_acp(a) then
        health.ok(("%s no chat (<Leader>Ac): ponte ACP `%s` instalada"):format(a.label, a.acp_bin))
      else
        health.warn(
          ("%s não abre no chat: falta a ponte `%s` — rode `:Astrograph configurar` (npm install -g %s)"):format(
            a.label,
            a.acp_bin,
            a.acp_npm or ""
          )
        )
      end
    end
  end
  local http = vim.env.ANTHROPIC_API_KEY or vim.env.OPENAI_API_KEY or vim.env.GEMINI_API_KEY or info.local_model
  if not http then
    health.info(
      "Assistente inline (<Leader>Aq): sem Ollama nem chave de API, ele usa o agente atual pelo avante "
        .. "(com uma chave ANTHROPIC_API_KEY, OPENAI_API_KEY ou GEMINI_API_KEY, ou o Ollama, fica mais rápido). "
        .. "Chat, terminal e as ações (<Leader>Ax corrigir, Ai explicar, Ar revisar, Am commit) usam o login dos agentes."
    )
  end
  if info.ollama then
    health.ok(("Ollama no ar em %s (%d modelos)"):format(agents.ollama_host(), #info.models))
    if info.local_model then
      health.ok("Modelo local: " .. info.local_model)
    else
      health.warn "Ollama sem modelo — rode `ollama pull qwen2.5-coder:7b`"
    end
  elseif exe "ollama" then
    health.warn "Ollama instalado mas fora do ar — rode `ollama serve`"
  else
    health.info "Ollama não instalado (opcional, para IA local/offline)"
  end
  if vim.fn.filereadable(agents.config_path()) == 1 then
    health.info("Config do instalador: " .. agents.config_path())
  end
end

return M
