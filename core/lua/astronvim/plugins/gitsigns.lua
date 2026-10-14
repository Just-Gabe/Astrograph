return {
  "lewis6991/gitsigns.nvim",
  enabled = require("astronvim").executable "git",
  event = "User AstroGitFile",
  opts = function(_, opts)
    local astrocore, get_icon = require "astrocore", require("astroui").get_icon
    return astrocore.extend_tbl(opts, {
      signs = {
        add = { text = get_icon "GitSign" },
        change = { text = get_icon "GitSign" },
        delete = { text = get_icon "GitSign" },
        topdelete = { text = get_icon "GitSign" },
        changedelete = { text = get_icon "GitSign" },
        untracked = { text = get_icon "GitSign" },
      },
      signs_staged = {
        add = { text = get_icon "GitSign" },
        change = { text = get_icon "GitSign" },
        delete = { text = get_icon "GitSign" },
        topdelete = { text = get_icon "GitSign" },
        changedelete = { text = get_icon "GitSign" },
        untracked = { text = get_icon "GitSign" },
      },
      on_attach = function(bufnr)
        local prefix, maps = "<Leader>g", astrocore.empty_map_table()
        for _, mode in ipairs { "n", "v" } do
          maps[mode][prefix] = { desc = get_icon("Git", 1, true) .. "Git" }
        end

        maps.n[prefix .. "l"] = { function() require("gitsigns").blame_line() end, desc = "Git: ver blame" }
        maps.n[prefix .. "L"] =
          { function() require("gitsigns").blame_line { full = true } end, desc = "Git: ver blame completo" }
        maps.n[prefix .. "p"] = { function() require("gitsigns").preview_hunk_inline() end, desc = "Pré-visualizar trecho do Git" }
        maps.n[prefix .. "r"] = { function() require("gitsigns").reset_hunk() end, desc = "Git: desfazer trecho" }
        maps.v[prefix .. "r"] = {
          function() require("gitsigns").reset_hunk { vim.fn.line ".", vim.fn.line "v" } end,
          desc = "Git: desfazer trecho",
        }
        maps.n[prefix .. "R"] = { function() require("gitsigns").reset_buffer() end, desc = "Git: desfazer alterações do buffer" }
        maps.n[prefix .. "s"] = { function() require("gitsigns").stage_hunk() end, desc = "Git: preparar/remover trecho do stage" }
        maps.v[prefix .. "s"] = {
          function() require("gitsigns").stage_hunk { vim.fn.line ".", vim.fn.line "v" } end,
          desc = "Git: preparar trecho (stage)",
        }
        maps.n[prefix .. "S"] = { function() require("gitsigns").stage_buffer() end, desc = "Git: preparar buffer (stage)" }
        maps.n[prefix .. "d"] = { function() require("gitsigns").diffthis() end, desc = "Git: ver diff" }

        maps.n["[G"] = { function() require("gitsigns").nav_hunk "first" end, desc = "Primeiro trecho do Git" }
        maps.n["]G"] = { function() require("gitsigns").nav_hunk "last" end, desc = "Último trecho do Git" }
        maps.n["]g"] = { function() require("gitsigns").nav_hunk "next" end, desc = "Próximo trecho do Git" }
        maps.n["[g"] = { function() require("gitsigns").nav_hunk "prev" end, desc = "Trecho anterior do Git" }
        for _, mode in ipairs { "o", "x" } do
          maps[mode]["ig"] = { ":<C-U>Gitsigns select_hunk<CR>", desc = "dentro do trecho do Git" }
        end

        astrocore.set_mappings(maps, { buffer = bufnr })
      end,
      worktrees = astrocore.config.git_worktrees,
    })
  end,
}
