-- Dashboard de abertura do Astrograph: troca o cabeçalho (sections[1]) pela
-- tela de boas-vindas (estrela, nome em traço fino, linha discreta), adiciona
-- a ação de Agentes, estreita a coluna e põe o rodapé de startup em português.
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
        -- coluna estreita: o olho percorre as ações sem atravessar a tela
        dash.width = 50
        local scene = require "astrograph.dashboard"
        dash.sections[1] = function(self) return scene.scene(self) end
        for i, section in ipairs(dash.sections) do
          if type(section) == "table" and section.section == "startup" then
            dash.sections[i] = function() return scene.footer() end
          elseif type(section) == "table" and section.section == "keys" then
            -- cabe numa janela de ~27 linhas sem cortar a estrela no topo
            section.padding = 2
          end
        end
        local keys = vim.tbl_get(dash, "preset", "keys")
        if type(keys) == "table" then
          -- menos opcoes, mais foco: marcadores continuam em <Leader>f'
          for i = #keys, 1, -1 do
            if keys[i].key == "'" then table.remove(keys, i) end
          end
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
