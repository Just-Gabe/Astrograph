return {
  "stevearc/aerial.nvim",
  event = "User AstroFileIdle",
  specs = {
    {
      "AstroNvim/astrocore",
      opts = function(_, opts)
        local maps = opts.mappings
        maps.n["<Leader>lS"] = { function() require("aerial").toggle() end, desc = "Estrutura de símbolos" }
      end,
    },
  },
  opts = function(_, opts)
    opts = require("astrocore").extend_tbl(opts, {
      attach_mode = "global",
      backends = { "lsp", "treesitter", "markdown", "man" },
      layout = { min_width = 28 },
      show_guides = true,
      filter_kind = false,
      guides = {
        mid_item = "├ ",
        last_item = "└ ",
        nested_top = "│ ",
        whitespace = "  ",
      },
      keymaps = {
        ["[y"] = "actions.prev",
        ["]y"] = "actions.next",
        ["[Y"] = "actions.prev_up",
        ["]Y"] = "actions.next_up",
        ["{"] = false,
        ["}"] = false,
        ["[["] = false,
        ["]]"] = false,
      },
      on_attach = function(bufnr)
        local astrocore = require "astrocore"
        astrocore.set_mappings({
          n = {
            ["]y"] = { function() require("aerial").next(vim.v.count1) end, desc = "Próximo símbolo" },
            ["[y"] = { function() require("aerial").prev(vim.v.count1) end, desc = "Símbolo anterior" },
            ["]Y"] = { function() require("aerial").next_up(vim.v.count1) end, desc = "Próximo símbolo acima" },
            ["[Y"] = { function() require("aerial").prev_up(vim.v.count1) end, desc = "Símbolo anterior acima" },
          },
        }, { buffer = bufnr })
      end,
    })

    local large_buf = vim.tbl_get(require("astrocore").config, "features", "large_buf")
    if large_buf then
      opts.disable_max_lines, opts.disable_max_size = large_buf.lines or nil, large_buf.size or nil
    end

    return opts
  end,
}
