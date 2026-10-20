---
title: Motor nativo
nav_order: 9
---

# Motor nativo: o Astrograph sem Neovim

**Objetivo.** Um app unificado em Rust, com a praticidade do Vim (todos os atalhos),
mas sem o Neovim como motor: mais rápido, sem travas e sem exigir que a pessoa
saiba configurar nada. O editor no terminal (`nvim` + esta config) continua
existindo como espelho para quem não quiser a UI, mas o app em Rust é o produto.

Isso substitui a decisão inicial do épico #1 ("UI em Rust + Neovim embutido").
O Neovim segue sendo a **referência de comportamento**: cada parte nova do motor é
conferida contra ele.

## Como garantimos "todos os atalhos"

Teste diferencial (`prototype/astrograph-core/examples/diferencial.rs`): sequências
de teclas aleatórias rodam no `nvim --headless --clean` e no motor Rust; texto,
cursor, modo e registrador têm de ser idênticos. Cada divergência é reduzida ao
menor trecho de teclas. Os casos que concordam viram regressão
(`tests/ouro.tsv`) e rodam no CI sem o nvim.

## Estado

| Parte | Estado |
| --- | --- |
| Buffer (rope), colunas visuais (tab, CJK) | pronto |
| Movimentos `h j k l w W b B e E ge gE 0 ^ $ _ gg G f F t T ; , % { } + -` com contagem | **100%** idêntico ao nvim (3000 casos) |
| Operadores `d c y < >` + movimentos, `dd cc yy`, `x X D C s S Y`, regras do Vim (`cw`=`ce`, exclusivo→linewise, `d` multilinha→linewise) | **100%** (2500 casos) |
| Registradores (`"`, `0`, `1-9`, `-`, nomeados), `p P` com contagem | idêntico |
| Inserção (`i a I A o O`, contagem, `<CR>`, `<BS>`, autoindent, smarttab), `J r ~` | idêntico |
| Desfazer/refazer (`u`, `<C-r>`), repetir (`.` com contagem) | 99,8% (sobra o cursor após `u` em casos raros) |
| Objetos de texto `iw aw iW aW ip ap i( a( ib i[ i{ iB i< i\" i' i\``, com contagem | **99,9%** (2500 casos) |
| Visual `v` e `V` (movimentos, objetos, `d y c x X D Y C s S < > ~ u U J r o`), com as regras do Vim (cursor sobre a quebra de linha, `ip` que estende) | 99,2% (2500 casos) |
| Busca (`/ ? n N * #`, realce animado, `:noh`) | pronto |
| Marcas (`m a-z`, `'x`, `` `x ``, `''`, `'<` `'>`), com reajuste por linhas e desfazer | **99%** (195/200) |
| Macros (`q`, `@`, `@@`) | **100%** (400 casos) |
| `p`/`P`/`gp`/`gP` no visual (substitui a seleção; linha x caractere, registrador, um passo de desfazer) | **97%** (145/150) |
| Comandos `:` do núcleo: faixas (`%`, `1,3`, `'<,'>`, `.+1`, `'a`), `:s` (grupos, `&`, `\\u \\U`, flags `g i I e`), `:d`, `:y`, `:j`, `:>` `:<`, `:g`/`:v`, `:normal` e `:N` | **98%** (196/200) |
| Visual em bloco (`<C-v>`) | próximo |
| Realce, indent, textobjects (tree-sitter) | prontos em `astrograph-syntax` (reaproveitados) |
| Buscador nativo (crate `ignore` + casamento difuso próprio, sem `fd`/`ripgrep`): `<Espaço>ff` arquivos, `<Espaço>fw` texto, `<Espaço>fo` recentes, e `f o w` na tela inicial | **funcionando** |
| Setas, Home/End, PageUp/PageDown (nunca viram texto; teclas sem função são ignoradas) | pronto |
| Tecla líder configurável (`[keys] leader` no `config.toml` da UI; padrão `<Espaço>`); as menções a ela mostram o nome da tecla escolhida | pronto |
| Explorador de arquivos nativo (`<Espaço>e`: árvore com pastas que abrem/fecham, `j k h l`, `<CR>`, revela o arquivo aberto, painel que desliza) | **funcionando** |
| Terminal nativo (pty + emulação VT: `<Espaço>t`; `<C-\\><C-n>` sai, `<Espaço>T` fecha) e agentes de IA (`<Espaço>Al` roda claude/opencode/codex/gemini nele) | **funcionando** |
| Git: branch na pílula e sinais de margem (nova/alterada/removida) comparando o buffer ao `HEAD`, ao vivo (`similar`; só lê o `HEAD` com o `git` instalado) | **funcionando** |
| LSP nativo (cliente JSON-RPC próprio; servidores do PATH ou do Mason): diagnósticos, hover `K`, definição `gd`, completar (popup com filtro local; `Tab`/`<C-y>` aceita, `<C-n>`/`<C-p>` navegam); renomear ainda não | **funcionando** (verificado com o pyright real) |
| Janelas divididas e abas | roadmap (S3, S4...) |
| Camada de tela nativa no app (`ASTROGRAPH_ENGINE=native [arquivo]`): numeração, realce tree-sitter, seleção visual, statusline, `:w :q :wq :e :N`, rolagem e clique | **funcionando** (verificado no app real sob Xvfb, sem Neovim) |

## Experimentar

```
ASTROGRAPH_ENGINE=native astrograph arquivo.rs
```

Abre o arquivo no motor nativo (sem Neovim): todos os movimentos, operadores,
objetos de texto, visual, desfazer e `.` descritos acima, mais `:w`, `:q`, `:q!`,
`:wq`, `:e arquivo` e `:<número>`. Sem arquivo, abre a tela inicial do Astrograph
(`n` novo, `f` buscar arquivo, `o` recentes, `w` buscar palavra). O padrão do app continua sendo o Neovim até o
motor nativo cobrir janelas, explorador, git, LSP e as funções dos plugins.

## Divergências conhecidas

- Cursor depois de `u` quando o Vim guarda várias entradas de undo para um comando
  (`cj` + texto, por exemplo): o texto fica certo, a coluna/linha do cursor às vezes
  difere.
- `d%`/`y%` em linha sem colchete (o Vim inclui a quebra de linha).
- Comandos que o Vim executa como bloco de undo único junto de outro (`>>dGu`).

## Ordem do trabalho

1. **Núcleo** (este PR): movimentos, operadores, registradores, inserção, undo, `.`.
2. Visual, objetos de texto (`iw aw i( a" ip...`) usando as consultas do tree-sitter,
   busca, marcas, macros, `:s` `:g` `:w` e afins.
3. Camada de tela: o app passa a desenhar o motor nativo (linhas, numeração, status,
   cursor, realce) no mesmo renderer, e o Neovim sai do caminho do app.
4. Funcionalidades hoje vindas de plugins, reescritas nativas: busca de arquivos e
   texto, explorador, git, diagnósticos/formatação, LSP, completar, terminal, agentes.
5. Empacotar tudo (sem baixar nada na instalação ou na primeira abertura).
