local astroui_foldexpr = "v:lua.require'astroui.folding'.foldexpr()"
return {
  "AstroNvim/astroui",
  lazy = true,
  specs = {
    {
      "AstroNvim/astrocore",
      opts = {
        autocmds = {
          persistent_astroui_foldexpr = {
            {
              event = "FileType",
              callback = function()
                -- Astrograph: janelas flutuantes (picker, preview, hover) não têm
                -- dobras; redefinir o foldexpr nelas recalculava as dobras do
                -- buffer inteiro e era ~2/3 do tempo de abrir a busca.
                if vim.api.nvim_win_get_config(0).relative ~= "" then return end
                if require("astrocore.buffer").is_valid() then
                  if vim.wo[0][0].foldexpr ~= astroui_foldexpr and vim.go.foldexpr == astroui_foldexpr then
                    vim.wo[0][0].foldexpr = astroui_foldexpr
                  end
                end
              end,
            },
          },
        },
        options = {
          opt = {
            foldcolumn = "1", -- display fold column
            foldenable = true, -- enable folds
            foldexpr = astroui_foldexpr, -- set function for calculating folds
            foldlevel = 99, -- set high foldlevel
            foldmethod = "expr", -- use `foldexpr` for calculating folds
            foldtext = "", -- use transparent foldtext
          },
        },
      },
    },
  },
  ---@type AstroUIOpts
  opts = {
    colorscheme = "astrotheme",
    folding = {
      enabled = function(bufnr) return require("astrocore.buffer").is_valid(bufnr) end,
      methods = { "lsp", "treesitter", "indent" },
    },
    icons = {
      ActiveLSP = "",
      ActiveTS = "",
      ArrowLeft = "",
      ArrowRight = "",
      Bookmarks = "",
      BufferClose = "󰅖",
      DapBreakpoint = "",
      DapBreakpointCondition = "",
      DapBreakpointRejected = "",
      DapLogPoint = "󰛿",
      DapStopped = "󰁕",
      Debugger = "",
      DefaultFile = "󰈙",
      Diagnostic = "󰒡",
      DiagnosticError = "",
      DiagnosticHint = "󰌵",
      DiagnosticInfo = "󰋼",
      DiagnosticWarn = "",
      Ellipsis = "…",
      Environment = "",
      FileNew = "",
      FileModified = "",
      FileReadOnly = "",
      FoldClosed = "",
      FoldOpened = "",
      FoldSeparator = " ",
      FolderClosed = "",
      FolderEmpty = "",
      FolderOpen = "",
      Git = "󰊢",
      GitAdd = "",
      GitBranch = "",
      GitChange = "",
      GitConflict = "",
      GitDelete = "",
      GitIgnored = "◌",
      GitRenamed = "➜",
      GitSign = "▎",
      GitStaged = "✓",
      GitUnstaged = "✗",
      GitUntracked = "★",
      List = "",
      LSPLoading1 = "",
      LSPLoading2 = "󰀚",
      LSPLoading3 = "",
      MacroRecording = "",
      Package = "󰏖",
      Paste = "󰅌",
      Refresh = "",
      Search = "",
      Selected = "❯",
      Session = "󱂬",
      Sort = "󰒺",
      Spellcheck = "󰓆",
      Tab = "󰓩",
      TabClose = "󰅙",
      Terminal = "",
      Window = "",
      WordFile = "󰈭",
    },
    text_icons = {
      ActiveLSP = "LSP:",
      ArrowLeft = "<",
      ArrowRight = ">",
      BufferClose = "x",
      DapBreakpoint = "B",
      DapBreakpointCondition = "C",
      DapBreakpointRejected = "R",
      DapLogPoint = "L",
      DapStopped = ">",
      DefaultFile = "[F]",
      DiagnosticError = "X",
      DiagnosticHint = "?",
      DiagnosticInfo = "i",
      DiagnosticWarn = "!",
      Ellipsis = "...",
      Environment = "Env:",
      FileModified = "*",
      FileReadOnly = "[lock]",
      FoldClosed = "+",
      FoldOpened = "-",
      FoldSeparator = " ",
      FolderClosed = "[D]",
      FolderEmpty = "[E]",
      FolderOpen = "[O]",
      GitAdd = "[+]",
      GitChange = "[/]",
      GitConflict = "[!]",
      GitDelete = "[-]",
      GitIgnored = "[I]",
      GitRenamed = "[R]",
      GitSign = "|",
      GitStaged = "[S]",
      GitUnstaged = "[U]",
      GitUntracked = "[?]",
      MacroRecording = "Recording:",
      Paste = "[PASTE]",
      Search = "?",
      Selected = "*",
      Spellcheck = "[SPELL]",
      TabClose = "X",
    },
    -- Astrograph: o tema do lazygit é ligado depois do startup, e só se ele
    -- estiver instalado (astronvim.lazygit.enable, chamado pelo toggleterm).
    -- Checar o binário aqui custava ~20 ms na leitura da config, e o astroui
    -- regravava o YAML do tema em todo startup.
    lazygit = false,
  },
}
