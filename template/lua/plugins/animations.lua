-- Animações no estilo "Claude/opencode": rápidas, suaves e sem overshoot.
-- Controladas pelo Snacks: janelas (backdrop), scroll suave, indent guides e
-- transições de picker/notify. Desligar tudo globalmente: vim.g.snacks_animate = false
--
--   <Leader>um  → liga/desliga TODAS as animações (m = movimento)
--   <Leader>uM  → liga/desliga apenas o scroll suave

---@type LazySpec
return {
  {
    "folke/snacks.nvim",
    opts = function(_, opts)
      -- Via SSH cada quadro vira tráfego de rede: desliga tudo (reative com <Leader>um)
      if vim.env.SSH_CONNECTION or vim.env.SSH_TTY then vim.g.snacks_animate = false end

      -- Base das animações. 60 fps acompanha a taxa de atualização dos terminais;
      -- 120 dobrava os redraws sem diferença visível (e tremia no Windows Terminal).
      opts.animate = { fps = 60, duration = { step = 5, total = 160 }, easing = "outQuad" }

      -- Scroll suave: passo curto, easing "out" para começar forte e frear no fim
      opts.scroll = {
        animate = { duration = { step = 8, total = 140 }, easing = "outCubic" },
        animate_repeat = { delay = 120, duration = { step = 5, total = 60 }, easing = "linear" },
      }

      -- AstroNvim desliga animação das indent guides; reativamos com easing suave
      opts.indent = vim.tbl_deep_extend("force", opts.indent or {}, {
        animate = { enabled = true, easing = "outQuad", duration = { step = 5, total = 160 } },
      })

      -- Backdrop levemente escurecido nos floats (notify, lazy, picker)
      opts.win = vim.tbl_deep_extend("force", opts.win or {}, { backdrop = 60 })

      return opts
    end,
    keys = {
      {
        "<Leader>um",
        function()
          require("snacks").toggle.animate():toggle()
          -- o cursor animado (smear-cursor) acompanha o liga/desliga global
          local ok, smear = pcall(require, "smear_cursor")
          if ok then smear.enabled = vim.g.snacks_animate ~= false end
        end,
        desc = "Alternar animações",
      },
      { "<Leader>uM", function() require("snacks").toggle.scroll():toggle() end, desc = "Alternar scroll suave" },
    },
  },
}
