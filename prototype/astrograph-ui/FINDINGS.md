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

## Revisao da fase 1: desempenho e design (2026-10-15)

Principio: o que funciona bem na primeira vez tem que funcionar bem n vezes.

### Correcoes

| Problema | Causa | Correcao |
|----------|-------|----------|
| `AB你 好 CD` | a grade avancava 2 colunas no caractere largo **e** gravava a celula `""` que o Neovim manda | cada celula do protocolo ocupa uma coluna; `""` vira metade direita |
| Quadros pela metade | redesenho a cada lote `redraw`, nao no `flush` | grade de trabalho + publicacao so no `flush` (so linhas alteradas) |
| Clipboard nao copiava | valores string no `g:clipboard` sao executados como **shell** | funcoes Lua: `rpcnotify` (copiar, sem esperar) e `rpcrequest` (colar) |
| `<` sumia, `Ctrl+Seta`/F-keys nao chegavam | traducao de teclas incompleta | `<lt>`, modificadores em teclas especiais, F1–F12, AltGr |
| Fechar a janela perdia edicoes | `CloseRequested` saia na hora | pede `:confirm qa`; a janela fecha quando o Neovim sai |
| Neovim saia e a janela ficava | fim do `io_handler` ignorado | fim do Neovim fecha a UI |
| Glifos sumindo apos muito uso | atlas cheio cacheava "sem glifo" para sempre | atlas cheio e limpo e o glifo e refeito |
| Caractere largo cortado ao meio | fundo da celula seguinte pintado depois do glifo | cada linha em 3 passadas: fundos, glifos, decoracoes |
| Janela maior que a tela | tamanho fixo da config | limitada a area util do monitor, centralizada |

### Numeros (release, 1280x750)

| Metrica | Antes | Depois |
|---------|-------|--------|
| Quadro com a tela inteira | 1,03 ms | 0,94 ms |
| Quadro tipico (digitar, piscar, mover o cursor) | 1,03 ms (tela inteira) | **0,027 ms** (~38x menos) |
| Quadros por lote `redraw` | 1 por lote | 1 por `flush` |
| Copiar `"+y` | (nao funcionava) | notificacao, sem processo externo |

- Repintura incremental: `buffer.age()` diz se o quadro anterior foi
  preservado; se sim, so as linhas sujas (e as do cursor) sao pintadas e
  enviadas com `present_with_damage`.
- Mistura de cor em inteiros (vermelho e azul numa multiplicacao), cache de
  glifos ASCII em vetor (sem hash), atributos resolvidos uma vez por trecho.
- A superficie so e redimensionada quando o tamanho muda.
- Com o cursor deslizando, quadros a ~144 Hz so durante os 80 ms do
  movimento; parado, o loop dorme ate o proximo piscar.
- "Primeira pintura" agora mede a janela **aparecendo ja com o conteudo do
  Neovim** (~300 ms, o tempo de inicio da config); antes media uma janela vazia
  aos ~50 ms seguida do conteudo. Sem flash: a janela nasce invisivel e com a
  barra de titulo na cor do tema; se o Neovim demorar mais de 600 ms, ela
  aparece mesmo assim.

### Design

- Barra de titulo, borda e fundo numa cor so (a do tema), cantos arredondados:
  a janela parece uma peca unica, como no macOS.
- Margem de 10 px ao redor do texto, com a sobra (menor que uma celula)
  dividida entre os lados.
- Texto com peso "cheio" (gama 1/1.15 na cobertura), mais proximo do macOS.
- Cursor deslizando com ease-out; aceso enquanto voce digita ou move.
- Blocos e molduras desenhados como geometria; cantos `╭╮╯╰` com arco
  antisserrilhado.

## Ligaduras (2026-10-15)

Ultimo item da issue #9: `->`, `=>`, `!=`, `===` como ligaduras da JetBrains
Mono.

### Escolha: rustybuzz, nao cosmic-text

- `cosmic-text` traz layout de paragrafo, sistema de fontes e rasterizador
  proprios (swash): duplicaria o atlas e o fallback que ja temos e nao conhece
  a grade do Neovim.
- `rustybuzz` so faz o shaping (porta do HarfBuzz em Rust puro) e usa o mesmo
  `ttf-parser` 0.25 que ja estava no projeto. O desenho continua no atlas do
  `ab_glyph`, agora tambem por id de glifo.

### Como funciona

- As fontes de programacao fazem ligaduras com `calt`: cada caractere vira um
  glifo de **uma celula** (o `>` de `->` vira a seta inteira, invadindo a
  celula do `-`, que vira um espaco). A grade continua valendo.
- So passam pelo shaping trechos de **pontuacao ASCII** de mesma cor e estilo
  com 2+ caracteres. O resto da linha segue o caminho rapido, celula a celula.
- Resultado em cache por texto e estilo; trechos que o shaping nao muda (`();`)
  ficam marcados e voltam ao caminho rapido sem novo shaping.
- O trecho que contem o cursor e desenhado sem ligadura: o caractere da celula
  aparece (como o `disable_ligatures cursor` do kitty), parado ou digitando.
- `font.ligatures = false` desliga tudo. Fontes "NL" nao tem ligaduras e caem
  sozinhas no caminho rapido.
- Limite conhecido: ligaduras com letras ou digitos (`www`, `0xF`) nao sao
  formadas.

### Numeros (release)

| Metrica | Antes | Depois |
|---------|-------|--------|
| SPIKE_BENCH, tela inteira (1280x750) | 0,98-1,02 ms | 0,96-0,98 ms |
| SPIKE_BENCH, so o que mudou | 0,033-0,038 ms | 0,031-0,036 ms |
| Tela cheia de ligaduras, tela inteira (1320x960) | 0,77 ms (desligadas) | 0,84 ms (ligadas) |
| Tela cheia de ligaduras, uma linha | 0,018 ms (desligadas) | 0,020 ms (ligadas) |

O pior caso (codigo denso em `->`/`=>`/`!=`, teste `bench_ligaduras`) custa
~10% a mais; no dashboard a diferenca fica dentro do ruido.

## Animacoes nativas e visual (2026-10-15)

### Rolagem por pixel

- Primeira versao usava os eventos `grid_scroll`. Na config do Astrograph eles
  **nunca chegavam**: as guias de indentacao do Snacks pedem redesenho da janela
  a cada rolagem, e entao o Neovim reescreve as linhas em vez de rolar.
- Versao final: a config avisa a UI no `WinScrolled` (que roda antes do Neovim
  desenhar o quadro) quantas linhas **de tela** a janela rolou
  (`nvim_win_text_height`, contando quebras e dobras). A UI guarda as linhas que
  vao sair e, nos quadros seguintes, desenha a regiao deslocada com as linhas
  saindo pela borda, desacelerando (exponencial, ~160 ms). Funciona com
  qualquer plugin.
- Teste `SPIKE_SCROLL`: Ctrl+D/Ctrl+U de 11 linhas geram ~150 quadros
  intermediarios, deslocamento maximo de 319 px.

### Neovim esperando a UI (`--embed` sem `--headless`)

Com `--embed --headless` o Neovim **nao espera** o `ui_attach`: carrega a config
na hora. Tudo que a UI configurava "antes da config" (clipboard, aviso de UI
nativa) chegava depois dos plugins, e o scroll do Snacks seguia ligado,
animando por cima da rolagem nativa (cada Ctrl+D virava 7 rolagens). Agora:
`--embed` (espera a UI), `--cmd` com `g:astrograph_gui` e `ui_attach` ja com o
tamanho real da janela (o dashboard nascia centralizado para outra altura e
deslocado 2 linhas).

| Metrica | Antes | Depois |
|---------|-------|--------|
| Janela visivel com conteudo | ~290 ms | ~175 ms |
| Quadro tipico (so o que mudou) | 0,034 ms | 0,038 ms (com sombras e rolagem) |
| Quadro com a tela inteira | 0,92 ms | 0,96 ms |

### Outros

- Sombra das janelas flutuantes: a config manda os retangulos (com borda) so
  quando mudam (`SafeState`, `WinResized`, `OptionSet winblend`); a UI desenha
  uma sombra arredondada suave, so nas linhas repintadas no quadro, sem sombra
  de paineis dentro de outro painel (as partes da busca).
- Cursor: fade no piscar (180 ms) e troca de forma animada (constante de 28 ms).
- Molduras grossas como geometria (tabela de bracos de U+2500..257F).
- A janela nasce na cor do tema e o primeiro quadro (a tela vazia de antes da
  config) nao mostra cursor: sem "flash" na abertura.
