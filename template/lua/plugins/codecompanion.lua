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

      -- Adapters ACP (agentes no chat). No Windows os CLIs do npm são `.cmd` e o
      -- spawn pelo nome falha (ENOENT): usa o caminho completo resolvido.
      opts.adapters.acp = opts.adapters.acp or {}
      for _, a in ipairs(agents.agents) do
        if agents.has(a.bin) and agents.has_acp(a) then
          opts.adapters.acp[a.adapter] = function()
            local cmd = agents.acp_command(a)
            local ext = { commands = { default = cmd } }
            -- Codex: sem OPENAI_API_KEY usa o login do ChatGPT (`codex login`)
            if a.adapter == "codex" and not vim.env.OPENAI_API_KEY then
              ext.defaults = { auth_method = "chat-gpt" }
            end
            -- Claude Code: o adapter declara `CLAUDE_CODE_OAUTH_TOKEN =
            -- "CLAUDE_CODE_OAUTH_TOKEN"`; sem a variavel no ambiente, o
            -- CodeCompanion mandava o proprio texto como token (erro 401). So
            -- passa o token quando ele existe; senao a ponte usa o login do CLI
            -- (`claude auth login`), como no terminal.
            if a.adapter == "claude_code" then
              ext.env = {
                CLAUDE_CODE_OAUTH_TOKEN = function()
                  local token = vim.env.CLAUDE_CODE_OAUTH_TOKEN
                  return (token and token ~= "") and token or nil
                end,
              }
              ext.handlers = { auth = function() return true end }
            end
            return require("codecompanion.adapters").extend(a.adapter, ext)
          end
        end
      end

      -- Padrões por detecção. inline/cmd/background exigem um adapter HTTP:
      -- Ollama local, depois o primeiro provedor com chave de API no ambiente.
      local http_default = (info.ollama and info.local_model and "ollama")
        or (vim.env.ANTHROPIC_API_KEY and "anthropic")
        or (vim.env.OPENAI_API_KEY and "openai")
        or (vim.env.GEMINI_API_KEY and "gemini")
        or (url and "astrograph_local")
        or "anthropic"
      -- Chat: o agente atual (o mesmo do terminal e da edição inline, escolhido em
      -- :Astrograph agente); sem a ponte ACP dele, o primeiro agente que tiver;
      -- senão o mesmo adapter HTTP.
      local chat_default = http_default
      local current = agents.current()
      if current and agents.has_acp(current) then
        chat_default = current.adapter
      else
        for _, a in ipairs(agents.agents) do
          if agents.has(a.bin) and agents.has_acp(a) then
            chat_default = a.adapter
            break
          end
        end
      end

      opts.interactions = opts.interactions or {}
      opts.interactions.chat = vim.tbl_deep_extend("force", opts.interactions.chat or {}, {
        adapter = chat_default,
        roles = {
          user = "Você",
          llm = function(adapter) return adapter.formatted_name end,
        },
      })

      -- Chat em português (textos e respostas) e janelas com borda arredondada.
      opts.opts = vim.tbl_deep_extend("force", opts.opts or {}, { language = "Portuguese" })
      opts.display = vim.tbl_deep_extend("force", opts.display or {}, {
        chat = {
          intro_message = "Chat do Astrograph ✨  ? opções · ga troca o agente",
          floating_window = { border = "rounded" },
        },
      })
      for _, key in ipairs { "inline", "cmd", "background" } do
        opts.interactions[key] = vim.tbl_deep_extend("force", opts.interactions[key] or {}, { adapter = http_default })
      end

      -- Interação CLI: registra só os agentes presentes no PATH.
      local cli = {}
      for _, a in ipairs(present) do
        cli[a.bin] = { cmd = a.cmd, description = a.label }
      end
      opts.interactions.cli = vim.tbl_deep_extend("force", opts.interactions.cli or {}, {
        agents = cli,
        agent = current and current.bin or (present[1] and present[1].bin) or nil,
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
          local agentic = function() return require "astrograph.agentic" end
          -- abre o chat só depois de checar o login (cacheado): sem login o
          -- agente nunca responde, e é melhor saber antes
          local function chat(cmd)
            return function()
              agentic().ensure_ready(function() vim.cmd(cmd) end)
            end
          end
          -- assistente inline: precisa de um modelo por HTTP (chave de API ou
          -- Ollama); sem isso, o agente atual edita pelo avante
          local function inline()
            local info = require("astrograph.agents").detect()
            local http = (info.ollama and info.local_model) or vim.env.ANTHROPIC_API_KEY
              or vim.env.OPENAI_API_KEY or vim.env.GEMINI_API_KEY or require("astrograph.agents").local_url()
            if http then return vim.cmd "CodeCompanion" end
            local visual = vim.fn.mode():match "[vV\22]"
            -- os <Plug> do avante só existem depois que ele carrega
            require("lazy").load { plugins = { "avante.nvim" } }
            vim.api.nvim_feedkeys(vim.keycode(visual and "<Plug>(AvanteEdit)" or "<Plug>(AvanteAsk)"), "m", false)
          end
          maps.n["<Leader>A"] = { desc = "Agentes" }
          maps.n["<Leader>Ac"] = { chat "CodeCompanionChat Toggle", desc = "Chat (abrir/fechar)" }
          maps.v["<Leader>Ac"] = { chat "CodeCompanionChat Toggle", desc = "Chat (abrir/fechar)" }
          maps.n["<Leader>An"] = { chat "CodeCompanionChat", desc = "Novo chat" }
          maps.n["<Leader>Aq"] = { inline, desc = "Assistente inline" }
          maps.v["<Leader>Aq"] = { inline, desc = "Assistente inline" }
          maps.n["<Leader>Ax"] = { function() agentic().fix() end, desc = "Corrigir o problema da linha" }
          maps.n["<Leader>Ai"] = { function() agentic().explain() end, desc = "Explicar o arquivo" }
          maps.v["<Leader>Ai"] = { function() agentic().explain() end, desc = "Explicar a seleção" }
          maps.n["<Leader>Ar"] = { function() agentic().review() end, desc = "Revisar alterações" }
          maps.n["<Leader>Am"] = { function() agentic().commit_message() end, desc = "Mensagem de commit" }
          maps.n["<Leader>Ad"] = { function() agentic().changes() end, desc = "Ver alterações (diff)" }
          maps.n["<Leader>Ap"] = { "<cmd>CodeCompanionActions<cr>", desc = "Ações (adapter/modelo)" }
          maps.v["<Leader>Ap"] = { "<cmd>CodeCompanionActions<cr>", desc = "Ações (adapter/modelo)" }
          maps.v["<Leader>Aa"] = { "<cmd>CodeCompanionChat Add<cr>", desc = "Adicionar seleção ao chat" }
          -- agente nativo (astrograph-agent): vê o agente trabalhar no próprio código
          local agent = function() return require "astrograph.agent" end
          maps.n["<Leader>Ae"] = { function() agent().ask() end, desc = "Pedir ao agente (nativo)" }
          maps.v["<Leader>Ae"] = { function() agent().ask() end, desc = "Pedir ao agente sobre a seleção" }
          maps.n["<Leader>Ao"] = { function() agent().toggle() end, desc = "Painel do agente" }
          maps.n["<Leader>Ak"] = { function() agent().cancel() end, desc = "Interromper o agente" }
          maps.n["<Leader>Aw"] = { function() agent().pick_mode() end, desc = "Modo do agente" }
          maps.n["<Leader>Av"] = { function() agent().toggle_follow() end, desc = "Seguir o agente (liga/desliga)" }
        end,
      },
    },
  },
}
