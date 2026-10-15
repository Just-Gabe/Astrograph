-- Astrograph: tema do lazygit gerado a partir do colorscheme (astroui).
--
-- Ligado só depois do startup e só com o lazygit instalado: `enable()` é
-- chamado pelas checagens adiadas de ferramentas externas (toggleterm).

local M = {}

M.defaults = {
  theme_path = vim.fs.normalize(vim.fn.stdpath "cache" .. "/astroui-lazygit-config.yml"),
  theme = {
    [241] = { fg = "Special" },
    activeBorderColor = { fg = "MatchParen", bold = true },
    cherryPickedCommitBgColor = { bg = "Substitute" },
    cherryPickedCommitFgColor = { fg = "Substitute" },
    defaultFgColor = { fg = "Normal" },
    inactiveBorderColor = { fg = "FloatBorder" },
    markedBaseCommitBgColor = { bg = "CurSearch" },
    markedBaseCommitFgColor = { fg = "CurSearch" },
    optionsTextColor = { fg = "Function" },
    searchingActiveBorderColor = { fg = "MatchParen", bold = true },
    selectedLineBgColor = { bg = "Visual" }, -- set to `default` to have no background colour
    unstagedChangesColor = { fg = "DiagnosticError" },
  },
}

---Liga a sincronização do tema do lazygit (idempotente).
function M.enable()
  local ok, astroui = pcall(require, "astroui")
  if not ok or type(astroui.config.lazygit) == "table" then return end
  astroui.config.lazygit = vim.deepcopy(M.defaults)
  -- o módulo guarda a config ao ser carregado: recarrega com a config nova
  package.loaded["astroui.lazygit"] = nil
  require("astroui.lazygit").setup()
end

return M
