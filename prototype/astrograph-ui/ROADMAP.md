# Roteiro: o Astrograph em Rust

Decisao (2026-10-15): o Astrograph passa a ser a **UI em Rust**. O Neovim continua
como motor (edicao, LSP, Treesitter: codigo nativo em C, ja rapido); tudo o que
hoje e interface ou plugin em Lua por cima dele vai, aos poucos, para o Rust.

Medicao de partida (Windows, mediana de 9 aberturas):

| | Tempo |
|---|---|
| Neovim puro (sem config) | 69 ms |
| Config atual, sem arquivo | 207 ms |
| Config atual, abrindo um `.lua` | 287 ms |
| Janela da UI com conteudo | ~150 ms |
| Plugins: instalados / na abertura / depois de 4 s | 46 / 14 / 28 |
| Mais caros na abertura | core do AstroNvim (~70 ms) e lazy.nvim (~48 ms) |

Cada fase substitui Lua por Rust e tira algo do caminho da abertura ou da
digitacao. A ordem e por ganho, com uma excecao: os agentes vieram para a fase 2
porque trabalhar com o Claude dentro do editor e o objetivo principal.

## v1.0: os servicos do editor tambem em Rust (decisao de 2026-10-16)

Alem da interface, vai para o Rust o que o **proprio Neovim** faz em Lua ou
Vimscript no runtime. O nucleo C (buffers, motions, undo, registros) fica: e o
vim de verdade, e reescreve-lo foi o que custou caro ao Helix e ao Zed. Runtime
do Neovim 0.12.6 medido: `lua/vim` 86 mil linhas (cliente LSP 22 mil, treesitter
6 mil, diagnostic 3 mil, filetype 5,7 mil); Vimscript `syntax/` 156 mil,
`autoload/` 27 mil, `indent/` 23 mil, `ftplugin/` 18 mil.

O servico roda no processo do Astrograph e fala com o nvim por RPC. Medido com
`cargo run --release --example rpc_bench` e `--example ts_bench` (Windows,
arquivo Lua de 2000 linhas):

| | mediana | p99 |
|---|---|---|
| ida e volta de uma chamada RPC | 72 us | 121 us |
| 200 extmarks numa chamada atomica | 0,49 ms | 0,79 ms |
| 200 extmarks em chamadas avulsas | 12,9 ms | 17,9 ms |
| tecla -> aviso no Rust -> 20 highlights aplicados | 0,63 ms | 1,16 ms |
| tree-sitter: parse incremental de 1 caractere | 0,89 ms | 1,50 ms |
| tree-sitter: consulta de realce da linha | 9 us | 38 us |
| abertura headless: `nvim --clean` / com a config | 35,8 ms | 115,6 ms (mediana de 9) |

Conclusao: uma tecla com realce feito fora do nvim custa ~1,5 ms (p99 ~2,7 ms),
menos de 1/6 de um quadro a 60 Hz, e o parse sai do loop do nvim (hoje roda no
redraw). Regras que saem da medicao: **sempre em lote** (`nvim_call_atomic`; 26x
mais rapido que avulso) e **sincronia pelo `nvim_buf_attach`** (os eventos de
linha chegam por RPC, sem Lua no meio).

Conselho (6 conselheiros + revisao cruzada): manter o nucleo C; medir antes de
cada servico; teto de horas por fase (estourou 2x, para e reavalia); cada
servico migrado troca tambem os plugins que dependiam dele (`vim.lsp`,
`vim.treesitter`, `vim.diagnostic`), senao lspconfig/blink/aerial ficam sem dado.
Indent ao digitar, autopairs e textobjects de operador so saem do nvim se a
medicao do servico couber no orcamento da tecla.

Ordem (por retorno e risco), cada uma com criterio de pronto e comparacao lado a
lado com o nvim nativo:

- [ ] **S1 tipo de arquivo + realce por tree-sitter em Rust** (substitui
  `filetype.lua`, `syntax/` e o nvim-treesitter): gramaticas compiladas no
  executavel, realce em lote por extmarks, injecoes de linguagem.
  - [x] Crate `prototype/astrograph-syntax`: 7 gramaticas (bash, javascript,
    json, lua, python, rust, toml) com as consultas do nvim-treesitter
    (`queries/`, Apache-2.0; `#lua-match?` traduzido para regex, `; inherits:`),
    deteccao por nome/extensao/shebang, parse incremental pelos eventos de linha.
    Teste: o realce incremental e igual ao do parse do zero.
  - [x] No app, opcional: `ASTROGRAPH_REALCE=rust` (`src/realce.rs`). Assina o
    buffer com `nvim_buf_attach`, desliga o nativo (`b:astrograph_realce`, que o
    astrocore respeita) e pinta primeiro a parte na tela. Arquivo grande (como o
    `large_buf` do astrocore) fica de fora.
  - [x] Medido com `cargo run --release --example realce_e2e -- <arquivo.lua>`
    (299 linhas, 5 rodadas com a config instalada; `E2E_NATIVO=1` = referencia):
    cores iguais ao nativo em 99,4% dos bytes (99,9% sem a config); linha
    digitada -> realce: Rust 9,5-10,7 ms de mediana / 18,8-22,8 ms p90, nativo
    8,9-10,6 / 18,4-26,6 (empate; o custo e o nvim ocupado com os plugins da
    config, igual com ou sem Rust). Sem config: 0,9-1,1 ms por lote no Rust.
    Executavel: 2,65 -> 7,63 MB.
  - [x] Injecoes: `comment` (TODO/FIXME), `luap` (padroes Lua), `luadoc`,
    `printf`, `regex` e `vim`, recalculadas so nas linhas pintadas (ate 2
    niveis). `luap`/`luadoc` compilados de `vendor/` (os crates puxariam o
    tree-sitter 0.20 e uma 2a copia da biblioteca C). `vim`: o crate e mais
    antigo que a consulta do Neovim, fica com a consulta da gramatica. Com a
    config: 100% dos bytes coloridos; o que difere do nativo instalado e so o
    que o Rust pinta a mais (luadoc/printf, cujos parsers a config nao instala).
    Executavel: 8,86 MB.
  - [x] App de verdade (`SPIKE_REALCE=<arquivo>`): 1938 extmarks, nativo
    desligado; print lado a lado identico ao nativo. **Ligado por padrao no
    app** (`ASTROGRAPH_REALCE=nvim` volta ao nativo); o nvim do terminal segue
    com o nativo.
  - [ ] Trocar o `filetype.lua`: medido 24 us por arquivo e ~3,3 ms na abertura
    (`vim.filetype` 2,3 ms + `filetype.lua` 0,5 + `syntax` 0,5) de 115,6 ms.
    Igualar ~1000 tipos para ganhar 3 ms: adiado (regra do conselho). O Rust ja
    detecta por conta propria para escolher a gramatica.
  - [ ] Tirar o nvim-treesitter: depende da S2 (o indent ainda e dele).
- [ ] **S2 indent e textobjects por tree-sitter** (substitui `indent/` e
  nvim-treesitter-textobjects), se couber no orcamento medido em S1.
  - [x] Indent: porte do `indent.lua` do nvim-treesitter (`astrograph-syntax`,
    `src/indent.rs`) com as mesmas consultas. Medido antes: a `indentexpr`
    nativa custa 49,6 us de mediana / 102 us p90 / 3,7 ms max por linha.
  - [x] Paridade (`cargo run --release --example indent_e2e -- <arquivos>`):
    **4168/4168 linhas iguais** em lua, python, rust, bash e json, contando o
    caso de digitar (arquivo cortado na linha k + linha nova). Dois achados para
    a paridade: (1) o nvim-treesitter monta o mapa por `iter_captures`, que em no
    de erro devolve capturas que o cursor de casamentos nao devolve
    (`(ERROR "try" . ":") @indent.begin` numa docstring aberta); o Rust faz igual;
    (2) a varredura e da arvore inteira, uma vez por versao (o `memoize` dele).
  - [x] No app: a `indentexpr` vira `rpcrequest` ao app, que espera a copia
    alcancar o `b:changedtick` (ate 50 ms; senao -1). Aplicar as edicoes e
    pintar sao tasks separadas (o indent nao espera a pintura; as faixas
    pendentes andam com cada edicao). Texto digitado identico ao nativo.
  - [x] **Ligado por padrao no app** (`ASTROGRAPH_INDENT=nvim` volta ao
    nativo). A varredura de capturas vai so nos blocos de primeiro nivel que
    contem a linha anterior e a pedida (nenhum no do caminho e cortado; a
    paridade continua 4168/4168), guardada por bloco ate a proxima edicao.
    Indent logo depois de uma edicao no meio do arquivo, com o cursor la:
    grid.rs 1,1 ms x 4,5 ms do nativo; render.rs 3,6 x 9,4; agent.lua 0,2 x 8,0
    (o nativo varre a arvore inteira a cada versao). Sequencia digitada no fim
    do arquivo: Rust 95,8-107,8 ms x nativo 104-114 (6 rodadas).
    Armadilha de medicao: headless, o nativo so reparseia a parte visivel
    (`parser:parse({w0, w$})`); com o cursor longe da edicao ele indenta com a
    arvore velha e parece rapido (~40 us).
  - [ ] Textobjects.
- [ ] **S3 diagnosticos e formatacao** (substitui none-ls): resultado entregue
  pela API `vim.diagnostic`, para os plugins que leem continuarem vendo.
- [ ] **S4 cliente LSP em Rust** (substitui nvim-lspconfig, `vim.lsp` e Mason):
  piloto com um servidor so (rust-analyzer) antes de se comprometer.
- [ ] Depois: carregador proprio (fase 7) e as fases de interface abaixo.

## Fase 1: a UI e o Astrograph

- [x] Executavel `astrograph.exe` instalado em `%LOCALAPPDATA%\Astrograph\bin`, com
  icone e atalho no menu Iniciar; sem janela de console (nem a do `nvim`).
- [x] Instalador baixa o executavel do GitHub Releases (workflow `release-app.yml` compila no Windows) ou compila com cargo; so baixa quando muda a versao.
- [x] Instalador nao atualiza com o app aberto (a config ficava misturada) e acha o cargo em `~\.cargo\bin` fora do PATH.
- [ ] Publicar o primeiro release (`v0.1.0`).

## Fase 2: agente nativo (ver o agente trabalhar no codigo)

`astrograph-agent` (Rust, `prototype/astrograph-agent`): cliente ACP que o
Neovim inicia com `jobstart`; fala JSON por linhas com o editor e ACP com o
agente (o mesmo de `<Leader>Ag`). Funciona no app e no `nvim` do terminal.

Medido com o Claude Code (`claude-agent-acp`): ele grava direto no disco (nao
usa o `fs` do cliente), mas pede permissao com o diff exato antes e informa
arquivo/linha de cada ferramenta. Por isso o editor mostra a edicao antes
(previa no proprio arquivo) e depois (recarrega e destaca o que mudou).

- [x] Ponte ACP em Rust: sessao, pedidos, permissoes, cancelar (responde as
  permissoes pendentes), modos, `fs` do cliente; testes com agente falso que
  repete a sequencia real do Claude Code.
- [x] `<Leader>Ae` pedir (com a selecao ou o arquivo atual), `<Leader>Ao`
  painel, `<Leader>Ak` interromper, `<Leader>Aw` modo, `<Leader>Av` seguir.
- [x] Segue o agente: o arquivo que ele le/edita abre na janela de codigo, na
  linha, sem tirar o foco. Previa do diff (vermelho/verde) antes de aprovar;
  linhas alteradas destacadas ate o proximo pedido.
- [x] Instalador e release levam o `astrograph-agent.exe`.
- [ ] Aprovar/recusar com teclas no proprio arquivo (hoje: menu `vim.ui.select`).
- [ ] Varias edicoes no mesmo arquivo: previa de todas de uma vez.
- [ ] Sessoes: retomar a ultima conversa (`session/load`), historico.
- [ ] Painel desenhado pela UI em Rust (hoje e um buffer markdown).
- [ ] Substituir CodeCompanion e avante quando o nativo cobrir chat e inline.
## Fase 3: busca nativa (substitui snacks.picker, fd e rg)

Arquivos, texto, buffers e recentes num painel no estilo Spotlight, com as
bibliotecas do ripgrep (`ignore`, `grep-searcher`) e do Helix (`nucleo`). Sem
processos externos: no Windows, cada `fd`/`rg` custa dezenas de ms so para
nascer. Busca em threads, resultados enquanto digita, previa com destaque.

## Fase 4: linha de comando e mensagens nativas

`ext_cmdline`, `ext_messages` e `ext_popupmenu`: a linha de comando vira uma
barra flutuante, as mensagens viram avisos discretos (sem "Press ENTER") e o
menu de completacao e desenhado pela UI. Substitui a ui2 (experimental).

## Fase 5: barra de status, abas e tela inicial nativas

Substitui heirline e o dashboard do snacks. A UI le o estado (modo, arquivo,
git, diagnosticos, agente) por eventos e desenha com a tipografia e as
animacoes dela.

## Fase 6: janelas como camadas (`ext_multigrid`)

Cada janela e flutuante vira uma camada da UI: rolagem por janela, sombras e
animacoes de abrir/fechar nativas, explorador de arquivos em Rust. Substitui
neo-tree e o `motion.lua`.

## Fase 7: carregador proprio

Substitui lazy.nvim e o core do AstroNvim (o maior custo da abertura): a UI
sabe o que carregar e quando, e o Lua que sobra (LSP, Treesitter, mapeamentos)
vira uma camada fina.

