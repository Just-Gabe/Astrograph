---
title: Performance
nav_order: 7
---

# Performance

Medido no Windows 11 com `nvim --startuptime` e o profile do lazy.nvim
(média de 5 aberturas, cache quente):

| | AstroNvim original | Astrograph |
| --- | --- | --- |
| Abrir o editor | ~392 ms | **~277 ms** (−29%) |
| Primeiro arquivo aberto | ~230 ms | **~119 ms** (−48%) |

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
- Sem lazygit instalado, o tema dele não é regravado em todo startup.

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
