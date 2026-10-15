# FINDINGS - spike da UI em Rust sobre Neovim

Resultado do spike (epico #1, issues #3..#8). Rota R1: manter o **Neovim** como
motor e reescrever a **casca** em Rust; o objetivo de medio prazo e desacoplar do
**AstroNvim**, nao do Neovim.

Data: 2026-10-15. Ambiente: Windows 11, host Rust `x86_64-pc-windows-gnu` (MinGW,
sem MSVC), Neovim 0.12.6, GPU AMD RX 7600 (irrelevante para o render por CPU).

## Metodo

Crate isolado `prototype/astrograph-ui` (winit + softbuffer + nvim-rs + tokio +
ab_glyph). O Neovim roda com `nvim --embed --headless` carregando a config e o
tema do Astrograph; a UI faz `ui_attach` (`ext_linegrid` + `rgb`), reconstroi a
grade a partir dos eventos `redraw` e desenha por CPU com um atlas de glifos
(JetBrainsMono Nerd Font). Input de teclado vai para o Neovim via `nvim_input`.

## Resultados

Todos os passos passaram:

| Passo | Prova | Resultado |
|-------|-------|-----------|
| 0 - toolchain | winit + softbuffer | `GATE_OK`, janela abre no `windows-gnu` |
| 1 - protocolo | embed + ui_attach + parse de redraw | dashboard do Astrograph reconstruido em texto; `colors_name = astrograph` |
| 2 - estado | grid + highlights + default colors | 160x44 celulas, cores por `hl_attr_define` |
| 3 - render | softbuffer + atlas ab_glyph | dashboard pintado (icons Nerd Font visiveis) |
| 4 - input | `nvim_input` | digitar `:` entra em `cmdline_normal`; `INPUT_OK: sim` |
| 5 - medicao | render por CPU | ver abaixo |

### Numeros (1280x800)

| Metrica | Debug | Release |
|---------|-------|---------|
| Primeira pintura (desde o inicio) | 40 ms | 34 ms |
| Render medio por frame (CPU, sem vsync) | 13,26 ms (~75 fps) | **1,13 ms (~880 fps)** |

- 1,13 ms/frame em release para repintar 1280x800 com blit de glifos. Folga
  enorme sobre 60 fps (16,7 ms).
- A config/tema do Astrograph carregam intactos pelo motor embutido
  (`colors_name = astrograph`, "pronto em ~130 ms" na statusline).

### Comportamentos observados

- O parametro do `redraw` e um array de entradas no formato
  `[nome, payload1, payload2, ...]`; eventos consecutivos de mesmo nome vem
  agrupados (um unico item `grid_line` carrega varias linhas; um
  `hl_attr_define` carrega dezenas de atributos).
- Os redraws nao cessam (dashboard/statusline atualizam periodicamente), entao
  a UI foi dirigida por eventos (`EventLoopProxy`), nao por "estabilidade".

## O que NAO foi feito (fora do escopo do spike)

- Cursor de forma variavel (so o bloco), selecao de mouse, scroll do mouse,
  copiar/colar, `CSI`/clipboard, ligaduras, blend, multi-grid (winbar/flutuantes
  como floats usam `grid_line` normal, mas `ext_multigrid` nao foi testado).
- Fonte/medida de celula nao ligadas ao `guifont`; tamanho fixo (18px).
- Repintura total por frame (nao ha dirty-tracking de celulas; irrelevante dado
  o custo medido).

## Recomendacao (go/no-go)

**GO.** A rota R1 e viavel: toolchain fecha no `windows-gnu`, o tema atual
funciona sem reescrita, o render por CPU tem folga de ~14x sobre 60 fps, e o
input flui normalmente. Nao e necessario `wgpu` neste estagio.

Proximo passo: Fase 1 (issue #9) - promover a crate a UI de produto (dirty
tracking, cursor completo, mouse/clipboard, medicao de fonte por `guifont`),
antes de atacar as fases 2 (distribuicao) e 3 (absorver bootstrap/plugins de
`core/`, aposentando o AstroNvim).
