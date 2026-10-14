return {
  "AstroNvim/astrolsp",
  ---@param opts AstroLSPOpts
  opts = function(_, opts)
    ---@type AstroLSPMappings
    local maps = require("astrocore").empty_map_table()
    maps.n["<Leader>l"] = { desc = require("astroui").get_icon("ActiveLSP", 1, true) .. "Ferramentas de linguagem" }
    maps.v["<Leader>l"] = { desc = require("astroui").get_icon("ActiveLSP", 1, true) .. "Ferramentas de linguagem" }

    maps.n["<Leader>la"] =
      { function() vim.lsp.buf.code_action() end, desc = "LSP: ação de código", cond = "textDocument/codeAction" }
    maps.x["<Leader>la"] =
      { function() vim.lsp.buf.code_action() end, desc = "LSP: ação de código", cond = "textDocument/codeAction" }
    maps.n["<Leader>lA"] = {
      function() vim.lsp.buf.code_action { context = { only = { "source" }, diagnostics = {} } } end,
      desc = "LSP: ação de arquivo",
      cond = "textDocument/codeAction",
    }

    -- TODO: remove mapping when dropping support for Neovim v0.11
    maps.n["<Leader>ll"] = {
      function()
        if vim.lsp.codelens.enable then
          vim.lsp.codelens.enable(true)
        else
          vim.lsp.codelens.refresh()
        end
      end,
      desc = "LSP: atualizar CodeLens",
      cond = "textDocument/codeLens",
    }
    maps.n["<Leader>lL"] =
      { function() vim.lsp.codelens.run() end, desc = "LSP: executar CodeLens", cond = "textDocument/codeLens" }
    maps.n["<Leader>uL"] = {
      function() require("astrolsp.toggles").codelens() end,
      desc = "Alternar CodeLens",
      cond = "textDocument/codeLens",
    }

    maps.n["gD"] = {
      function() vim.lsp.buf.declaration() end,
      desc = "Declaração do símbolo atual",
      cond = "textDocument/declaration",
    }
    maps.n["gd"] = {
      function() vim.lsp.buf.definition() end,
      desc = "Definição do símbolo atual",
      cond = "textDocument/definition",
    }
    maps.n["gI"] = {
      function() vim.lsp.buf.implementation() end,
      desc = "Implementação do símbolo atual",
      cond = "textDocument/implementation",
    }

    local function formatting_checker(method)
      method = "textDocument/" .. (method or "formatting")
      return function(client, bufnr)
        local disabled = opts.formatting.disabled
        return client:supports_method(method, bufnr)
          and disabled ~= true
          and not vim.tbl_contains(disabled, client.name)
      end
    end
    local formatting_enabled = formatting_checker()
    maps.n["<Leader>lf"] = {
      function() vim.lsp.buf.format(require("astrolsp").format_opts) end,
      desc = "Formatar buffer",
      cond = formatting_enabled,
    }
    maps.v["<Leader>lf"] = {
      function() vim.lsp.buf.format(require("astrolsp").format_opts) end,
      desc = "Formatar buffer",
      cond = formatting_checker "rangeFormatting",
    }
    maps.n["<Leader>uf"] = {
      function() require("astrolsp.toggles").buffer_autoformat() end,
      desc = "Alternar formatação automática (buffer)",
      cond = formatting_enabled,
    }
    maps.n["<Leader>uF"] = {
      function() require("astrolsp.toggles").autoformat() end,
      desc = "Alternar formatação automática (global)",
      cond = formatting_enabled,
    }

    maps.n["<Leader>u?"] = {
      function() require("astrolsp.toggles").signature_help() end,
      desc = "Alternar ajuda de assinatura automática",
      cond = "textDocument/signatureHelp",
    }

    maps.n["<Leader>uh"] = {
      function() require("astrolsp.toggles").buffer_inlay_hints() end,
      desc = "Alternar dicas inline do LSP (buffer)",
      cond = "textDocument/inlayHint",
    }
    maps.n["<Leader>uH"] = {
      function() require("astrolsp.toggles").inlay_hints() end,
      desc = "Alternar dicas inline do LSP (global)",
      cond = "textDocument/inlayHint",
    }

    maps.n["<Leader>lR"] =
      { function() vim.lsp.buf.references() end, desc = "Buscar referências", cond = "textDocument/references" }

    maps.n["<Leader>lr"] =
      { function() vim.lsp.buf.rename() end, desc = "Renomear símbolo", cond = "textDocument/rename" }

    maps.n["<Leader>lh"] =
      { function() vim.lsp.buf.signature_help() end, desc = "Ajuda de assinatura", cond = "textDocument/signatureHelp" }
    maps.n["gK"] =
      { function() vim.lsp.buf.signature_help() end, desc = "Ajuda de assinatura", cond = "textDocument/signatureHelp" }

    maps.n["gy"] = {
      function() vim.lsp.buf.type_definition() end,
      desc = "Definição do tipo atual",
      cond = "textDocument/typeDefinition",
    }

    maps.n["<Leader>lG"] =
      { function() vim.lsp.buf.workspace_symbol() end, desc = "Buscar símbolos do projeto", cond = "workspace/symbol" }

    -- TODO: Remove check when dropping support for Neovim v0.11
    if vim.lsp.buf.workspace_diagnostics then
      maps.n["<Leader>lw"] = {
        function() vim.lsp.buf.workspace_diagnostics() end,
        desc = "Diagnósticos do projeto",
        cond = "workspace/diagnostic",
      }
    end

    maps.n["<Leader>uY"] = {
      function() require("astrolsp.toggles").buffer_semantic_tokens() end,
      desc = "Alternar destaque semântico do LSP (buffer)",
      cond = function(client, bufnr) return client:supports_method("textDocument/semanticTokens/full", bufnr) end,
    }
    opts.mappings = require("astrocore").extend_tbl(opts.mappings, maps)
  end,
}
