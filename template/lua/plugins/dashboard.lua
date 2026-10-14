-- Dashboard de abertura do Astrograph: troca o cabeçalho (sections[1]) pela cena
-- lua + estrelas + banner, adiciona o botão de Agentes e põe o rodapé de
-- startup em português. O resto do dashboard do AstroNvim permanece.
--
-- O core define `opts.dashboard` depois dos specs do usuário, então o ajuste é
-- feito no `config` (já com os opts finais), não no `opts`.
--
-- Implementação em `astrograph.dashboard`; arte em `astrograph.art`.

---@type LazySpec
return {
  {
    "folke/snacks.nvim",
    config = function(_, opts)
      local dash = opts.dashboard
      if type(dash) == "table" and type(dash.sections) == "table" then
        local scene = require "astrograph.dashboard"
        dash.sections[1] = function(self) return scene.scene(self) end
        for i, section in ipairs(dash.sections) do
          if type(section) == "table" and section.section == "startup" then
            dash.sections[i] = function() return scene.footer() end
          end
        end
        local keys = vim.tbl_get(dash, "preset", "keys")
        if type(keys) == "table" then
          table.insert(keys, math.min(#keys + 1, 5), {
            key = "a",
            action = "<Leader>Ac",
            icon = "󰚩 ",
            desc = "Agentes de IA  ",
          })
        end
      end
      require("snacks").setup(opts)
    end,
  },
}
