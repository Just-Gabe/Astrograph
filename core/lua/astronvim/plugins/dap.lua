return {
  "mfussenegger/nvim-dap",
  lazy = true,
  specs = {
    {
      "AstroNvim/astrocore",
      opts = function(_, opts)
        local get_icon = require("astroui").get_icon
        local maps = opts.mappings
        maps.n["<Leader>d"] = vim.tbl_get(opts, "_map_sections", "d")
        -- modified function keys found with `showkey -a` in the terminal to get key code
        -- run `nvim -V3log +quit` and search through the "Terminal info" in the `log` file for the correct keyname
        maps.n["<F5>"] = { function() require("dap").continue() end, desc = "Debugger: iniciar" }
        maps.n["<F17>"] = { function() require("dap").terminate() end, desc = "Debugger: parar" } -- Shift+F5
        maps.n["<F21>"] = { -- Shift+F9
          function()
            vim.ui.input({ prompt = "Condition: " }, function(condition)
              if condition then require("dap").set_breakpoint(condition) end
            end)
          end,
          desc = "Debugger: breakpoint condicional",
        }
        maps.n["<F29>"] = { function() require("dap").restart_frame() end, desc = "Debugger: reiniciar" } -- Control+F5
        maps.n["<F6>"] = { function() require("dap").pause() end, desc = "Debugger: pausar" }
        maps.n["<F9>"] = { function() require("dap").toggle_breakpoint() end, desc = "Debugger: alternar breakpoint" }
        maps.n["<F10>"] = { function() require("dap").step_over() end, desc = "Debugger: pular" }
        maps.n["<F11>"] = { function() require("dap").step_into() end, desc = "Debugger: entrar" }
        maps.n["<F23>"] = { function() require("dap").step_out() end, desc = "Debugger: sair" } -- Shift+F11
        maps.n["<Leader>db"] = { function() require("dap").toggle_breakpoint() end, desc = "Alternar breakpoint (F9)" }
        maps.n["<Leader>dB"] = { function() require("dap").clear_breakpoints() end, desc = "Limpar breakpoints" }
        maps.n["<Leader>dc"] = { function() require("dap").continue() end, desc = "Iniciar/continuar (F5)" }
        maps.n["<Leader>dC"] = {
          function()
            vim.ui.input({ prompt = "Condition: " }, function(condition)
              if condition then require("dap").set_breakpoint(condition) end
            end)
          end,
          desc = "Breakpoint condicional (S-F9)",
        }
        maps.n["<Leader>di"] = { function() require("dap").step_into() end, desc = "Entrar (F11)" }
        maps.n["<Leader>do"] = { function() require("dap").step_over() end, desc = "Pular (F10)" }
        maps.n["<Leader>dO"] = { function() require("dap").step_out() end, desc = "Sair (S-F11)" }
        maps.n["<Leader>dq"] = { function() require("dap").close() end, desc = "Fechar sessão" }
        maps.n["<Leader>dQ"] = { function() require("dap").terminate() end, desc = "Encerrar sessão (S-F5)" }
        maps.n["<Leader>dp"] = { function() require("dap").pause() end, desc = "Pausar (F6)" }
        maps.n["<Leader>dr"] = { function() require("dap").restart_frame() end, desc = "Reiniciar (C-F5)" }
        maps.n["<Leader>dR"] = { function() require("dap").repl.toggle() end, desc = "Alternar REPL" }
        maps.n["<Leader>ds"] = { function() require("dap").run_to_cursor() end, desc = "Executar até o cursor" }

        if not opts.signs then opts.signs = {} end
        opts.signs.DapBreakpoint = { text = get_icon "DapBreakpoint", texthl = "DiagnosticInfo" }
        opts.signs.DapBreakpointCondition = { text = get_icon "DapBreakpointCondition", texthl = "DiagnosticInfo" }
        opts.signs.DapBreakpointRejected = { text = get_icon "DapBreakpointRejected", texthl = "DiagnosticError" }
        opts.signs.DapLogPoint = { text = get_icon "DapLogPoint", texthl = "DiagnosticInfo" }
        opts.signs.DapStopped = { text = get_icon "DapStopped", texthl = "DiagnosticWarn" }
      end,
    },
  },
  dependencies = {
    {
      "jay-babu/mason-nvim-dap.nvim",
      dependencies = { "nvim-dap", "mason-org/mason.nvim" },
      cmd = { "DapInstall", "DapUninstall" },
      opts_extend = { "ensure_installed" },
      opts = { ensure_installed = {}, handlers = {} },
      config = function(...) require "astronvim.plugins.configs.mason-nvim-dap"(...) end,
    },
    {
      "rcarriga/nvim-dap-ui",
      lazy = true,
      specs = {
        {
          "AstroNvim/astrocore",
          opts = function(_, opts)
            local maps = opts.mappings
            maps.n["<Leader>d"] = vim.tbl_get(opts, "_map_sections", "d")
            maps.v["<Leader>d"] = vim.tbl_get(opts, "_map_sections", "d")
            maps.n["<Leader>dE"] = {
              function()
                vim.ui.input({ prompt = "Expression: " }, function(expr)
                  if expr then require("dapui").eval(expr, { enter = true }) end
                end)
              end,
              desc = "Avaliar entrada",
            }
            maps.n["<Leader>du"] = { function() require("dapui").toggle() end, desc = "Alternar interface do debugger" }
            maps.n["<Leader>dh"] = { function() require("dap.ui.widgets").hover() end, desc = "Debugger: inspecionar" }
            maps.v["<Leader>dE"] = { function() require("dapui").eval() end, desc = "Avaliar seleção" }
          end,
        },
      },
      dependencies = { { "nvim-neotest/nvim-nio", lazy = true } },
      opts = {},
      config = function(...) require "astronvim.plugins.configs.nvim-dap-ui"(...) end,
    },
    {
      "rcarriga/cmp-dap",
      lazy = true,
      config = function(...) require "astronvim.plugins.configs.cmp-dap"(...) end,
      specs = {
        {
          "saghen/blink.cmp",
          optional = true,
          specs = { { "saghen/blink.compat", lazy = true, opts = {} } },
          opts = {
            sources = {
              providers = {
                dap = {
                  name = "dap",
                  module = "blink.compat.source",
                  score_offset = 100,
                },
              },
            },
          },
        },
      },
    },
  },
  config = function() require "astronvim.plugins.configs.nvim-dap"() end,
}
