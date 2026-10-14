-- Dashboard de abertura proprio do Astrograph: substitui apenas o cabecalho
-- (sections[1]) pela cena lua + estrelas + banner. O resto do dashboard do
-- AstroNvim (atalhos, startup, <Leader>h) permanece intacto.
--
-- O core do AstroNvim define `opts.dashboard` depois dos specs do usuario, entao
-- o patch tem que acontecer no `config` (ja com os opts finais), nao no `opts`.
--
-- Implementacao em `astrograph.dashboard`; arte em `astrograph.art`.

---@type LazySpec
return {
  {
    "folke/snacks.nvim",
    config = function(_, opts)
      if type(opts.dashboard) == "table" and type(opts.dashboard.sections) == "table" then
        opts.dashboard.sections[1] = function(self)
          return require("astrograph.dashboard").scene(self)
        end
      end
      require("snacks").setup(opts)
    end,
  },
}
