# Astrograph

**Astrograph** é a minha versão pessoal do [AstroNvim](https://github.com/AstroNvim/AstroNvim):
um monorepo que reúne o **core** do AstroNvim e o **template** (a configuração do usuário),
com otimizações e ajustes próprios.

## Estrutura

```
Astrograph/
├── core/       # Nossa versão do core (base: AstroNvim v6.1.0)
└── template/   # Nossa configuração de usuário (derivada do AstroNvim/template)
```

- `core/` — código-fonte do AstroNvim (plugins, engine de configuração, UI via astroui/astrocore/astrolsp).
- `template/` — a config que você abre no Neovim (`init.lua`, `lua/plugins/*`, etc.).

## Mudanças e otimizações

### `template/`
- **`lua/plugins/mason.lua`** — instala `fd` e `ripgrep` via Mason de forma **cross-platform**
  (Windows/Linux/macOS). São as dependências do *Snacks picker*:
  - `fd` → `<Leader>ff`, `<Leader>fF`, `<Leader>fs` (buscar arquivos)
  - `ripgrep` → `<Leader>fw`, `<Leader>fW` (live grep)
  - Há uma guarda com `mason-registry.has_package` para nunca abortar a instalação dos
    outros pacotes quando o registry estiver incompleto/offline.
- **`lua/plugins/animations.lua`** — animações no estilo *Claude/opencode*, via Snacks:
  - **`animate`** — base rápida (`fps = 120`) com easing `outQuad` (suave, sem overshoot);
  - **`scroll`** — scroll suave (`outCubic`), com animação mais curta ao segurar a tecla;
  - **`indent`** — reativa a animação das indent guides (o AstroNvim desliga por padrão);
  - **`win`** — backdrop levemente escurecido nos floats (notify, lazy, picker).
  - Atalhos: `<Leader>ua` liga/desliga **todas** as animações; `<Leader>uS` só o scroll suave.
- **`lua/polish.lua`** — otimização geral:
  - `vim.loader.enable()` (cache de bytecode Lua → startup mais rápido);
  - desabilita os providers Perl/Ruby não usados (evita checagem no startup);
  - `smoothscroll` ligado (complementa o scroll suave do Snacks);
  - mantém o `CC=gcc` no Windows (treesitter compila com WinLibs).

### Modo agentic (agentes no editor)

Configurado em `template/lua/plugins/` com um núcleo de detecção em
`template/lua/astrograph/agents.lua`. Prefixo único de IA: **`<Leader>A`**.

- **`codecompanion.lua`** — engine unificada (CodeCompanion):
  - **HTTP** → Claude, OpenAI, Gemini, **Ollama** e endpoints locais OpenAI-compatíveis;
  - **ACP** → `claude_code`, `codex`, `gemini_cli`, `opencode` (no chat buffer);
  - **CLI** → TUIs nativos recebendo contexto do buffer.
  - Teclas: `<Leader>Ac` (chat), `<Leader>An` (novo chat), `<Leader>Aq` (inline),
    `<Leader>Ap` (ações/adapter·modelo), `<Leader>Aa` (adicionar seleção). Dentro do chat, `ga` troca adapter/modelo.
- **`terminals.lua`** — lança os CLIs num terminal flutuante (Snacks):
  `<Leader>Al` (lançar/agente), `<Leader>AT` (alternar), `<Leader>A.` (enviar arquivo atual).
- **`avante.lua`** — edição inline estilo Cursor, priorizando o modelo local (Ollama):
  `<Leader>AE` (perguntar/editar), `<Leader>At`/`<Leader>As`/`<Leader>Af`/`<Leader>Az`.
- Detecção automática: só habilita o que existe (Ollama no ar, CLIs no PATH); sem custo de startup.

### Performance (startup ~390 ms → ~280 ms no Windows)

Medido com `nvim --startuptime` e o profile do lazy.nvim. O que pesava não era o runtime
legado do Vim (~18 ms no total), e sim checagens de binário e plugins carregados cedo:

- **`core/` — `require("astronvim").executable(bin)`**: cache por sessão de `vim.fn.executable`.
  No Windows cada busca de um binário **ausente** varre o PATH inteiro (~20 ms cada), e o core
  repetia `git`/`rg` em vários specs.
- **`core/.../toggleterm.lua`**: os atalhos de lazygit/node/gdu/btm/python são registrados logo
  após o startup (`VeryLazy`, um binário por tick), sem bloquear a abertura. Overrides do
  usuário continuam valendo.
- **`core/.../_astroui.lua`**: sem `lazygit` instalado, o astroui não regrava o YAML do tema dele
  a cada startup.
- **`avante.lua`**: carrega só pelas teclas/comandos (antes: em todo primeiro arquivo aberto,
  ~100 ms + detecção do Ollama). Primeiro arquivo aberto: ~230 ms → ~120 ms.
- **Runtime legado**: `rplugin` e `tutor` desligados; providers perl/ruby/python3/node desligados
  (nenhum plugin usa); `vim.loader` ativado no topo do `init.lua`, antes do lazy.nvim.
- **`astrograph.agents`**: detecção de CLIs cacheada (`detect(true)` força nova checagem).

### `lua/lazy_setup.lua` (template)
- Passa a **preferir o core local** do monorepo (`../core`) quando existir, caindo para o
  AstroNvim oficial caso contrário. Isso permite testar mudanças no core sem publicar nada.

## Requisitos

- Neovim **0.11+**
- Uma [Nerd Font](https://www.nerdfonts.com/font-downloads) instalada e configurada no terminal
- Um compilador C no PATH (para os parsers do Treesitter)
- `git` (obrigatório); `fd` e `ripgrep` são instalados automaticamente via Mason

## Instalação (uma linha)

Em uma máquina limpa, o instalador cuida de tudo: Neovim, Git, `ripgrep`, `fd`, compilador C,
Nerd Font JetBrainsMono, o clone do Astrograph e a ligação da config (Lazy + Mason inclusos).

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
```

**Linux / macOS** (bash):

```bash
curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh | bash
```

Depois é só abrir um terminal novo e rodar `nvim`. Os dois scripts são **idempotentes**
(podem rodar de novo) e fazem **backup** da config antiga antes de ligar a nova.

No fim, aparece um **checklist** (`[ ]`) pra escolher agentes opcionais — Claude Code, opencode,
Codex CLI, Gemini CLI e Ollama (com download de modelo) — seguido dos passos de login. Sem
terminal interativo, use a variável `ASTROGRAPH_AGENTS` (ex.: `ASTROGRAPH_AGENTS="claude,ollama:qwen2.5-coder:7b"`).

## Como usar

Copie (ou faça symlink de) `template/` para o diretório de config do Neovim:

- **Windows:** `%LOCALAPPDATA%\nvim`
- **Linux/macOS:** `~/.config/nvim`

Durante o desenvolvimento do core, aponte a config para este checkout — o `lazy_setup.lua`
detecta automaticamente o `../core` (ou use a variável de ambiente `ASTROGRAPH_CORE`):

```powershell
$env:ASTROGRAPH_CORE = "C:\caminho\para\Astrograph\core"
nvim
```

## Créditos e licença

Baseado em [AstroNvim](https://github.com/AstroNvim/AstroNvim) (GPL-3.0), de Folke Lemaitre
e colaboradores. Este repositório é mantido por [Just-Gabe](https://github.com/Just-Gabe).
O `core/` mantém a licença original; veja [`LICENSE`](./LICENSE).
