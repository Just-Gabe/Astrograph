# template/ — a configuração que o Neovim abre

Esta pasta é a configuração de usuário do **Astrograph**. O instalador liga
`%LOCALAPPDATA%\nvim` (Windows) ou `~/.config/nvim` (Linux/macOS) a ela.

| Arquivo | Para que serve |
| --- | --- |
| `init.lua` | Bootstrap: otimizações de startup e instalação do lazy.nvim |
| `lua/lazy_setup.lua` | Carrega o core local (`../core`) e o tema local (`../theme`) |
| `lua/polish.lua` | Ajustes finais: recarregar arquivos dos agentes, `:Astrograph` |
| `lua/plugins/*.lua` | Plugins e ajustes (agentes, animações, dashboard, Mason…) |
| `lua/astrograph/*.lua` | Módulos próprios: agentes, dashboard, health, comandos |

Os arquivos `astrocore.lua`, `astrolsp.lua`, `none-ls.lua`, `treesitter.lua` e
`user.lua` são exemplos do AstroNvim e vêm **desligados** (primeira linha
`if true then return {} end`). Apague essa linha para personalizar.

Documentação completa: <https://just-gabe.github.io/Astrograph/>
