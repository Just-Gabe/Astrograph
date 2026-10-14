return {
  "folke/snacks.nvim",
  lazy = false,
  priority = 1000,
  opts = function(_, opts)
    local get_icon = require("astroui").get_icon
    local buf_utils = require "astrocore.buffer"

    opts.dashboard = {
      preset = {
        keys = {
          { key = "n", action = "<Leader>n", icon = get_icon("FileNew", 0, true), desc = "Novo arquivo  " },
          { key = "f", action = "<Leader>ff", icon = get_icon("Search", 0, true), desc = "Buscar arquivo  " },
          { key = "o", action = "<Leader>fo", icon = get_icon("DefaultFile", 0, true), desc = "Recentes  " },
          { key = "w", action = "<Leader>fw", icon = get_icon("WordFile", 0, true), desc = "Buscar palavra  " },
          { key = "'", action = "<Leader>f'", icon = get_icon("Bookmarks", 0, true), desc = "Marcadores  " },
          { key = "s", action = "<Leader>Sl", icon = get_icon("Refresh", 0, true), desc = "Última sessão  " },
        },
        header = table.concat({
          " █████  ███████ ████████ ██████   ██████ ",
          "██   ██ ██         ██    ██   ██ ██    ██",
          "███████ ███████    ██    ██████  ██    ██",
          "██   ██      ██    ██    ██   ██ ██    ██",
          "██   ██ ███████    ██    ██   ██  ██████ ",
          "",
          "███    ██ ██    ██ ██ ███    ███",
          "████   ██ ██    ██ ██ ████  ████",
          "██ ██  ██ ██    ██ ██ ██ ████ ██",
          "██  ██ ██  ██  ██  ██ ██  ██  ██",
          "██   ████   ████   ██ ██      ██",
        }, "\n"),
      },
      sections = {
        { section = "header", padding = 5 },
        { section = "keys", gap = 1, padding = 3 },
        { section = "startup" },
      },
    }

    -- configure image support
    opts.image = { doc = { enabled = false } }

    -- configure `vim.ui.input`
    opts.input = {}

    -- configure notifier
    opts.notifier = {}
    opts.notifier.icons = {
      debug = get_icon "Debugger",
      error = get_icon "DiagnosticError",
      info = get_icon "DiagnosticInfo",
      trace = get_icon "DiagnosticHint",
      warn = get_icon "DiagnosticWarn",
    }

    -- configure picker and `vim.ui.select`
    opts.picker = { ui_select = true }

    opts.indent = {
      indent = { char = "▏" },
      scope = { char = "▏" },
      filter = function(bufnr)
        return buf_utils.is_valid(bufnr)
          and not buf_utils.is_large(bufnr)
          and vim.g.snacks_indent ~= false
          and vim.b[bufnr].snacks_indent ~= false
      end,
      animate = { enabled = false },
    }

    opts.scope = {
      filter = function(bufnr)
        return buf_utils.is_valid(bufnr)
          and not buf_utils.is_large(bufnr)
          and vim.g.snacks_scope ~= false
          and vim.b[bufnr].snacks_scope ~= false
      end,
    }

    opts.words = {
      enabled = true,
      filter = function(bufnr)
        return buf_utils.is_valid(bufnr)
          and not buf_utils.is_large(bufnr)
          and vim.g.snacks_words ~= false
          and vim.b[bufnr].snacks_words ~= false
      end,
    }

    opts.zen = {
      toggles = { dim = false, diagnostics = false, inlay_hints = false },
      on_open = function(win)
        -- disable snacks indent
        vim.b[win.buf].snacks_indent_old = vim.b[win.buf].snacks_indent
        vim.b[win.buf].snacks_indent = false
      end,
      on_close = function(win)
        -- restore snacks indent setting
        vim.b[win.buf].snacks_indent = vim.b[win.buf].snacks_indent_old
      end,
      win = {
        width = function() return math.min(120, math.floor(vim.o.columns * 0.75)) end,
        height = 0.9,
        backdrop = {
          transparent = false,
          win = { wo = { winhighlight = "Normal:Normal" } },
        },
        wo = {
          number = false,
          relativenumber = false,
          signcolumn = "no",
          foldcolumn = "0",
          winbar = "",
          list = false,
          showbreak = "NONE",
        },
      },
    }
  end,
  specs = {
    {
      "AstroNvim/astrocore",
      opts = function(_, opts)
        local maps = opts.mappings
        local snack_opts = require("astrocore").plugin_opts "snacks.nvim"

        -- Snacks.dashboard mappings
        maps.n["<Leader>h"] = {
          function()
            if vim.bo.filetype == "snacks_dashboard" then
              require("astrocore.buffer").close()
            else
              require("snacks").dashboard()
            end
          end,
          desc = "Tela inicial",
        }

        -- Snacks.indent mappings
        maps.n["<Leader>u|"] =
          { function() require("snacks").toggle.indent():toggle() end, desc = "Alternar guias de indentação" }

        -- Snacks.notifier mappings
        if vim.tbl_get(snack_opts, "notifier", "enabled") ~= false then
          maps.n["<Leader>uD"] = { function() require("snacks.notifier").hide() end, desc = "Dispensar notificações" }
        end

        -- Snacks.gitbrowse mappings
        if vim.tbl_get(snack_opts, "gitbrowse", "enabled") ~= false then
          if require("astronvim").executable "git" then
            maps.n["<Leader>g"] = vim.tbl_get(opts, "_map_sections", "g")
            maps.n["<Leader>go"] = { function() require("snacks").gitbrowse() end, desc = "Git: abrir no navegador" }
            maps.x["<Leader>go"] = { function() require("snacks").gitbrowse() end, desc = "Git: abrir no navegador" }
          end
        end

        -- Snacks.picker
        if vim.tbl_get(snack_opts, "picker", "enabled") ~= false then
          maps.n["<Leader>f"] = vim.tbl_get(opts, "_map_sections", "f")
          if require("astronvim").executable "git" then
            maps.n["<Leader>g"] = vim.tbl_get(opts, "_map_sections", "g")
            maps.n["<Leader>gb"] = { function() require("snacks").picker.git_branches() end, desc = "Git: branches" }
            maps.n["<Leader>gc"] = {
              function() require("snacks").picker.git_log() end,
              desc = "Git: commits (repositório)",
            }
            maps.n["<Leader>gC"] = {
              function() require("snacks").picker.git_log { current_file = true, follow = true } end,
              desc = "Git: commits (arquivo atual)",
            }
            maps.n["<Leader>gt"] = { function() require("snacks").picker.git_status() end, desc = "Git: status" }
            maps.n["<Leader>gT"] = { function() require("snacks").picker.git_stash() end, desc = "Git: stash" }
          end
          maps.n["<Leader>f<CR>"] =
            { function() require("snacks").picker.resume() end, desc = "Retomar última busca" }
          maps.n["<Leader>f'"] = { function() require("snacks").picker.marks() end, desc = "Buscar marcas" }
          maps.n["<Leader>fl"] = {
            function() require("snacks").picker.lines() end,
            desc = "Buscar linhas",
          }
          maps.n["<Leader>fa"] = {
            function() require("snacks").picker.files { dirs = { vim.fn.stdpath "config" }, desc = "Arquivos de config" } end,
            desc = "Buscar arquivos de config do Astrograph",
          }
          maps.n["<Leader>fb"] = { function() require("snacks").picker.buffers() end, desc = "Buscar buffers" }
          maps.n["<Leader>fc"] =
            { function() require("snacks").picker.grep_word() end, desc = "Buscar palavra sob o cursor" }
          maps.n["<Leader>fC"] = { function() require("snacks").picker.commands() end, desc = "Buscar comandos" }
          maps.n["<Leader>ff"] = {
            function()
              require("snacks").picker.files {
                hidden = vim.tbl_get(vim.uv.fs_stat ".git" or {}, "type") == "directory",
              }
            end,
            desc = "Buscar arquivos",
          }
          maps.n["<Leader>fF"] = {
            function() require("snacks").picker.files { hidden = true, ignored = true } end,
            desc = "Buscar todos os arquivos",
          }
          maps.n["<Leader>fg"] = { function() require("snacks").picker.git_files() end, desc = "Buscar arquivos do git" }
          maps.n["<Leader>fh"] = { function() require("snacks").picker.help() end, desc = "Buscar ajuda" }
          maps.n["<Leader>fk"] = { function() require("snacks").picker.keymaps() end, desc = "Buscar atalhos" }
          maps.n["<Leader>fm"] = { function() require("snacks").picker.man() end, desc = "Buscar man pages" }
          maps.n["<Leader>fn"] =
            { function() require("snacks").picker.notifications() end, desc = "Buscar notificações" }
          maps.n["<Leader>fo"] = { function() require("snacks").picker.recent() end, desc = "Arquivos recentes" }
          maps.n["<Leader>fO"] = {
            function() require("snacks").picker.recent { filter = { cwd = true } } end,
            desc = "Arquivos recentes (pasta atual)",
          }
          maps.n["<Leader>fp"] = { function() require("snacks").picker.projects() end, desc = "Buscar projetos" }
          maps.n["<Leader>fr"] = { function() require("snacks").picker.registers() end, desc = "Buscar registradores" }
          maps.n["<Leader>fs"] = { function() require("snacks").picker.smart() end, desc = "Buscar buffers/recentes/arquivos" }
          maps.n["<Leader>ft"] = { function() require("snacks").picker.colorschemes() end, desc = "Buscar temas" }
          if require("astronvim").executable "rg" then
            maps.n["<Leader>fw"] = { function() require("snacks").picker.grep() end, desc = "Buscar texto" }
            maps.n["<Leader>fW"] = {
              function() require("snacks").picker.grep { hidden = true, ignored = true } end,
              desc = "Buscar texto em todos os arquivos",
            }
          end
          maps.n["<Leader>fu"] = { function() require("snacks").picker.undo() end, desc = "Histórico de desfazer" }
          maps.n["<Leader>lD"] = { function() require("snacks").picker.diagnostics() end, desc = "Buscar diagnósticos" }
          maps.n["<Leader>ls"] = {
            function()
              local aerial_avail, aerial = pcall(require, "aerial")
              if aerial_avail and aerial.snacks_picker then
                aerial.snacks_picker()
              else
                require("snacks").picker.lsp_symbols()
              end
            end,
            desc = "Buscar símbolos",
          }
        end

        -- Snacks.words mappings
        if vim.tbl_get(snack_opts, "words", "enabled") ~= false then
          maps.n["<Leader>ur"] =
            { function() require("snacks").toggle.words():toggle() end, desc = "Alternar destaque de referências" }
          maps.n["]r"] = { function() require("snacks").words.jump(vim.v.count1) end, desc = "Próxima referência" }
          maps.n["[r"] = { function() require("snacks").words.jump(-vim.v.count1) end, desc = "Referência anterior" }
        end

        -- Snacks.zen mappings
        if vim.tbl_get(snack_opts, "zen", "enabled") ~= false then
          maps.n["<Leader>uZ"] = { function() require("snacks").toggle.zen():toggle() end, desc = "Alternar modo zen" }
        end
      end,
    },
    {
      "nvim-neo-tree/neo-tree.nvim",
      optional = true,
      opts = {
        commands = {
          find_files_in_dir = function(state)
            local node = state.tree:get_node()
            local path = node.type == "file" and node:get_parent_id() or node:get_id()
            require("snacks").picker.files { cwd = path }
          end,
          find_all_files_in_dir = function(state)
            local node = state.tree:get_node()
            local path = node.type == "file" and node:get_parent_id() or node:get_id()
            require("snacks").picker.files { cwd = path, hidden = true, ignored = true }
          end,
          find_words_in_dir = function(state)
            local node = state.tree:get_node()
            local path = node.type == "file" and node:get_parent_id() or node:get_id()
            require("snacks").picker.grep { cwd = path }
          end,
          find_all_words_in_dir = function(state)
            local node = state.tree:get_node()
            local path = node.type == "file" and node:get_parent_id() or node:get_id()
            require("snacks").picker.grep { cwd = path, hidden = true, ignored = true }
          end,
        },
        filesystem = {
          window = {
            mappings = {
              f = { "show_help", nowait = false, config = { title = "Find Files", prefix_key = "f" } },
              ["f/"] = "filter_on_submit",
              ff = "find_files_in_dir",
              fF = "find_all_files_in_dir",
              fw = require("astronvim").executable "rg" and "find_words_in_dir" or nil,
              fW = require("astronvim").executable "rg" and "find_all_words_in_dir" or nil,
            },
          },
        },
      },
    },
  },
}
