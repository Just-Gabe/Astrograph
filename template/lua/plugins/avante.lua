-- avante.nvim — edição inline estilo Cursor, priorizando modelo local (Ollama).
--
-- Divisão de trabalho: o CodeCompanion cobre agentes/CLIs/chat amplo; o avante
-- cobre a edição inline com o modelo local. Prefixo de IA: <Leader>A (as teclas
-- abaixo não colidem com as do CodeCompanion nem dos terminais).
--
--   <Leader>AE  perguntar/editar seleção   <Leader>At  alternar sidebar
--   <Leader>As  alternar sugestões          <Leader>Af  focar   <Leader>Az  zen

---@type LazySpec
return {
  {
    "yetone/avante.nvim",
    build = vim.fn.has "win32" == 1
        and "powershell -ExecutionPolicy Bypass -File Build.ps1 -BuildFromSource false"
      or "make",
    -- Carrega só sob demanda (teclas/comandos). Antes era `User AstroFile`, o que
    -- custava ~100ms (+ detecção do Ollama) no primeiro arquivo aberto, mesmo sem
    -- usar o avante. As teclas apontam para os <Plug> do avante porque ele não
    -- remapeia teclas já gerenciadas pelo lazy.nvim (Utils.safe_keymap_set).
    keys = {
      { "<Leader>AE", "<Plug>(AvanteAsk)", mode = "n", remap = true, desc = "avante: perguntar" },
      { "<Leader>AE", "<Plug>(AvanteEdit)", mode = "v", remap = true, desc = "avante: editar seleção" },
      { "<Leader>At", "<Plug>(AvanteToggle)", remap = true, desc = "avante: alternar sidebar" },
      { "<Leader>As", "<Plug>(AvanteToggleSuggestion)", remap = true, desc = "avante: alternar sugestões" },
      { "<Leader>Af", "<Plug>(AvanteFocus)", remap = true, desc = "avante: focar" },
      { "<Leader>Az", "<Plug>(AvanteZenMode)", mode = { "n", "v" }, remap = true, desc = "avante: zen" },
    },
    cmd = {
      "AvanteAsk",
      "AvanteBuild",
      "AvanteEdit",
      "AvanteRefresh",
      "AvanteSwitchProvider",
      "AvanteShowRepoMap",
      "AvanteModels",
      "AvanteChat",
      "AvanteToggle",
      "AvanteClear",
      "AvanteFocus",
      "AvanteStop",
    },
    dependencies = {
      "nvim-lua/plenary.nvim",
      "MunifTanjim/nui.nvim",
      { "stevearc/dressing.nvim", optional = true },
      { "folke/snacks.nvim", optional = true },
    },
    opts = function(_, opts)
      local agents = require "astrograph.agents"
      local info = agents.detect()

      opts.selector = { provider = "snacks" }
      opts.behaviour = vim.tbl_deep_extend("force", opts.behaviour or {}, { auto_suggestions = false })

      -- Teclas do Astrograph (o resto fica no padrão do avante, sob <leader>a).
      opts.mappings = {
        ask = "<Leader>AE",
        edit = "<Leader>AE",
        focus = "<Leader>Af",
        zen_mode = "<Leader>Az",
        toggle = { default = "<Leader>At", suggestion = "<Leader>As" },
      }

      -- Agentes via ACP (usam o login do CLI, sem chave de API). No Windows o
      -- binário do npm é um `.cmd`: o uv.spawn precisa do caminho completo. O
      -- ACP_PATH_TO_CLAUDE_CODE_EXECUTABLE padrão aponta para o shim sem
      -- extensão (que não executa no Windows); vazio, a ponte usa o CLI embutido.
      opts.acp_providers = opts.acp_providers or {}
      for _, a in ipairs(agents.agents) do
        if a.adapter == "claude_code" and agents.has(a.bin) and agents.has_acp(a) then
          opts.acp_providers["claude-code"] = vim.tbl_deep_extend("force", opts.acp_providers["claude-code"] or {}, {
            command = agents.exe(a.acp_bin),
            env = { ACP_PATH_TO_CLAUDE_CODE_EXECUTABLE = "" },
          })
        elseif a.adapter == "gemini_cli" and agents.has(a.bin) then
          opts.acp_providers["gemini-cli"] =
            vim.tbl_deep_extend("force", opts.acp_providers["gemini-cli"] or {}, { command = agents.exe(a.bin) })
        elseif a.adapter == "codex" and agents.has(a.bin) and agents.has_acp(a) then
          opts.acp_providers["codex"] =
            vim.tbl_deep_extend("force", opts.acp_providers["codex"] or {}, { command = agents.exe(a.acp_bin) })
        end
      end

      -- Provedor: modelo local (Ollama) quando no ar; senão Claude por API (se
      -- houver chave); senão o Claude Code via ACP; por último a API do Claude.
      if info.ollama and info.local_model then
        opts.provider = "ollama"
        opts.providers = {
          ollama = {
            endpoint = agents.ollama_host(),
            model = info.local_model,
            is_env_set = function()
              local ok, mod = pcall(require, "avante.providers.ollama")
              return ok and mod.check_endpoint_alive and mod.check_endpoint_alive() or false
            end,
          },
        }
      elseif vim.env.ANTHROPIC_API_KEY then
        opts.provider = "claude"
      elseif opts.acp_providers["claude-code"] then
        opts.provider = "claude-code"
      else
        opts.provider = "claude"
      end

      return opts
    end,
  },
}
