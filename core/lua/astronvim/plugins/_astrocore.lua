local astronvim = require "astronvim"
astronvim.init()

return {
  { "folke/lazy.nvim", dir = vim.env.LAZY },
  {
    "AstroNvim/AstroNvim",
    build = function()
      if astronvim.config.pin_plugins and astronvim.config.update_notification ~= false then
        local astrocore_avail, astrocore = pcall(require, "astrocore")
        if astrocore_avail then
          vim.schedule(
            function()
              astrocore.notify(
                "As versões fixadas dos plugins do core podem ter mudado.\nRode `:Lazy update` de novo para receber as atualizações.",
                vim.log.levels.WARN
              )
            end
          )
        end
      end
    end,
    priority = 10000,
    lazy = false,
  },
  { import = "astronvim.lazy_snapshot", cond = astronvim.config.pin_plugins },
  {
    "AstroNvim/astrocore",
    dependencies = { "AstroNvim/astroui" },
    opts_extend = { "treesitter.ensure_installed" },
    lazy = false,
    priority = 10000,
    opts = function(_, opts)
      local get_icon = require("astroui").get_icon
      return require("astrocore").extend_tbl(opts, {
        features = {
          large_buf = {
            enabled = function(bufnr) return require("astrocore.buffer").is_valid(bufnr) end,
            notify = true,
            size = 1.5 * 1024 * 1024,
            lines = 100000,
            line_length = 1000,
          },
          autopairs = true, -- enable autopairs at start
          cmp = true, -- enable completion at start
          diagnostics = true, -- enable diagnostics by default
          highlighturl = true, -- highlight URLs by default
          notifications = true, -- enable notifications
        },
        diagnostics = {
          virtual_text = true,
          signs = {
            text = {
              [vim.diagnostic.severity.ERROR] = get_icon "DiagnosticError",
              [vim.diagnostic.severity.HINT] = get_icon "DiagnosticHint",
              [vim.diagnostic.severity.WARN] = get_icon "DiagnosticWarn",
              [vim.diagnostic.severity.INFO] = get_icon "DiagnosticInfo",
            },
          },
          update_in_insert = false,
          underline = true,
          severity_sort = true,
          float = {
            source = "if_many",
            header = "",
            prefix = "",
          },
          jump = {
            -- TODO: remove when dropping support for Neovim v0.11
            float = vim.fn.has "nvim-0.12" == 0 and true or nil,
            on_jump = function(_, bufnr) vim.diagnostic.open_float { bufnr = bufnr, scope = "cursor", focus = false } end,
          },
        },
        rooter = {
          enabled = true,
          detector = { "lsp", { ".git", "_darcs", ".hg", ".bzr", ".svn" }, { "lua", "Makefile", "package.json" } },
          ignore = {
            servers = {},
            dirs = {},
          },
          autochdir = false,
          scope = "global",
          notify = false,
        },
        sessions = {
          autosave = { last = true, cwd = true },
          ignore = {
            dirs = {},
            filetypes = { "gitcommit", "gitrebase" },
            buftypes = { "nofile" },
          },
        },
        treesitter = {
          enabled = function(_, bufnr) return not require("astrocore.buffer").is_large(bufnr) end,
          highlight = true,
          indent = true,
          auto_install = true,
          ensure_installed = { "bash", "c", "lua", "markdown", "markdown_inline", "python", "query", "vim", "vimdoc" },
          textobjects = {
            select = {
              select_textobject = {
                ["ak"] = { query = "@block.outer", desc = "ao redor do bloco" },
                ["ik"] = { query = "@block.inner", desc = "dentro do bloco" },
                ["ac"] = { query = "@class.outer", desc = "ao redor da classe" },
                ["ic"] = { query = "@class.inner", desc = "dentro da classe" },
                ["a?"] = { query = "@conditional.outer", desc = "ao redor da condicional" },
                ["i?"] = { query = "@conditional.inner", desc = "dentro da condicional" },
                ["af"] = { query = "@function.outer", desc = "ao redor da função" },
                ["if"] = { query = "@function.inner", desc = "dentro da função" },
                ["ao"] = { query = "@loop.outer", desc = "ao redor do loop" },
                ["io"] = { query = "@loop.inner", desc = "dentro do loop" },
                ["aa"] = { query = "@parameter.outer", desc = "ao redor do argumento" },
                ["ia"] = { query = "@parameter.inner", desc = "dentro do argumento" },
              },
            },
            move = {
              goto_next_start = {
                ["]k"] = { query = "@block.outer", desc = "Próximo início de bloco" },
                ["]f"] = { query = "@function.outer", desc = "Próximo início de função" },
                ["]a"] = { query = "@parameter.inner", desc = "Próximo início de argumento" },
              },
              goto_next_end = {
                ["]K"] = { query = "@block.outer", desc = "Próximo fim de bloco" },
                ["]F"] = { query = "@function.outer", desc = "Próximo fim de função" },
                ["]A"] = { query = "@parameter.inner", desc = "Próximo fim de argumento" },
              },
              goto_previous_start = {
                ["[k"] = { query = "@block.outer", desc = "Início de bloco anterior" },
                ["[f"] = { query = "@function.outer", desc = "Início de função anterior" },
                ["[a"] = { query = "@parameter.inner", desc = "Início de argumento anterior" },
              },
              goto_previous_end = {
                ["[K"] = { query = "@block.outer", desc = "Fim de bloco anterior" },
                ["[F"] = { query = "@function.outer", desc = "Fim de função anterior" },
                ["[A"] = { query = "@parameter.inner", desc = "Fim de argumento anterior" },
              },
            },
            swap = {
              swap_next = {
                [">K"] = { query = "@block.outer", desc = "Trocar com próximo bloco" },
                [">F"] = { query = "@function.outer", desc = "Trocar com próxima função" },
                [">A"] = { query = "@parameter.inner", desc = "Trocar com próximo argumento" },
              },
              swap_previous = {
                ["<K"] = { query = "@block.outer", desc = "Trocar com bloco anterior" },
                ["<F"] = { query = "@function.outer", desc = "Trocar com função anterior" },
                ["<A"] = { query = "@parameter.inner", desc = "Trocar com argumento anterior" },
              },
            },
          },
        },
      } --[[@as AstroCoreOpts]])
    end,
  },
}
