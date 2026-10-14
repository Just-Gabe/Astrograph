local term = vim.trim((vim.env.TERM_PROGRAM or ""):lower())
local mux = term == "tmux" or term == "wezterm" or vim.env.KITTY_LISTEN_ON

return {
  "mrjones2014/smart-splits.nvim",
  lazy = true,
  event = mux and "VeryLazy" or nil, -- load early if mux detected
  specs = {
    {
      "AstroNvim/astrocore",
      opts = function(_, opts)
        local maps = opts.mappings
        maps.n["<C-H>"] = { function() require("smart-splits").move_cursor_left() end, desc = "Ir para a divisão da esquerda" }
        maps.n["<C-J>"] = { function() require("smart-splits").move_cursor_down() end, desc = "Ir para a divisão de baixo" }
        maps.n["<C-K>"] = { function() require("smart-splits").move_cursor_up() end, desc = "Ir para a divisão de cima" }
        maps.n["<C-L>"] = { function() require("smart-splits").move_cursor_right() end, desc = "Ir para a divisão da direita" }
        maps.n["<C-Up>"] = { function() require("smart-splits").resize_up() end, desc = "Redimensionar para cima" }
        maps.n["<C-Down>"] = { function() require("smart-splits").resize_down() end, desc = "Redimensionar para baixo" }
        maps.n["<C-Left>"] = { function() require("smart-splits").resize_left() end, desc = "Redimensionar para a esquerda" }
        maps.n["<C-Right>"] = { function() require("smart-splits").resize_right() end, desc = "Redimensionar para a direita" }
      end,
    },
  },
  opts = { ignored_filetypes = { "nofile", "quickfix", "qf", "prompt" }, ignored_buftypes = { "nofile" } },
}
