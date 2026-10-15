-- Astrograph: decoração própria (cores e TODOs) no lugar do nvim-highlight-colors
-- e do todo-comments. Implementação em `astronvim.decor`.
return {
  "AstroNvim/astrocore",
  opts = function(_, opts)
    local maps = opts.mappings
    local decor = function() return require "astronvim.decor" end
    maps.n["<Leader>uz"] = { function() decor().toggle "colors" end, desc = "Alternar destaque de cores" }
    maps.n["<Leader>uT"] = { function() decor().toggle "todo" end, desc = "Alternar destaque de TODOs" }
    maps.n["]T"] = { function() decor().jump(1) end, desc = "Próximo TODO" }
    maps.n["[T"] = { function() decor().jump(-1) end, desc = "TODO anterior" }
    maps.n["<Leader>fT"] = { function() decor().pick() end, desc = "Buscar TODOs" }

    opts.autocmds = opts.autocmds or {}
    opts.autocmds.astronvim_decor_setup = {
      {
        event = "User",
        pattern = "AstroFileIdle",
        once = true,
        desc = "Ligar a decoração de cores e TODOs",
        callback = function() decor().setup() end,
      },
    }
  end,
}
