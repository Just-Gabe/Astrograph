-- Customize Mason

---@type LazySpec
return {
  -- use mason-tool-installer for automatically installing Mason packages
  {
    "WhoIsSethDaniel/mason-tool-installer.nvim",
    -- overrides `require("mason-tool-installer").setup(...)`
    opts = function(_, opts)
      -- Make sure to use the names found in `:Mason`
      local ensure_installed = {
        -- install language servers
        "lua-language-server",

        -- install formatters
        "stylua",

        -- install debuggers
        "debugpy",

        -- install any other package
        "tree-sitter-cli",

        -- `fd` e `ripgrep` (pickers <Leader>ff/<Leader>fw) NAO ficam aqui: nao existem
        -- no registro do Mason ("Cannot find package"). Quem instala sao o install.ps1 /
        -- install.sh (winget/brew/apt...); o :checkhealth avisa se faltarem.
      }

      -- Only keep entries that actually exist in the active Mason registry.
      -- This avoids a hard error (which aborts installing the other tools) when
      -- a registry is incomplete/offline, and stays cross-platform.
      local ok, registry = pcall(require, "mason-registry")
      if ok then
        ensure_installed = vim.tbl_filter(function(pkg) return registry.has_package(pkg) end, ensure_installed)
      end

      opts.ensure_installed = ensure_installed
    end,
  },
}
