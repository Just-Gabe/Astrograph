-- CodeCompanion como engine unificada de agentes do Astrograph.
--
-- Cobre três modos, com detecção automática (ver lua/astrograph/agents.lua):
--   * HTTP  -> anthropic, openai, gemini, ollama e endpoints locais OpenAI-compatíveis
--   * ACP   -> claude_code, codex, gemini_cli, opencode (só no chat buffer)
--   * CLI   -> TUIs nativos do agente, recebendo contexto do buffer
--
-- Prefixo de IA: <Leader>A. Troca de adapter/modelo dentro do chat: `ga`.
--
--   <Leader>Ac  chat (toggle)        <Leader>An  novo chat
--   <Leader>Aq  assistente inline    <Leader>Ap  paleta de ações
--   <Leader>Aa  adicionar seleção (visual)

---@type LazySpec
return {
  {
    "olimorris/codecompanion.nvim",
    cmd = {
      "CodeCompanion",
      "CodeCompanionChat",
      "CodeCompanionActions",
      "CodeCompanionCmd",
      "CodeCompanionCLI",
    },
    dependencies = { "nvim-lua/plenary.nvim", "nvim-treesitter/nvim-treesitter" },
    opts = function(_, opts)
      local agents = require "astrograph.agents"
      local info = agents.detect()
      local present = agents.available()

      opts.adapters = opts.adapters or {}
      opts.adapters.http = opts.adapters.http or {}

      -- Ollama: só sobrescreve o adapter embutido quando há um modelo definido.
      if info.ollama and info.local_model then
        opts.adapters.http.ollama = function()
          local adapter = require("codecompanion.adapters").extend("ollama", { env = { url = agents.ollama_host() } })
          if adapter.schema and adapter.schema.model then adapter.schema.model.default = info.local_model end
          return adapter
        end
      end

      -- Endpoint local OpenAI-compatível (LM Studio, llama.cpp, vLLM…).
      local url = agents.local_url()
      if url then
        opts.adapters.http.astrograph_local = function()
          return require("codecompanion.adapters").extend("openai_compatible", {
            env = { url = url, api_key = vim.env.ASTROGRAPH_LOCAL_KEY or "no-key" },
            schema = { model = { default = vim.env.ASTROGRAPH_LOCAL_MODEL or "local" } },
          })
        end
      end

      -- Defaults por detecção. inline/cmd/background exigem adapter HTTP.
      local local_default = (info.ollama and info.local_model and "ollama") or "anthropic"
      local chat_default = (info.claude and "claude_code") or local_default

      opts.interactions = opts.interactions or {}
      opts.interactions.chat = vim.tbl_deep_extend("force", opts.interactions.chat or {}, { adapter = chat_default })
      for _, key in ipairs { "inline", "cmd", "background" } do
        opts.interactions[key] = vim.tbl_deep_extend("force", opts.interactions[key] or {}, { adapter = local_default })
      end

      -- Interação CLI: registra só os agentes presentes no PATH.
      local cli = {}
      for _, a in ipairs(present) do
        cli[a.bin] = { cmd = a.cmd, description = a.label }
      end
      opts.interactions.cli = vim.tbl_deep_extend("force", opts.interactions.cli or {}, {
        agents = cli,
        agent = present[1] and present[1].bin or nil,
      })

      return opts
    end,
    specs = {
      {
        "AstroNvim/astrocore",
        opts = function(_, opts)
          local maps = opts.mappings
          maps.n = maps.n or {}
          maps.v = maps.v or {}
          maps.n["<Leader>A"] = { desc = "Agentes" }
          maps.n["<Leader>Ac"] = { "<cmd>CodeCompanionChat Toggle<cr>", desc = "Chat (abrir/fechar)" }
          maps.v["<Leader>Ac"] = { "<cmd>CodeCompanionChat Toggle<cr>", desc = "Chat (abrir/fechar)" }
          maps.n["<Leader>An"] = { "<cmd>CodeCompanionChat<cr>", desc = "Novo chat" }
          maps.n["<Leader>Aq"] = { "<cmd>CodeCompanion<cr>", desc = "Assistente inline" }
          maps.v["<Leader>Aq"] = { "<cmd>CodeCompanion<cr>", desc = "Assistente inline" }
          maps.n["<Leader>Ap"] = { "<cmd>CodeCompanionActions<cr>", desc = "Ações (adapter/modelo)" }
          maps.v["<Leader>Ap"] = { "<cmd>CodeCompanionActions<cr>", desc = "Ações (adapter/modelo)" }
          maps.v["<Leader>Aa"] = { "<cmd>CodeCompanionChat Add<cr>", desc = "Adicionar seleção ao chat" }
        end,
      },
    },
  },
}
