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
    event = "User AstroFile",
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

      -- Local (Ollama) quando disponível; senão Claude via API/env.
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
      else
        opts.provider = "claude"
      end

      return opts
    end,
  },
}
