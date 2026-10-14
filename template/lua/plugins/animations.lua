-- Animações no estilo "Claude/opencode": rápidas, suaves e sem overshoot.
-- Controladas pelo Snacks: janelas (backdrop), scroll suave, indent guides e
-- transições de picker/notify. Desligar tudo globalmente: vim.g.snacks_animate = false
--
--   <Leader>ua  → liga/desliga TODAS as animações
--   <Leader>uC  → liga/desliga apenas o scroll suave

---@type LazySpec
return {
  {
    "folke/snacks.nvim",
    opts = function(_, opts)
      -- Base das animações (fps alto + easing suave = sensação fluida)
      opts.animate = { fps = 120, duration = { step = 5, total = 160 }, easing = "outQuad" }

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
      { "<Leader>ua", function() require("snacks").toggle.animate():toggle() end, desc = "Toggle Animations" },
      { "<Leader>uC", function() require("snacks").toggle.scroll():toggle() end, desc = "Toggle Smooth Scroll" },
    },
  },
}
