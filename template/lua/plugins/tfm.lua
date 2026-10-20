-- Gerenciador de arquivos de terminal (yazi, lf, nnn, ranger, vifm) integrado ao
-- explorador em árvore (neo-tree) pelo tfm.nvim: a árvore segue para o panorama
-- do projeto; o gerenciador entrega buscas rápidas, preview e navegação por
-- diretórios. Sem nenhum instalado, o plugin nem carrega e nada muda.
local FMS = { "yazi", "lf", "nnn", "ranger", "vifm" }

local function installed()
  for _, fm in ipairs(FMS) do
    if vim.fn.executable(fm) == 1 then return fm end
  end
end

---Pasta do nó selecionado na árvore (o arquivo vira a pasta que o contém).
local function node_dir(state)
  local node = state.tree:get_node()
  if not node then return vim.uv.cwd() end
  local path = node:get_id()
  if node.type ~= "directory" then path = vim.fs.dirname(path) end
  return path
end

---@type LazySpec
return {
  {
    "Rolv-Apneseth/tfm.nvim",
    cond = function() return installed() ~= nil end,
    lazy = true,
    keys = {
      { "<Leader>fm", function() require("tfm").open(vim.api.nvim_buf_get_name(0)) end, desc = "Gerenciador de arquivos (TUI)" },
      { "<Leader>fM", function() require("tfm").open(vim.uv.cwd()) end, desc = "Gerenciador de arquivos na raiz do projeto" },
      { "<Leader>fv", function()
        local tfm = require "tfm"
        tfm.open(nil, { open_mode = tfm.OPEN_MODE.vsplit })
      end, desc = "Gerenciador: abrir em divisão vertical" },
    },
    opts = function()
      return {
        file_manager = installed(),
        -- árvore e gerenciador não brigam pelo netrw: quem abre pastas é o neo-tree
        replace_netrw = false,
        enable_cmds = true,
        ui = { border = "rounded", height = 0.85, width = 0.85, x = 0.5, y = 0.5 },
      }
    end,
  },
  {
    "nvim-neo-tree/neo-tree.nvim",
    optional = true,
    opts = {
      filesystem = {
        window = {
          mappings = {
            -- `T` na árvore: abre o gerenciador já dentro da pasta sob o cursor
            T = {
              function(state)
                local ok, tfm = pcall(require, "tfm")
                if not ok then
                  vim.notify("Instale yazi, lf, nnn, ranger ou vifm para usar o gerenciador (TUI).", vim.log.levels.INFO)
                  return
                end
                tfm.open(node_dir(state))
              end,
              desc = "Abrir pasta no gerenciador (TUI)",
            },
          },
        },
      },
    },
  },
}
