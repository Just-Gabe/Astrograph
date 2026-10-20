# Consultas de realce

Os arquivos `*/highlights.scm` desta pasta vêm do
[nvim-treesitter](https://github.com/nvim-treesitter/nvim-treesitter)
(`runtime/queries`, commit `61df84986b4b4ec469ee745a182e433d49f8c27e`), sob a licença Apache-2.0
(https://www.apache.org/licenses/LICENSE-2.0). São as mesmas consultas que o
Astrograph usava pelo plugin, para o realce em Rust ter as mesmas cores.

O `astrograph-syntax` traduz na carga os predicados que só o Neovim entende
(`#lua-match?` vira `#match?`; `; inherits:` junta as consultas herdadas).

Os `*/textobjects.scm` vêm do
[nvim-treesitter-textobjects](https://github.com/nvim-treesitter/nvim-treesitter-textobjects)
(`queries`, commit `898ee307df58f854d11cd7edd06472574d48014e`), também Apache-2.0.
