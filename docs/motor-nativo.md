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
| **Folds** (`zf zF zo zO zc zC za zA zv zx zX zR zM zr zm zd zD zE zn zN zi zj zk`; `:set foldmethod=indent\|syntax`, `foldlevel`, `[editor] foldmethod`): a dobra fechada some do texto com um aviso `⋯ N linhas`, `j`/`k` pulam por cima, operadores por linhas levam a dobra inteira e a margem mostra os números do arquivo | **99%** (`dob`) |
| Marcas (`m a-z`, `'x`, `` `x ``, `''`, `'<` `'>`, `'.` `'^` `'[` `']`), **jumplist** (`<C-o>`/`<C-i>`), **changelist** (`g;`/`g,`), `gi`, com reajuste por linhas e desfazer | **99%** (`marc`) e 97% (`marc2`) |
| Macros (`q`, `@`, `@@`) | **100%** (400 casos) |
| `p`/`P`/`gp`/`gP` no visual (substitui a seleção; linha x caractere, registrador, um passo de desfazer) | **97%** (145/150) |
| Comandos `:` do núcleo: faixas (`%`, `1,3`, `'<,'>`, `.+1`, `'a`, `3:`), `:s` (grupos, `&`, `\\u \\U`, flags `g i I e n`), `:&` `:&&` `:~` `&`, `:d`, `:y`, `:j`, `:>` `:<`, `:t` `:m` `:sort` (`! n i u`), `:g`/`:v`, `:normal`, `@:` e `:N` | **99%** (298/300) |
| Inserção estendida: `<C-w>` `<C-u>` `<C-h>` `<C-t>` `<C-d>` `<C-o>`; formatação `gq`/`gw` (`:set textwidth`); `N%`, `d%` em linha vazia; `gJ gp gP gv gI`; `gf` (abre o arquivo sob o cursor, com `:linha:col`) e `gx` (URL no navegador); objetos de texto de **tree-sitter** `if af ic ac ia aa` com operador e no visual | 95% a 99% conforme o vocabulário (`ins`, `gq`, `pct`) |
| Visual em bloco (`<C-v>`: `d y c r ~ u U I A o O $`, `p`/`P` de registro em bloco, retângulo na tela) | **97%** (sobram `A`/`I` em bloco que corta caractere largo e `y` com a ponta num tab) |
| `<C-a>`/`<C-x>` (decimal, `0x`, `0b`, negativos, zeros à esquerda; visual e `g<C-a>`), operadores de caixa `gu gU g~ g?` (`guu`, `gUgU`, objetos, `.`), `.` depois de operador visual (mesmo tamanho a partir do cursor), registradores (`"A` anexa, `"_` buraco negro, `".`, `"/`, `"0`-`"9`, `"x` no visual, `.` com `"1p`→`"2p`), `<C-r>{reg}` na inserção, bloco que corta tab ou caractere largo (a metade vira espaço) | **99%** em `num`, `caso`, `reg`; 98% em `ponto` e `blk` (contra o nvim 0.11) |
| **Mapeamentos** `:map :nmap :imap :vmap :noremap :nnoremap :inoremap :vnoremap :unmap :mapclear` (recursivos ou não, prefixo pendente na tela, `<Leader>`, valem em todas as abas e painéis) e `[keys.normal]`/`[keys.insert]`/`[keys.visual]` no `config.toml`; **`:set`** (`ignorecase smartcase expandtab shiftwidth tabstop number relativenumber scrolloff clipboard`); **clipboard do sistema** (`"+`/`"*`, `clipboard=unnamedplus`, `[editor] clipboard`); **mouse** no editor (arrastar seleciona, duplo clique palavra, triplo linha, `Shift+clique` estende); **zoom** `Ctrl +/-/0` | **funcionando** (mapeamentos e `:set` conferidos contra o nvim: 30 casos) |
| Realce, indent, textobjects (tree-sitter) | prontos em `astrograph-syntax` e ligados ao núcleo (`if af ic ac ia aa`) |
| Buscador nativo em **segundo plano** (listar e buscar texto em threads com cancelamento e resultados parciais, 'buscando…' no cabeçalho; lista de arquivos em cache por raiz; letra a mais só procura entre os que já casaram; Enter antes da lista chegar espera e abre) (crate `ignore` + casamento difuso próprio, sem `fd`/`ripgrep`): `<Espaço>ff` arquivos, `<Espaço>fw` texto, `<Espaço>fo` recentes, e `f o w` na tela inicial | **funcionando** |
| Setas, Home/End, PageUp/PageDown (nunca viram texto; teclas sem função são ignoradas) | pronto |
| Tecla líder configurável (`[keys] leader` no `config.toml` da UI; padrão `<Espaço>`); as menções a ela mostram o nome da tecla escolhida | pronto |
| Explorador de arquivos nativo (`<Espaço>e`: árvore com pastas que abrem/fecham, `j k h l`, `<CR>`, revela o arquivo aberto, painel que desliza; **`a` cria** (termine com `/` para pasta), **`r` renomeia, `d` apaga** (confirma), **`y`/`x` copiam/recortam e `p` cola**; nomes coloridos e letra à direita com o **estado do git**) | **funcionando** |
| Buscador: **trecho do arquivo** selecionado (centrado na linha achada na busca de texto) e ranking que prefere o trecho contíguo (`main` → `main.rs`) | **funcionando** |
| Aparência: **tema claro** (`[theme] mode = "dark"\|"light"\|"auto"`, `:set background=light`), **fonte** do `[font]` (`regular`, `bold`, `italic`) com reserva do sistema, **menu de atalhos** da líder (aparece ao esperar), progresso do LSP e do agente no canto | **funcionando** |
| Projeto: **`.editorconfig`** (`indent_style`, `indent_size`, `tab_width`, `trim_trailing_whitespace`, `insert_final_newline`) e o cursor volta onde estava ao reabrir o arquivo | **funcionando** |
| Terminal nativo (pty + emulação VT: `<Espaço>t`; `<C-\\><C-n>` sai, `<Espaço>T` fecha; **seleção com o mouse** (copia ao soltar) e `Ctrl+Shift+C`; **colar** com `Ctrl+Shift+V`/`Shift+Insert` (bracketed paste); **histórico** com a roda e `Shift+PageUp/PageDown`; mouse repassado a programas TUI (vim, htop...), com `Shift` para selecionar; aviso quando o processo termina; **vários terminais**: `<Espaço>N` abre outro, `<Espaço>]`/`[` alternam, `<Espaço>T` fecha o atual e volta ao anterior) e agentes de IA (`<Espaço>Al` roda o agente escolhido nele; `<Espaço>Ag` escolhe entre os instalados (claude/opencode/codex/gemini) e `<Espaço>Aw` o modo (manual, editar, plano, auto, total), ambos salvos no `config.json` que o plugin Neovim também lê; `<Espaço>AS` retoma uma conversa do Claude Code na pasta dela (`s` na tela inicial retoma a última); `<Espaço>Ax/Ai/Ar/Am` pedem ao agente para corrigir o erro da linha, explicar a seleção, revisar as alterações e escrever o commit; **`<Espaço>AE`** edita a seleção com a IA (a proposta troca o trecho num passo só, `u` desfaz) ou, sem seleção, responde uma pergunta numa aba; `<Espaço>Ac` cita o arquivo ou as linhas (`@arquivo#L3-9`) no agente; `:AgentInstall`/`:AgentLogin` instalam e entram no agente; **Ollama** como agente local; o arquivo que o agente edita recarrega sozinho, e com alterações suas não salvas só avisa; `<Espaço>Az` abre o **modo zen**: o agente em tela cheia, coluna centrada num cartão, com fade) | **funcionando** |
| Git: branch na pílula e sinais de margem (nova/alterada/removida) comparando o buffer ao `HEAD`, ao vivo (`similar`; só lê o `HEAD` com o `git` instalado) | **funcionando** |
| LSP nativo (cliente JSON-RPC próprio; servidores do PATH ou do Mason): diagnósticos, hover `K`, definição `gd`, completar (popup com filtro local; `Tab`/`<C-y>` aceita, `<C-n>`/`<C-p>` navegam), **renomear** (`<Espaço>rn` / `:Rename`, em vários arquivos), **ações de código** (`<Espaço>ca`, lista), **formatar** (`<Espaço>lf` / `:Format`; `[lsp] format_on_save = true` formata ao `:w`), **snippets** do servidor com paradas (`Tab` pula, o valor padrão vem selecionado) e **imports automáticos** (`additionalTextEdits`), `didChange` **incremental**, um servidor por projeto, progresso (indexando…) no canto, **assinatura** ao digitar `(` e `,`, **referências** (`gr`), tipo (`gy`), implementação (`<Espaço>li`), **símbolos** do arquivo (`<Espaço>ls`) e do projeto (`<Espaço>lS` / `:WorkspaceSymbol`), **lista de diagnósticos** (`<Espaço>xx`) e `]d`/`[d` | **funcionando** (verificado com um servidor falso e o pyright real) |
| Abas: uma sessão por aba (`:tabnew [arquivo]`, `gt`/`gT`, `:tabc[!]`, `:tabo`, `:q` fecha a aba), barra de abas clicável, servidor LSP compartilhado, explorador e terminal acompanham a aba ativa | **funcionando** |
| Janelas divididas: `:split` `:vsplit` `:new` `:vnew` `:close` `:only` e `<C-w>` `s v n h j k l w W p c q o = _ \|`; clique foca o painel, a roda rola o painel sob o ponteiro, arrastar o divisor redimensiona. Painéis do mesmo arquivo compartilham o texto (edição, salvar e modificado acompanham); cada painel tem cena, rolagem e cursor próprios, e o focado leva a pílula | **funcionando** |
| Camada de tela nativa no app (`ASTROGRAPH_ENGINE=native [arquivo]`): numeração, realce tree-sitter, seleção visual, statusline, `:w :q :wq :e :N`, rolagem e clique | **funcionando** (verificado no app real sob Xvfb, sem Neovim) |

## Experimentar

```
ASTROGRAPH_ENGINE=native astrograph arquivo.rs
```

Abre o arquivo no motor nativo (sem Neovim): todos os movimentos, operadores,
objetos de texto, visual, desfazer e `.` descritos acima, mais `:w`, `:q`, `:q!`,
`:wq`, `:e arquivo` e `:<número>`. Sem arquivo, abre a tela inicial do Astrograph
(`n` novo, `f` buscar arquivo, `o` recentes, `w` buscar palavra). O padrão do app é o motor nativo; `ASTROGRAPH_ENGINE=nvim` (ou `[engine] native = false`)
volta ao Neovim.

## Desempenho

Medido no motor nativo com um arquivo de **200 mil linhas** (14 MB), CPU, sem animações
(`cargo test --release bench_arquivo_grande -- --ignored --nocapture` em `prototype/astrograph-ui`; cada número
inclui montar o modelo da tela depois da tecla):

| Operação | Custo |
| --- | --- |
| abrir o texto na sessão | ~60 ms |
| primeiro quadro | ~3 ms |
| `j`, `20j`, `G` e `gg` | menos de 10 µs |
| digitar um caractere (`Ax<Esc>`) | ~1 ms |
| `u` (desfazer) | ~1 ms |
| `dd` | ~3 ms |
| `/padrão` até o fim do arquivo | ~95 ms |
| `:%s/a/b/g` nas 200 mil linhas | ~0,8 s |

O quadro de pixels (1280×800) custa ~2 ms; sem animação o app não repinta. As buscas de arquivos e de texto rodam em
threads (a lista de arquivos tem cache por raiz), então não travam a digitação.

## Divergências conhecidas

- Mapeamentos: sem `timeoutlen` (um prefixo pendente espera a próxima tecla; `<Esc>` solta), sem `omap`/`cmap`/`<expr>`,
  e os feitos com `:map` valem só até fechar o app (os permanentes vão em `[keys.normal]` etc.).
- Ainda não existem: quebra de linha (`wrap`) e rolagem horizontal suave, IME/teclas mortas validados
  no Windows e no macOS, modelo de plugins sem Lua, abreviações, `=`/`!` (filtros), `:r`, `:w !cmd`, `"%`, `:registers`.
- Git: o branch e o `HEAD` são lidos em thread; arquivos com mais de 5000 linhas têm os sinais da margem
  calculados em segundo plano (os sinais antigos ficam até o cálculo terminar).
- Terminal: links OSC 8 não são tratados; o título do processo não aparece na pílula.
- LSP: criar/renomear/apagar arquivos pedidos pelo servidor são ignorados; renomear em arquivos fechados grava direto
  no disco; só foram conferidos de verdade o pyright e um servidor falso (os demais seguem o protocolo).
- IA inline: usa o CLI do agente em modo de uma resposta só (`claude -p`, `opencode run`, `codex exec`, `gemini -p`,
  `ollama run`), não o protocolo ACP: a proposta entra inteira (um `u` desfaz), sem aceitar/rejeitar por trecho.
- Janelas divididas: dois painéis do mesmo arquivo espelham o texto, mas cada um guarda o próprio
  histórico de desfazer (uma edição feita no outro painel entra como um passo; no Vim o histórico é um só).
- Cursor depois de `u` quando o Vim guarda várias entradas de undo para um comando
  (`cj` + texto, `<C-w>` além do começo da inserção, por exemplo): o texto fica certo, a coluna/linha do cursor às vezes
  difere.
- `v$d` seguido de `.`, `2d%` com contagem maior que a linha, e comandos que o Vim executa como bloco de undo único
  junto de outro (`>>dGu`).

## Ordem do trabalho

1. **Núcleo** (este PR): movimentos, operadores, registradores, inserção, undo, `.`.
2. Visual, objetos de texto (`iw aw i( a" ip...`) usando as consultas do tree-sitter,
   busca, marcas, macros, `:s` `:g` `:w` e afins.
3. Camada de tela: o app passa a desenhar o motor nativo (linhas, numeração, status,
   cursor, realce) no mesmo renderer, e o Neovim sai do caminho do app.
4. Funcionalidades hoje vindas de plugins, reescritas nativas: busca de arquivos e
   texto, explorador, git, diagnósticos/formatação, LSP, completar, terminal, agentes.
5. Empacotar tudo (sem baixar nada na instalação ou na primeira abertura).
