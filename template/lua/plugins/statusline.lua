-- Barra de status e abas no estilo Apple: informação em tons de cinza, uma cor
-- só onde importa.
--
--   * Sem os blocos coloridos de modo nas pontas (a assinatura do AstroNvim): o
--     modo vira um ponto colorido discreto e, fora do Normal, o nome em
--     português ("Inserir", "Visual"…) em cinza.
--   * Posição como "12:4", sem porcentagem nem barrinha de rolagem.
--   * Sem o "TS" do treesitter, sem negrito/itálico chamativo.
--   * O "×" das abas e o ramo do git em cinza (vermelho é para erro).

local MODE_NAMES = {
  insert = "Inserir",
  visual = "Visual",
  replace = "Substituir",
  command = "Comando",
  terminal = "Terminal",
}

---@type LazySpec
return {
  {
    "AstroNvim/astroui",
    ---@param opts AstroUIOpts
    opts = function(_, opts)
      opts.status = opts.status or {}
      opts.status.attributes = vim.tbl_deep_extend("force", opts.status.attributes or {}, {
        buffer_active = { bold = false, italic = false },
        git_branch = { bold = false },
        git_diff = { bold = false },
        virtual_env = { bold = false },
      })
      opts.status.colors = function(c)
        local secondary = c.buffer_fg
        c.git_branch_fg = secondary
        c.treesitter_fg = secondary
        c.close_fg = secondary
        c.tab_close_fg = secondary
        c.buffer_close_fg = c.tabline_bg -- some até passar o mouse/ficar ativa
        c.buffer_active_close_fg = secondary
        c.buffer_visible_close_fg = secondary
        return c
      end
    end,
  },
  {
    "rebelot/heirline.nvim",
    opts = function(_, opts)
      local status = require "astroui.status"
      local mode_dot = {
        init = function(self) self.mode = status.hl.mode_bg() end,
        {
          provider = "  ● ",
          hl = function(self) return { fg = self.mode } end,
        },
        {
          provider = function(self)
            local name = MODE_NAMES[self.mode]
            return name and (name .. "  ") or ""
          end,
          hl = { fg = "buffer_fg" },
        },
        update = {
          "ModeChanged",
          pattern = "*:*",
          callback = vim.schedule_wrap(function() vim.cmd.redrawstatus() end),
        },
      }
      opts.statusline = {
        hl = { fg = "fg", bg = "bg" },
        mode_dot,
        status.component.git_branch(),
        status.component.file_info(),
        status.component.git_diff(),
        status.component.diagnostics(),
        status.component.fill(),
        status.component.cmd_info(),
        status.component.fill(),
        status.component.lsp(),
        status.component.virtual_env(),
        status.component.nav { percentage = false, scrollbar = false, hl = { fg = "buffer_fg" } },
        { provider = "  " },
      }
      return opts
    end,
  },
}
