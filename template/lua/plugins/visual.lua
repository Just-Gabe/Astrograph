-- Visual moderno do Astrograph, pensado para o terminal.
--
--   * ui2 (Neovim 0.12+): linha de comando com destaque de sintaxe e mensagens
--     sem "Press ENTER". Desligue com `vim.g.astrograph_ui2 = false`.
--   * Bordas arredondadas em todas as janelas flutuantes (`winborder`).
--   * Completação com bordas arredondadas e ghost text (sugestão em cinza na
--     própria linha, estilo Copilot).
--   * Cursor animado (smear-cursor): um rastro suave ao pular pelo texto. Segue
--     o `<Leader>um` das animações e começa desligado via SSH.
--   * Janelas flutuantes surgem com um fade curto (astrograph.motion).
--   * Linha do cursor destacada só na janela ativa.
--   * Menos ruído: sem `~` no fim do buffer, CodeLens desligado por padrão e
--     uma leve translucidez no menu de completação.

-- Bordas arredondadas por padrão em hovers do LSP, diagnósticos, etc. (0.11+)
if vim.fn.has "nvim-0.11" == 1 then vim.o.winborder = "rounded" end

-- Conteúdo em primeiro lugar: sem `~` depois do fim do arquivo e uma leve
-- translucidez no menu de completação (lembra o vibrancy do macOS).
vim.opt.fillchars:append { eob = " " }
vim.o.pumblend = 8

-- ui2: experimental no 0.12; qualquer falha mantém a interface clássica
if vim.g.astrograph_ui2 ~= false then
  local ok, ui2 = pcall(require, "vim._core.ui2")
  if ok and type(ui2.enable) == "function" then pcall(ui2.enable, { enable = true }) end
end

-- janelas flutuantes surgem com um fade curto (astrograph.motion)
require("astrograph.motion").setup()

-- cursorline só na janela ativa: menos ruído com várias divisões abertas
vim.api.nvim_create_autocmd({ "WinEnter", "BufWinEnter", "WinLeave" }, {
  group = vim.api.nvim_create_augroup("astrograph_cursorline", { clear = true }),
  desc = "Linha do cursor só na janela ativa",
  callback = function(ev)
    local win = vim.api.nvim_get_current_win()
    if vim.api.nvim_win_get_config(win).relative ~= "" then return end
    vim.wo[win].cursorline = ev.event ~= "WinLeave"
  end,
})

---@type LazySpec
return {
  {
    "AstroNvim/astrocore",
    opts = function(_, opts)
      local maps = opts.mappings
      local motion = function() return require "astrograph.motion" end
      -- `dd` apaga na hora e a linha desce e some (com contagem e registrador)
      maps.n["dd"] = { function() motion().delete_lines() end, desc = "Apagar linha (animado)" }
      -- explorador abre e fecha deslizando a largura
      maps.n["<Leader>e"] = { function() motion().toggle_explorer() end, desc = "Alternar explorador" }
    end,
  },
  {
    "nvim-neo-tree/neo-tree.nvim",
    optional = true,
    -- sem polling de largura a cada 50 ms: o redesenho vem do evento WinResized
    -- (astrograph.motion), e o deslize do explorador não corta os nomes
    opts = { resize_timer_interval = -1 },
  },
  {
    "AstroNvim/astrolsp",
    optional = true,
    -- CodeLens ("N references" entre as linhas) começa desligado: menos ruído
    -- no código. <Leader>uL liga quando quiser.
    opts = { features = { codelens = false } },
  },
  {
    "folke/snacks.nvim",
    opts = function(_, opts)
      -- Buscas abrem na hora e os resultados entram conforme chegam (antes a
      -- janela esperava o `fd`/`rg` devolver algo: ~50–200 ms no Windows).
      -- As buscas do LSP ficam como estão: com um resultado só elas pulam
      -- direto para o código, e a janela apenas piscaria.
      opts.picker = opts.picker or {}
      opts.picker.sources = opts.picker.sources or {}
      for _, src in ipairs { "files", "grep", "grep_word", "buffers", "recent", "smart", "git_files", "help", "keymaps", "commands" } do
        opts.picker.sources[src] = vim.tbl_deep_extend("force", opts.picker.sources[src] or {}, { show_delay = 0 })
      end
    end,
  },
  {
    "folke/which-key.nvim",
    optional = true,
    -- menu de atalhos um pouco mais ágil (padrão: 200 ms); plugins seguem imediatos
    opts = { delay = function(ctx) return ctx.plugin and 0 or 150 end },
  },
  {
    "saghen/blink.cmp",
    optional = true,
    opts = {
      completion = {
        menu = { border = "rounded" },
        documentation = { window = { border = "rounded" } },
        -- mostra o 1º item em cinza na própria linha, mesmo sem nada selecionado
        ghost_text = { enabled = true, show_without_selection = true },
      },
      signature = { window = { border = "rounded" } },
    },
  },
  {
    "sphamba/smear-cursor.nvim",
    event = "VeryLazy",
    cond = function() return not (vim.env.SSH_CONNECTION or vim.env.SSH_TTY) end,
    opts = {
      -- rápido e sem "borracha": combina com as animações de 60 fps
      stiffness = 0.8,
      trailing_stiffness = 0.6,
      stiffness_insert_mode = 0.7,
      trailing_stiffness_insert_mode = 0.7,
      damping = 0.95,
      distance_stop_animating = 0.5,
      smear_between_buffers = true,
      smear_between_neighbor_lines = true,
      smear_insert_mode = true,
      scroll_buffer_space = true,
      never_draw_over_target = true,
    },
    config = function(_, opts)
      local smear = require "smear_cursor"
      smear.setup(opts)
      -- respeita o liga/desliga global das animações (<Leader>um)
      if vim.g.snacks_animate == false then smear.enabled = false end
    end,
  },
}
