return {
  "AstroNvim/astrocore",
  ---@param opts AstroCoreOpts
  opts = function(_, opts)
    local astro = require "astrocore"
    local get_icon = require("astroui").get_icon
    -- initialize internally used mapping section titles
    opts._map_sections = {
      f = { desc = get_icon("Search", 1, true) .. "Buscar" },
      p = { desc = get_icon("Package", 1, true) .. "Pacotes" },
      l = { desc = get_icon("ActiveLSP", 1, true) .. "Ferramentas de linguagem" },
      u = { desc = get_icon("Window", 1, true) .. "Interface" },
      b = { desc = get_icon("Tab", 1, true) .. "Buffers" },
      bs = { desc = get_icon("Sort", 1, true) .. "Ordenar buffers" },
      d = { desc = get_icon("Debugger", 1, true) .. "Debugger" },
      g = { desc = get_icon("Git", 1, true) .. "Git" },
      S = { desc = get_icon("Session", 1, true) .. "Sessão" },
      t = { desc = get_icon("Terminal", 1, true) .. "Terminal" },
      x = { desc = get_icon("List", 1, true) .. "Quickfix/Listas" },
    }

    -- initialize mappings table
    local maps = astro.empty_map_table()
    local sections = assert(opts._map_sections)

    -- Normal --
    -- Standard Operations
    maps.n["j"] = { "v:count == 0 ? 'gj' : 'j'", expr = true, silent = true, desc = "Mover cursor para baixo" }
    maps.x["j"] = maps.n["j"]
    maps.n["k"] = { "v:count == 0 ? 'gk' : 'k'", expr = true, silent = true, desc = "Mover cursor para cima" }
    maps.x["k"] = maps.n["k"]
    maps.n["<Leader>w"] = { "<Cmd>w<CR>", desc = "Salvar" }
    maps.n["<Leader>q"] = { "<Cmd>confirm q<CR>", desc = "Fechar janela" }
    maps.n["<Leader>Q"] = { "<Cmd>confirm qall<CR>", desc = "Sair do Astrograph" }
    maps.n["<Leader>n"] = { "<Cmd>enew<CR>", desc = "Novo arquivo" }
    maps.n["<C-S>"] = { "<Cmd>silent! update! | redraw<CR>", desc = "Forçar salvar" }
    maps.x["<C-S>"] = maps.n["<C-S>"]
    maps.n["<C-Q>"] = { "<Cmd>q!<CR>", desc = "Forçar saída" }
    maps.n["|"] = { "<Cmd>vsplit<CR>", desc = "Dividir na vertical" }
    maps.n["\\"] = { "<Cmd>split<CR>", desc = "Dividir na horizontal" }
    maps.n["<Leader>/"] = { "gcc", remap = true, desc = "Comentar/descomentar linha" }
    maps.x["<Leader>/"] = { "gc", remap = true, desc = "Comentar/descomentar" }

    -- TODO: remove check when dropping support for Neovim v0.12
    if vim.fn.has "nvim-0.13" == 1 then
      maps.n["<C-BS>"] = {
        function() vim.api.nvim_buf_clear_namespace(0, vim.api.nvim_create_namespace "nvim.multicursor", 0, -1) end,
        desc = "Limpar multicursores",
      }
    end

    maps.n["gco"] = { "o<esc>Vcx<esc><cmd>normal gcc<cr>fxa<bs>", desc = "Comentário abaixo" }
    maps.n["gcO"] = { "O<esc>Vcx<esc><cmd>normal gcc<cr>fxa<bs>", desc = "Comentário acima" }

    maps.n["<Leader>R"] = { function() require("astrocore").rename_file() end, desc = "Renomear arquivo" }

    -- Plugin Manager
    maps.n["<Leader>p"] = vim.tbl_get(sections, "p")
    maps.n["<Leader>pi"] = { function() require("lazy").install() end, desc = "Plugins: instalar" }
    maps.n["<Leader>ps"] = { function() require("lazy").home() end, desc = "Plugins: status" }
    maps.n["<Leader>pS"] = { function() require("lazy").sync() end, desc = "Plugins: sincronizar" }
    maps.n["<Leader>pu"] = { function() require("lazy").check() end, desc = "Plugins: verificar atualizações" }
    maps.n["<Leader>pU"] = { function() require("lazy").update() end, desc = "Plugins: atualizar" }
    maps.n["<Leader>pa"] = { function() require("astrocore").update_packages() end, desc = "Atualizar Lazy e Mason" }

    -- Manage Buffers
    maps.n["<Leader>c"] = { function() require("astrocore.buffer").close() end, desc = "Fechar buffer" }
    maps.n["<Leader>C"] = { function() require("astrocore.buffer").close(0, true) end, desc = "Forçar fechar buffer" }
    maps.n["]b"] = {
      function() require("astrocore.buffer").nav(vim.v.count1) end,
      desc = "Próximo buffer",
    }
    maps.n["[b"] = {
      function() require("astrocore.buffer").nav(-vim.v.count1) end,
      desc = "Buffer anterior",
    }
    maps.n[">b"] = {
      function() require("astrocore.buffer").move(vim.v.count1) end,
      desc = "Mover aba do buffer para a direita",
    }
    maps.n["<b"] = {
      function() require("astrocore.buffer").move(-vim.v.count1) end,
      desc = "Mover aba do buffer para a esquerda",
    }

    maps.n["<Leader>b"] = vim.tbl_get(sections, "b")
    maps.n["<Leader>bc"] =
      { function() require("astrocore.buffer").close_all(true) end, desc = "Fechar todos os buffers exceto o atual" }
    maps.n["<Leader>bC"] = { function() require("astrocore.buffer").close_all() end, desc = "Fechar todos os buffers" }
    maps.n["<Leader>bl"] =
      { function() require("astrocore.buffer").close_left() end, desc = "Fechar buffers à esquerda" }
    maps.n["<Leader>bp"] = { function() require("astrocore.buffer").prev() end, desc = "Buffer anterior" }
    maps.n["<Leader>br"] =
      { function() require("astrocore.buffer").close_right() end, desc = "Fechar buffers à direita" }
    maps.n["<Leader>bs"] = vim.tbl_get(sections, "bs")
    maps.n["<Leader>bse"] = { function() require("astrocore.buffer").sort "extension" end, desc = "Por extensão" }
    maps.n["<Leader>bsr"] = { function() require("astrocore.buffer").sort "unique_path" end, desc = "Por caminho relativo" }
    maps.n["<Leader>bsp"] = { function() require("astrocore.buffer").sort "full_path" end, desc = "Por caminho completo" }
    maps.n["<Leader>bsi"] = { function() require("astrocore.buffer").sort "bufnr" end, desc = "Por número do buffer" }
    maps.n["<Leader>bsm"] = { function() require("astrocore.buffer").sort "modified" end, desc = "Por modificação" }

    maps.n["<Leader>l"] = vim.tbl_get(sections, "l")
    maps.n["<Leader>li"] = { function() vim.cmd.checkhealth "vim.lsp" end, desc = "Informações do LSP" }
    maps.n["<Leader>ld"] = { function() vim.diagnostic.open_float() end, desc = "Ver diagnóstico" }
    local function diagnostic_jump(dir, severity)
      local jump_opts = {}
      if type(severity) == "string" then jump_opts.severity = vim.diagnostic.severity[severity] end
      return function()
        jump_opts.count = dir and vim.v.count1 or -vim.v.count1
        vim.diagnostic.jump(jump_opts)
      end
    end
    maps.n["[e"] = { diagnostic_jump(false, "ERROR"), desc = "Erro anterior" }
    maps.n["]e"] = { diagnostic_jump(true, "ERROR"), desc = "Próximo erro" }
    maps.n["[w"] = { diagnostic_jump(false, "WARN"), desc = "Aviso anterior" }
    maps.n["]w"] = { diagnostic_jump(true, "WARN"), desc = "Próximo aviso" }
    maps.n["gl"] = { function() vim.diagnostic.open_float() end, desc = "Ver diagnóstico" }

    -- Navigate tabs
    maps.n["]t"] = { function() vim.cmd.tabnext() end, desc = "Próxima aba" }
    maps.n["[t"] = { function() vim.cmd.tabprevious() end, desc = "Aba anterior" }

    -- Split navigation
    maps.n["<C-H>"] = { "<C-w>h", desc = "Ir para a divisão da esquerda" }
    maps.n["<C-J>"] = { "<C-w>j", desc = "Ir para a divisão de baixo" }
    maps.n["<C-K>"] = { "<C-w>k", desc = "Ir para a divisão de cima" }
    maps.n["<C-L>"] = { "<C-w>l", desc = "Ir para a divisão da direita" }
    maps.n["<C-Up>"] = { "<Cmd>resize -2<CR>", desc = "Redimensionar para cima" }
    maps.n["<C-Down>"] = { "<Cmd>resize +2<CR>", desc = "Redimensionar para baixo" }
    maps.n["<C-Left>"] = { "<Cmd>vertical resize -2<CR>", desc = "Redimensionar para a esquerda" }
    maps.n["<C-Right>"] = { "<Cmd>vertical resize +2<CR>", desc = "Redimensionar para a direita" }

    -- List management
    maps.n["<Leader>x"] = vim.tbl_get(sections, "x")
    maps.n["<Leader>xq"] = { "<Cmd>copen<CR>", desc = "Lista quickfix" }
    maps.n["<Leader>xl"] = { "<Cmd>lopen<CR>", desc = "Lista de locais" }

    -- Stay in indent mode
    maps.v["<S-Tab>"] = { "<gv", desc = "Desindentar linha" }
    maps.v["<Tab>"] = { ">gv", desc = "Indentar linha" }

    -- Improved Terminal Navigation
    local function term_nav(dir)
      return function()
        if vim.api.nvim_win_get_config(0).zindex then
          vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<C-" .. dir .. ">", true, false, true), "n", false)
        else
          vim.cmd.wincmd(dir)
        end
      end
    end
    maps.t["<C-H>"] = { term_nav "h", desc = "Terminal: ir para janela da esquerda" }
    maps.t["<C-J>"] = { term_nav "j", desc = "Terminal: ir para janela de baixo" }
    maps.t["<C-K>"] = { term_nav "k", desc = "Terminal: ir para janela de cima" }
    maps.t["<C-L>"] = { term_nav "l", desc = "Terminal: ir para janela da direita" }

    maps.n["<Leader>u"] = vim.tbl_get(sections, "u")
    -- Custom menu for modification of the user experience
    maps.n["<Leader>uA"] = { function() require("astrocore.toggles").autochdir() end, desc = "Alternar troca automática de pasta" }
    maps.n["<Leader>ub"] = { function() require("astrocore.toggles").background() end, desc = "Alternar fundo claro/escuro" }
    maps.n["<Leader>ud"] = { function() require("astrocore.toggles").diagnostics() end, desc = "Alternar diagnósticos" }
    maps.n["<Leader>ug"] = { function() require("astrocore.toggles").signcolumn() end, desc = "Alternar coluna de sinais" }
    maps.n["<Leader>u>"] = { function() require("astrocore.toggles").foldcolumn() end, desc = "Alternar coluna de dobras" }
    maps.n["<Leader>ui"] = { function() require("astrocore.toggles").indent() end, desc = "Alterar indentação" }
    maps.n["<Leader>ul"] = { function() require("astrocore.toggles").statusline() end, desc = "Alternar barra de status" }
    maps.n["<Leader>un"] = { function() require("astrocore.toggles").number() end, desc = "Alterar numeração de linhas" }
    maps.n["<Leader>uN"] =
      { function() require("astrocore.toggles").notifications() end, desc = "Alternar notificações" }
    maps.n["<Leader>up"] = { function() require("astrocore.toggles").paste() end, desc = "Alternar modo colar" }
    maps.n["<Leader>us"] = { function() require("astrocore.toggles").spell() end, desc = "Alternar corretor ortográfico" }
    maps.n["<Leader>uS"] = { function() require("astrocore.toggles").conceal() end, desc = "Alternar conceal" }
    maps.n["<Leader>ut"] = { function() require("astrocore.toggles").tabline() end, desc = "Alternar tabline" }
    maps.n["<Leader>uu"] = { function() require("astrocore.toggles").url_match() end, desc = "Alternar destaque de URLs" }
    maps.n["<Leader>uv"] = { function() require("astrocore.toggles").virtual_text() end, desc = "Alternar texto virtual" }
    maps.n["<Leader>uV"] =
      { function() require("astrocore.toggles").virtual_lines() end, desc = "Alternar linhas virtuais" }
    maps.n["<Leader>uw"] = { function() require("astrocore.toggles").wrap() end, desc = "Alternar quebra de linha" }
    maps.n["<Leader>uy"] =
      { function() require("astrocore.toggles").buffer_syntax() end, desc = "Alternar destaque de sintaxe (buffer)" }

    opts.mappings = maps
  end,
}
