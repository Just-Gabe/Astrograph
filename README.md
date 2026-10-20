# Astrograph

**Um editor pronto para usar**, baseado no [AstroNvim](https://github.com/AstroNvim/AstroNvim),
com **agentes de IA dentro do editor**, abertura mais rápida e tudo em **português**.
O app em Rust é o principal; o Neovim com a config do Astrograph segue como espelho
para quem prefere o terminal.

📖 **Documentação:** <https://just-gabe.github.io/Astrograph/>

![Abertura animada do Astrograph](docs/img/abertura.gif)

## Instalar

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
```

**Linux / macOS** (bash):

```bash
curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh | bash
```

O instalador cuida de tudo: Neovim 0.11+, Git, ripgrep, fd, compilador C, a
JetBrainsMono Nerd Font, o clone do Astrograph, o app em Rust (baixado do release ou
compilado, instalando Rust e linker se faltarem) e a ligação da config. Se já houver
uma config do Neovim, ele pergunta se limpa e substitui, importa seus plugins ou
mantém. No fim, um checklist opcional instala agentes de IA: Claude Code, opencode,
Codex, Gemini e Ollama. Em seguida ele instala o que o chat precisa e oferece
fazer o **login de cada agente** ali mesmo. Depois é só abrir um terminal novo e
rodar `nvim`.

[Mais detalhes da instalação →](https://just-gabe.github.io/Astrograph/instalacao)

## O que vem pronto

- **Agentes de IA no prefixo `<Leader>A`**: Claude Code, opencode, Codex ou Gemini
  no terminal flutuante e no chat, usando o login de cada CLI (sem chave de API);
  edição inline com o avante; IA local com Ollama. `:Astrograph configurar`
  instala e faz login no que faltar.
- **Em português**: menu de atalhos, tela inicial, `:checkhealth astrograph` e
  mensagens.
- **Mais rápido**: abre em ~256 ms no Windows (o AstroNvim original, ~392 ms);
  arquivos abrem em estágios, com o essencial em ~59 ms.
- **Visual no estilo Apple**: tema grafite inspirado no Xcode (cores calmas, um
  destaque só), tela inicial limpa e barra de status discreta; linha de comando
  com destaque, cursor animado, bordas arredondadas e ghost text.
- **Ações com o agente**: corrigir o erro da linha (`<Leader>Ax`), explicar
  (`<Leader>Ai`), revisar suas alterações (`<Leader>Ar`) e escrever a mensagem
  de commit (`<Leader>Am`). O editor avisa antes se o agente estiver sem login.
- **Resposta imediata**: o que você usa é pré-carregado quando o editor está
  ocioso, e o código aparece sem ruído (sem CodeLens nem `~` por padrão).
- **Movimento**: janelas com fade, explorador deslizante e `dd` animado (a
  linha colapsa e some), sempre sem atrasar a edição.
- **Um agente para tudo**: escolha com `<Leader>Ag` e terminal, chat e edição
  inline passam a usar o mesmo agente; a escolha fica salva. O modo (Manual,
  Aceitar edições, Plano, Auto) também: `<Leader>Aw` vale para todas as telas.
- **Código próprio onde importa**: cores e TODOs desenhados pelo motor nativo
  do Neovim, no lugar de dois plugins; rolar a tela ficou ~10× mais leve.
- **Tema próprio**: `astrograph` (escuro) e `astrograph-light`, com contraste
  verificado.
- **Arquivos editados pelo agente recarregam sozinhos** no editor.
- **Sessões de todos os projetos** (`<Leader>AS`): as conversas do agente com a
  tag do modelo que as fez; retomar volta para a pasta e o agente da conversa.
- **Plugins só desta máquina**: specs do lazy em
  `%LOCALAPPDATA%\astrograph-local\specs` (ou `$ASTROGRAPH_LOCAL`), fora do git.
  São código Lua executado na abertura do editor: ponha ali só o que é seu.
- **Gerenciador de arquivos TUI**: com yazi, lf, nnn, ranger ou vifm instalado,
  `<Leader>fm` (arquivo atual), `<Leader>fM` (raiz do projeto) e `<Leader>fv`
  (divisão vertical) abrem o gerenciador; na árvore, `T` abre a pasta sob o cursor
  (via [tfm.nvim](https://github.com/Rolv-Apneseth/tfm.nvim)).
- **`:Astrograph`**: `atualizar`, `saude`, `agentes`, `configurar`, `login`, `sessoes`, `atalhos`, `docs`.

| Editor | Menu de atalhos (`Espaço`) |
| --- | --- |
| ![Editor](docs/img/editor.png) | ![Atalhos](docs/img/atalhos.png) |
| **Agentes de IA (`Espaço` `A`)** | **Chat com o Claude Code (`Espaço` `A` `c`)** |
| ![Agentes](docs/img/agentes.png) | ![Chat](docs/img/chat.png) |
| **Completação com ghost text** | **Diagnóstico (`:Astrograph saude`)** |
| ![Completação](docs/img/completar.png) | ![Diagnóstico](docs/img/saude.png) |
| **Explorador deslizando (`<Leader>e`)** | **`dd`: a linha colapsa e some** |
| ![Explorador](docs/img/explorador.gif) | ![Apagar linha](docs/img/apagar-linha.gif) |

### UI nativa (em desenvolvimento)

Uma janela própria em Rust ([`prototype/astrograph-ui`](prototype/astrograph-ui)),
com dois motores: o Neovim (padrão) e o **motor nativo em Rust, sem Neovim**
(`ASTROGRAPH_ENGINE=native astrograph-ui [arquivo]`, ou `[engine] native = true` no
`config.toml` da UI para abrir sempre nele), que reproduz os atalhos do
Vim e é testado contra o Neovim de verdade (mais de 1000 sequências de teclas).
Cobre movimentos, operadores, objetos de texto, modo visual, busca, registradores,
desfazer/refazer, comandos `:w :q :wq :e`, macros (`q`, `@`), um **buscador próprio** (com a tecla líder,
`<Espaço>` por padrão: `<Espaço>ff` arquivos, `<Espaço>fw` texto, `<Espaço>fo` recentes, sem `fd`/`ripgrep`) e um
**explorador de arquivos** (`<Espaço>e`); o restante (visual em bloco) ainda é do Neovim. A tecla líder é configurável
(`[keys] leader = ","` no `config.toml` da UI); o app mostra o nome da tecla
escolhida (por exemplo `<Espaço>f` ou `<,>f` enquanto você digita o atalho).

No motor nativo o visual deixa de ser uma grade de terminal e vira uma cena de
pixels no estilo Apple (fonte Inter na interface, cantos arredondados, sombras)
com animações próprias, todas em mola:

- rolagem suave com overscroll elástico; cursor que desliza e pisca com fade;
- clarão ao copiar, realce da busca com fade, linhas apagadas/coladas que deslizam;
- paleta de comandos com fade, avisos que deslizam, pílula de modo na base;
- guias de recuo, destaque do par de colchetes e a mesma tela inicial do Astrograph;
- buscador com fade, trecho que casou em destaque e seleção que desliza;
- explorador que desliza da esquerda e empurra o texto;
- LSP nativo (rust-analyzer, pyright, typescript-language-server, gopls, lua-language-server,
  clangd e bash-language-server, do PATH ou do Mason): diagnósticos com sublinhado ondulado,
  ponto na margem e mensagem ao fim da linha, `K` (hover) e `gd` (ir para a definição),
  contagem de erros e avisos na pílula (`ASTROGRAPH_LSP=0` desliga);
- git na margem (linha nova, alterada ou removida, ao vivo enquanto você digita, sem
  salvar) e o branch na pílula;
- terminal nativo (`<Espaço>t`) que sobe da base, com cores e cursor, onde o agente de IA
  instalado (claude, opencode, codex ou gemini) roda via `<Espaço>Al`; `<C-\><C-n>` devolve o
  teclado ao texto e `<Espaço>T` fecha;
- relógio pequeno no canto inferior direito (também no app com Neovim), útil em tela
  cheia; `[clock] enabled = false` desliga e `hour12 = true` mostra `3:07 PM`.

Um quadro completo custa ~2 ms em 1280×800 (CPU); sem animação, o app não repinta.

| Editor | Busca |
| --- | --- |
| ![Editor nativo](docs/img/ui/nativo-editor.png) | ![Busca nativa](docs/img/ui/nativo-busca.png) |
| **Paleta de comandos** | |
| ![Paleta nativa](docs/img/ui/nativo-paleta.png) | |

No app com Neovim, o texto desliza por pixel ao rolar, o cursor desliza e pisca com
fade, e as janelas flutuantes ganham sombra:

| Abertura | Rolagem por pixel |
| --- | --- |
| ![Abertura na UI nativa](docs/img/ui/abertura.gif) | ![Rolagem suave](docs/img/ui/rolagem.gif) |
| **Cursor: desliza, muda de forma e pisca com fade** | **Busca com sombra** |
| ![Cursor](docs/img/ui/cursor.gif) | ![Painel com sombra](docs/img/ui/painel.gif) |

Em segundo plano o app prepara o projeto (git, parsers, ferramentas) com um
indicador discreto no canto; `:Astrograph atualizar` é transacional, com backup e
rollback automático. Detalhes em [`docs/motor-nativo.md`](docs/motor-nativo.md).

## Primeiros passos

| Atalho | O que faz |
| --- | --- |
| `Espaço` | Mostra todos os atalhos (espere meio segundo) |
| `<Leader>ff` / `<Leader>fw` | Buscar arquivo / texto |
| `<Leader>e` | Explorador de arquivos |
| `<Leader>w` / `<Leader>q` | Salvar / fechar janela |
| `<Leader>Al` | Abrir um agente de IA no terminal |
| `<Leader>Ac` | Chat com IA |
| `<Leader>AE` | Pedir uma edição para a IA (selecione o trecho antes) |
| `<Leader>Ax` / `<Leader>Ar` | Corrigir o erro da linha / revisar suas alterações |
| `<Leader>AS` | Sessões do agente de todos os projetos (retomar) |
| `<Leader>um` | Ligar/desligar as animações |

`<Leader>` é a tecla **Espaço**. Veja a [lista completa](https://just-gabe.github.io/Astrograph/uso)
e o [guia dos agentes](https://just-gabe.github.io/Astrograph/agentes).

## Estrutura

```
core/       fork do core do AstroNvim (v6.1.0) com as otimizações
template/   a config que o Neovim abre
theme/      colorschemes astrograph e astrograph-light
tools/      geradores da arte da tela inicial e dos prints
prototype/  o app em Rust: astrograph-ui (janela), astrograph-core (motor modal),
            astrograph-syntax (tree-sitter), astrograph-servicos (preparo e
            atualização), astrograph-agent
docs/       documentação (GitHub Pages) e prints
```

[Como funciona por dentro →](https://just-gabe.github.io/Astrograph/desenvolvimento) ·
[Performance →](https://just-gabe.github.io/Astrograph/performance) ·
[Astrograph e AstroNvim →](https://just-gabe.github.io/Astrograph/astronvim) ·
[Solução de problemas →](https://just-gabe.github.io/Astrograph/problemas)

## Créditos e licença

Baseado no [AstroNvim](https://github.com/AstroNvim/AstroNvim) (GPL-3.0), de Micah
Halter ([mehalter](https://github.com/mehalter)) e colaboradores; usa o lazy.nvim e o
snacks.nvim de Folke Lemaitre. Mantido por [Just-Gabe](https://github.com/Just-Gabe).
O `core/` mantém a licença original; veja o [`LICENSE`](./LICENSE).
