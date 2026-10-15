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

