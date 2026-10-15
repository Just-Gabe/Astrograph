-- TODO: Remove this compatibility alias when support for Neovim older than v0.13 is dropped.
local buf_key = vim.fn.has "nvim-0.13" == 1 and "buf" or "buffer"

return {
  "akinsho/toggleterm.nvim",
  cmd = { "ToggleTerm", "TermExec" },
  specs = {
    {
      "AstroNvim/astrocore",
      opts = function(_, opts)
        local maps = opts.mappings
        local astro = require "astrocore"
        maps.n["<Leader>t"] = vim.tbl_get(opts, "_map_sections", "t")
        -- Astrograph: os atalhos que dependem de binários externos (lazygit, node,
        -- gdu, btm, python) são registrados logo após o startup, um binário por
        -- tick. No Windows cada `executable()` de um binário ausente custa ~20ms,
        -- o que somava ~100ms ao boot. Overrides do usuário (inclusive `false`)
        -- continuam valendo: chaves já definidas na config final são puladas.
        local function optional_maps()
          local exe = require("astronvim").executable
          local checks = {
            function(m)
              if exe "git" and exe "lazygit" then
                require("astronvim.lazygit").enable()
                m.n["<Leader>g"] = vim.tbl_get(opts, "_map_sections", "g")
                local function lazygit()
                  local worktree = astro.file_worktree()
                  local flags = worktree
                      and (" --work-tree=%s --git-dir=%s"):format(
                        vim.fn.shellescape(worktree.toplevel),
                        vim.fn.shellescape(worktree.gitdir)
                      )
                    or ""
                  astro.toggle_term_cmd { cmd = "lazygit " .. flags, direction = "float" }
                end
                m.n["<Leader>gg"] = { lazygit, desc = "Terminal: lazygit" }
                m.n["<Leader>tl"] = { lazygit, desc = "Terminal: lazygit" }
              end
            end,
            function(m)
              if exe "node" then
                m.n["<Leader>tn"] = { function() astro.toggle_term_cmd "node" end, desc = "Terminal: node" }
              end
            end,
            function(m)
              local gdu = "gdu"
              if not exe(gdu) then
                if vim.fn.has "win32" == 1 then
                  gdu = "gdu_windows_amd64.exe"
                elseif vim.fn.has "mac" == 1 then
                  gdu = "gdu-go"
                end
              end
              if exe(gdu) then
                m.n["<Leader>tu"] =
                  { function() astro.toggle_term_cmd { cmd = gdu, direction = "float" } end, desc = "Terminal: gdu" }
              end
            end,
            function(m)
              if exe "btm" then
                m.n["<Leader>tt"] =
                  { function() astro.toggle_term_cmd { cmd = "btm", direction = "float" } end, desc = "Terminal: btm" }
              end
            end,
            function(m)
              local python = exe "python" and "python" or exe "python3" and "python3"
              if python then
                m.n["<Leader>tp"] = { function() astro.toggle_term_cmd(python) end, desc = "Terminal: python" }
              end
            end,
          }
          local function step(i)
            local check = checks[i]
            if not check then return end
            local m = astro.empty_map_table()
            check(m)
            local final = astro.config.mappings or {}
            for mode, mode_maps in pairs(m) do
              for lhs in pairs(mode_maps) do
                if vim.tbl_get(final, mode, lhs) ~= nil then mode_maps[lhs] = nil end
              end
            end
            astro.set_mappings(m)
            vim.defer_fn(function() step(i + 1) end, 30)
          end
          step(1)
        end
        vim.api.nvim_create_autocmd("User", {
          pattern = "VeryLazy",
          once = true,
          desc = "Register external-tool terminal mappings",
          -- depois da animacao de entrada da tela inicial (~450ms): cada
          -- `executable()` de um binario ausente trava ~20ms no Windows
          callback = function() vim.defer_fn(optional_maps, 700) end,
        })
        maps.n["<Leader>tf"] = { "<Cmd>ToggleTerm direction=float<CR>", desc = "Terminal flutuante" }
        maps.n["<Leader>th"] =
          { "<Cmd>ToggleTerm size=10 direction=horizontal<CR>", desc = "Terminal na horizontal" }
        maps.n["<Leader>tv"] = { "<Cmd>ToggleTerm size=80 direction=vertical<CR>", desc = "Terminal na vertical" }
        maps.n["<F7>"] = { '<Cmd>execute v:count . "ToggleTerm"<CR>', desc = "Alternar terminal" }
        maps.t["<F7>"] = { "<Cmd>ToggleTerm<CR>", desc = "Alternar terminal" }
        maps.i["<F7>"] = { "<Esc><Cmd>ToggleTerm<CR>", desc = "Alternar terminal" }
        maps.n["<C-'>"] = { '<Cmd>execute v:count . "ToggleTerm"<CR>', desc = "Alternar terminal" } -- requires terminal that supports binding <C-'>
        maps.t["<C-'>"] = { "<Cmd>ToggleTerm<CR>", desc = "Alternar terminal" } -- requires terminal that supports binding <C-'>
        maps.i["<C-'>"] = { "<Esc><Cmd>ToggleTerm<CR>", desc = "Alternar terminal" } -- requires terminal that supports binding <C-'>
      end,
    },
    {
      "nvim-neo-tree/neo-tree.nvim",
      optional = true,
      opts = function(_, opts)
        if not opts.commands then opts.commands = {} end
        if not opts.window then opts.window = {} end
        if not opts.window.mappings then opts.window.mappings = {} end

        local function toggleterm_in_direction(state, direction)
          local node = state.tree:get_node()
          local path = node.type == "file" and node:get_parent_id() or node:get_id()
          require("toggleterm.terminal").Terminal:new({ dir = path, direction = direction }):toggle()
        end
        local prefix = "T"
        ---@diagnostic disable-next-line: assign-type-mismatch
        opts.window.mappings[prefix] =
          { "show_help", nowait = false, config = { title = "New Terminal", prefix_key = prefix } }
        for suffix, direction in pairs { f = "float", h = "horizontal", v = "vertical" } do
          local command = "toggleterm_" .. direction
          opts.commands[command] = function(state) toggleterm_in_direction(state, direction) end
          opts.window.mappings[prefix .. suffix] = command
        end
      end,
    },
  },
  opts = {
    highlights = {
      Normal = { link = "Normal" },
      NormalNC = { link = "NormalNC" },
      NormalFloat = { link = "NormalFloat" },
      FloatBorder = { link = "FloatBorder" },
      StatusLine = { link = "StatusLine" },
      StatusLineNC = { link = "StatusLineNC" },
      WinBar = { link = "WinBar" },
      WinBarNC = { link = "WinBarNC" },
    },
    size = 10,
    ---@param t Terminal
    on_create = function(t)
      vim.opt_local.foldcolumn = "0"
      vim.opt_local.signcolumn = "no"
      if t.hidden then
        local function toggle() t:toggle() end
        vim.keymap.set({ "n", "t", "i" }, "<C-'>", toggle, { desc = "Alternar terminal", [buf_key] = t.bufnr })
        vim.keymap.set({ "n", "t", "i" }, "<F7>", toggle, { desc = "Alternar terminal", [buf_key] = t.bufnr })
      end
    end,
    shading_factor = 2,
  },
}
