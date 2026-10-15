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
SPIKE_BENCH=1 cargo run --release   # mede ms/frame (render por CPU)
```

## Etapas

- Passo 0: gate de toolchain (winit + softbuffer abrem janela). OK
- Passo 1: prova de protocolo (embed + ui_attach + parse de redraw). OK
- Passo 2: modelo de estado da tela (grid + highlights). OK
- Passo 3: render CPU com softbuffer + atlas de glifos. OK
- Passo 4: input de teclado + cursor. OK
- Passo 5: medicao + FINDINGS.md + go/no-go. OK (ver `FINDINGS.md`)

Resultado: **GO** para a rota R1 (UI em Rust sobre Neovim embutido).
