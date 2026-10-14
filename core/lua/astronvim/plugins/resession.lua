return {
  "stevearc/resession.nvim",
  lazy = true,
  specs = {
    {
      "AstroNvim/astrocore",
      opts = function(_, opts)
        local maps = opts.mappings
        maps.n["<Leader>S"] = vim.tbl_get(opts, "_map_sections", "S")
        maps.n["<Leader>Sl"] = { function() require("resession").load "Last Session" end, desc = "Abrir última sessão" }
        maps.n["<Leader>Ss"] = { function() require("resession").save() end, desc = "Salvar esta sessão" }
        maps.n["<Leader>SS"] = {
          function() require("resession").save(vim.uv.cwd(), { dir = "dirsession" }) end,
          desc = "Salvar sessão desta pasta",
        }
        maps.n["<Leader>St"] = { function() require("resession").save_tab() end, desc = "Salvar sessão desta aba" }
        maps.n["<Leader>Sd"] = { function() require("resession").delete() end, desc = "Apagar sessão" }
        maps.n["<Leader>SD"] =
          { function() require("resession").delete(nil, { dir = "dirsession" }) end, desc = "Apagar sessão de pasta" }
        maps.n["<Leader>Sf"] = { function() require("resession").load() end, desc = "Abrir sessão" }
        maps.n["<Leader>SF"] =
          { function() require("resession").load(nil, { dir = "dirsession" }) end, desc = "Abrir sessão de pasta" }
        maps.n["<Leader>S."] = {
          function() require("resession").load(vim.uv.cwd(), { dir = "dirsession" }) end,
          desc = "Abrir sessão da pasta atual",
        }
        opts.autocmds.resession_auto_save = {
          {
            event = "VimLeavePre",
            desc = "Salvar sessão ao fechar",
            callback = function()
              local buf_utils = require "astrocore.buffer"
              local autosave = require("astrocore").config.sessions.autosave
              if autosave and buf_utils.is_valid_session() then
                local save = require("resession").save
                if autosave.last then save("Last Session", { notify = false }) end
                if autosave.cwd then save(vim.uv.cwd(), { dir = "dirsession", notify = false }) end
              end
            end,
          },
        }
      end,
    },
  },
  opts = {
    buf_filter = function(bufnr) return require("astrocore.buffer").is_restorable(bufnr) end,
    tab_buf_filter = function(tabpage, bufnr) return vim.tbl_contains(vim.t[tabpage].bufs or {}, bufnr) end,
    extensions = { astrocore = { enable_in_tab = true } },
  },
}
