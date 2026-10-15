# astrograph-ui (spike)

Spike de viabilidade da **UI em Rust** do Astrograph sobre um **Neovim embutido**
(`nvim --embed`, protocolo UI via msgpack-rpc). Rota R1: o Neovim continua sendo
o motor; o Rust assume a casca. O objetivo de medio prazo e **desacoplar do
AstroNvim**, nao do Neovim.

Ver o epico #1 e as issues do spike (#3..#8).

## Rodar

```
cargo run
```

Variaveis uteis para verificacao automatizada:

```
SPIKE_SMOKE=1 cargo run   # dump da grade em texto e sai
SPIKE_INPUT=1 cargo run   # testa input (digita ":" e checa cmdline)
SPIKE_WIDE=1 cargo run    # testa caracteres largos (CJK)
SPIKE_BENCH=1 cargo run --release   # mede ms/frame (render por CPU)
DEBUG_FONT=1 cargo run    # imprime metricas da fonte
```

## Configuracao

Config TOML opcional (ver `astrograph-ui.example.toml`). Ordem de busca: flag
`--config`, `ASTROGRAPH_UI_CONFIG`, `./astrograph-ui.toml`, ao lado do exe,
`%APPDATA%\astrograph\ui.toml`. Ajusta janela, fonte (tamanho em px de EM,
altura de linha, arquivos por estilo) e cursor (blink).

## Atalhos (UI)

- `Ctrl +` / `Ctrl -`: aumenta/diminui a fonte.
- `Ctrl 0`: volta ao tamanho da config.
- `F11`: tela cheia.

O restante do teclado e do mouse vai para o Neovim (mouse via `nvim_input_mouse`).

## Etapas

- Passo 0: gate de toolchain (winit + softbuffer abrem janela). OK
- Passo 1: prova de protocolo (embed + ui_attach + parse de redraw). OK
- Passo 2: modelo de estado da tela (grid + highlights). OK
- Passo 3: render CPU com softbuffer + atlas de glifos. OK
- Passo 4: input de teclado + cursor. OK
- Passo 5: medicao + FINDINGS.md + go/no-go. OK (ver `FINDINGS.md`)

Resultado: **GO** para a rota R1 (UI em Rust sobre Neovim embutido).

## Fase 1 (produto) - em andamento

- Config TOML (janela, fonte, cursor).
- Fonte configuravel (tamanho em EM, altura de linha, arquivos por estilo).
- Atalhos de UI: zoom (`Ctrl +/-/0`), tela cheia (`F11`).
- Identidade: titulo e icone proprio (estrela do Astrograph).
- Cursor por modo (bloco/barra/sublinhado) com blink.
- Caracteres largos (CJK) e mouse (clique + scroll via `nvim_input_mouse`).
- Fallback de fontes por glifo (emoji, simbolos, CJK), configuravel.
- Decisao: `wgpu` **nao** e necessario (render por CPU a ~1 ms/frame).
- Pendente: shaping/ligaduras via `cosmic-text`, clipboard, cursor de mouse,
  selecao visual com o mouse. Fallback CJK completo depende de fonte TTF (os
  fontes CJK do Windows sao TTC, que o ab_glyph nao carrega).
