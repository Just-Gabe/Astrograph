---
title: Performance
nav_order: 7
---

# Performance

Medido no Windows 11 com `nvim --startuptime` e o profile do lazy.nvim
(média de 5 aberturas, cache quente):

| | AstroNvim original | Astrograph |
| --- | --- | --- |
| Abrir o editor | ~392 ms | **~256 ms** (−35%) |
| Primeiro arquivo: caminho crítico | ~230 ms | **~59 ms** (−74%) |

## Resposta imediata

Tempo da tecla até a janela aparecer, com o editor aberto há alguns segundos
(UI conectada, cache quente):

| Ação | 1ª vez | 2ª vez |
| --- | --- | --- |
| Buscar arquivo (`<Leader>ff`) | ~218 ms | ~136 ms |
| Buscar texto (`<Leader>fw`) | ~178 ms | ~116 ms |
| Explorador (`<Leader>e`) | ~149 ms | ~63 ms |
| Menu de atalhos (`Espaço`) | ~159 ms | ~157 ms |

O que entrou para isso:

- **Pré-aquecimento no ocioso** (`astrograph.warmup`): ~1,2 s depois de abrir,
  um item por vez e só quando você não está digitando, carrega os módulos da
  busca, o explorador e executa `fd`/`rg`/`git` uma vez (no Windows, a primeira
  execução de cada um custa 120–210 ms porque o sistema e o antivírus os examinam).
  Desligue com `vim.g.astrograph_warmup = false`.
- **Buscas abrem na hora**: a janela aparece sem esperar o `fd`/`rg` responder,
  e os resultados entram conforme chegam.
- **Janelas flutuantes mais leves**: o core não recalcula dobras nem procura
  `.editorconfig` para janelas do picker, preview e painéis.
- **Menu de atalhos** aparece em 150 ms (antes 200 ms).

## Código próprio no lugar de plugins

Cronometrando cada autocmd em uso real, o `nvim-highlight-colors` custava
**2,6 ms a cada evento de rolagem**, mais que todo o resto junto, e o scroll
suave dispara dezenas desses eventos. O `todo-comments` fazia um trabalho
parecido. Os dois foram substituídos por `astronvim.decor`, um motor próprio
que usa o **provedor de decoração nativo do Neovim** (o mesmo mecanismo do
Treesitter): o Neovim só pede a decoração das linhas que está redesenhando, e
cada linha fica em cache pelo próprio texto.

| Cenário | Antes | Depois |
| --- | --- | --- |
| Autocmds ao mover o cursor 60× e rolar 4 páginas | 216 ms | **20 ms** |
| Plugins a baixar e carregar | 2 | 0 |

Mantém cores `#rgb`/`#rrggbb`/`#rrggbbaa`/`rgb()`/`hsl()`, TODO/FIX/HACK/WARN/
PERF/NOTE/TEST só em comentários (via Treesitter), `]T`/`[T`, `<Leader>fT`,
`<Leader>uz` (cores) e `<Leader>uT` (TODOs).

## De onde veio o ganho

O código legado do Vim (plugins de `$VIMRUNTIME`, `syntax`, `ftplugin`) custava
só ~18 ms. O que pesava era outra coisa:

**Procurar programas no PATH.** No Windows, `vim.fn.executable()` para um
programa **que não existe** varre todas as pastas do PATH × extensões
(~20 ms cada). O core checava lazygit, gdu, btm, python… no boot e repetia `git`
e `rg` em vários lugares.

- `require("astronvim").executable(bin)` guarda o resultado durante a sessão.
- Os atalhos que dependem de programas externos (`<Leader>tl`, `<Leader>tn`,
  `<Leader>tp`, `<Leader>tu`, `<Leader>tt`, `<Leader>gg`) são registrados logo
  depois que a tela aparece, um programa por vez, sem travar a abertura.
- O tema do lazygit só é configurado depois do startup, e só se ele existir.

**Arquivo em estágios.** O AstroNvim carregava tudo ao abrir um arquivo
(~120 ms com o editor travado) e ainda rodava `cmd.exe /C git rev-parse`
(~39 ms) para saber se o arquivo está num repositório. Agora:

- `AstroFile` (na hora): só LSP, completação e Treesitter;
- `AstroFileIdle` (10 ms depois, quando o arquivo já apareceu): none-ls,
  decoração (cores e TODOs), breadcrumbs, autotag e autopairs;
- git: detectado procurando a pasta `.git` em Lua (~0,2 ms), sem processo.

**Plugins carregados cedo demais.** O avante carregava em todo primeiro
arquivo aberto (~100 ms + checagem do Ollama). Agora só carrega quando você usa
`<Leader>AE/At/As/Af/Az` ou um comando `:Avante…`.

**Runtime legado.** Desligados os plugins de runtime que nenhum recurso usa
(`netrw`, `gzip`, `tar`, `zip`, `tohtml`, `rplugin`, `tutor`) e os providers
perl/ruby/python3/node. O `vim.loader` (cache de bytecode) é ligado no topo do
`init.lua`, antes até do lazy.nvim. O `matchit` continua ligado: é ele que faz
o `%` pular entre `if`/`end` e tags HTML.

**Animações.** 60 fps em vez de 120: os terminais redesenham a 60 Hz, então metade
dos quadros era trabalho perdido. As estrelas da tela inicial pausam quando ela
não está visível ou o terminal perde o foco.

## Medir na sua máquina

```bash
nvim --startuptime startup.log +q
```

O total está na linha `--- NVIM STARTED ---` no fim do arquivo. Dentro do
editor, `:Lazy profile` mostra o custo de cada plugin.
