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

## Etapas

- Passo 0: gate de toolchain (winit + softbuffer abrem janela).
- Passo 1: prova de protocolo (embed + ui_attach + parse de redraw).
- Passo 2: modelo de estado da tela (grid + highlights).
- Passo 3: render CPU com softbuffer + atlas de glifos.
- Passo 4: input de teclado + cursor.
- Passo 5: medicao + FINDINGS.md + go/no-go.
