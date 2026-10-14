# Design — Modo agentic (CodeCompanion + terminais CLI + avante/Ollama)

- **Data:** 2026-10-14
- **Status:** aprovado em revisão pelo usuário (seções 1–6)
- **Repo:** `Just-Gabe/Astrograph` (config viva: `%LOCALAPPDATA%\nvim` → `template/`)

## Contexto

Hoje o Astrograph só traz o `astrocommunity.ai.claudecode-nvim` (`<Leader>Ac` etc.). Queremos
um **modo agentic** fluido e fácil de integrar com Claude, opencode e outros agentes —
inclusive **agentes locais e Ollama** — dentro do Neovim. A experiência deve ser unificada
(um prefixo só) e instalável de forma opcional pelo instalador de uma linha.

## Decisões do usuário

| Tema | Escolha |
|---|---|
| Superfície | **Terminal CLIs + chat local** |
| Agentes | **Claude Code, opencode, Codex CLI, Gemini CLI, Ollama local** |
| Abordagem | **CodeCompanion unificado + terminais + avante** |
| opencode | **ACP + CLI** (ACP como fallback; CLI tem prioridade por bug conhecido do ACP) |
| Instalador | **Menu interativo com checklist `[ ]`**, sem flag; passo de configuração depois |
| Interação do menu | **A** — arrows/espaço/enter, com fallback numerado e não-interativo |

## Alternativas consideradas

1. **Terminais CLI + avante para local** — CLIs com UI nativa + avante inline. Descartada
   sozinha: dois sistemas, menos unificado que o CodeCompanion.
2. **Mínimo incremental** (manter `claudecode-nvim` + CodeCompanion só p/ Ollama) — Descartada:
   dois plugins de agente, keymaps inconsistentes.
3. **CodeCompanion unificado + terminais + avante** — *escolhida*: uma engine cobre HTTP
   (Claude/Gemini/OpenAI/Ollama/OpenAI-compatível), ACP (claude_code/codex/gemini_cli/opencode)
   e interação CLI; terminais dão os TUIs nativos; avante cobre o inline estilo Cursor com local.

## Arquitetura e componentes

```
template/lua/astrograph/agents.lua      -- núcleo compartilhado (detecção + helpers)
template/lua/plugins/codecompanion.lua  -- engine unificada (HTTP + ACP + CLI)
template/lua/plugins/avante.lua         -- inline/chat local (Ollama, Claude)
template/lua/plugins/terminals.lua      -- launcher de CLIs em terminal flutuante
community.lua                           -- remove claudecode-nvim (superseded)
template/lua/plugins/animations.lua     -- corrige conflito de keymap (ver Guardas)
install.ps1 / install.sh                -- menu de agentes + passo de configuração
```

**Prefixo único de IA: `<Leader>A`** (verificado livre no `core/`). Observação: o AstroCommunity
usa `<Leader>A` em avante, codecompanion e claudecode — por isso **não** importamos esses
módulos; escrevemos specs próprios para ter keymaps não sobrepostos e adapters dinâmicos.

### `template/lua/astrograph/agents.lua` (núcleo)

- `detect()` — tabela com `claude`, `opencode`, `codex`, `gemini` (via `vim.fn.executable`),
  `ollama_up`, `local_model`. Cacheado (evita re-checar no boot).
- `ollama_up()` — checa `OLLAMA_HOST`/`http://localhost:11434` com `vim.system` e **timeout
  curto (~300 ms)**; lista modelos via `/api/tags`.
- `has(bin)` — `vim.fn.executable(bin) == 1`.
- `pick_local_model()` — ordem: `ASTROGRAPH_LOCAL_MODEL` (env) → config gravado pelo instalador
  (`%LOCALAPPDATA%\astrograph\config.json` / `~/.config/astrograph/config.json`) → primeiro
  modelo do Ollama.
- `terminal_cmd(name)` — comando base de cada CLI (`claude`, `opencode`, `codex`, `gemini`).
- API sempre tolerante: se Ollama não responde ou binário ausente, retorna `false`/`nil` sem
  erro (consumidores tratam).

## CodeCompanion (engine unificada)

Spec próprio (`plugins/codecompanion.lua`), carregado sob demanda (`cmd` + `User AstroFile`).

**Adapters** montados conforme `agents.detect()`:

| Tipo | Adapters | Onde roda |
|---|---|---|
| HTTP | `anthropic`, `openai`, `gemini`, `ollama`, `openai_compatible` | chat/inline/cmd/background |
| ACP | `claude_code`, `codex`, `gemini_cli`, `opencode` | só chat buffer (with approvals) |
| CLI | `claude_code` (`cmd=claude`), `opencode`, `codex`, `gemini` | TUI nativa em terminal |

**Defaults por detecção:** `chat` → `claude_code` (se `has("claude")`) → `ollama` (se vivo) →
`anthropic`; `inline`/`cmd`/`background` → `ollama` → `anthropic` (inline/cmd exigem HTTP).
O adapter `openai_compatible` (endpoints locais tipo LM Studio/llama.cpp/vLLM) só é registrado
quando `ASTROGRAPH_LOCAL_URL` está definido; o modelo vem de `ASTROGRAPH_LOCAL_MODEL`.

**Keymaps** (visual onde indicado):

| Tecla | Ação |
|---|---|
| `<Leader>Ac` | toggle do chat |
| `<Leader>An` | novo chat |
| `<Leader>Aq` | assistente inline (seleção) |
| `<Leader>Ap` | paleta de ações |
| `<Leader>Aa` | adicionar seleção/buffer ao chat (visual) |
| `<Leader>Am` | trocar modelo/adapter |
| `<Leader>Al` | lançar agente-CLI (picker) |
| `<Leader>Ax` | limpar chat |

**Prompt library:** prompts curtos do Astrograph (explicar, revisar, testes, refatorar,
commit-msg). Painéis Markdown integrados a `render-markdown` (filetype `codecompanion`).

## Terminais dos CLIs

- Terminal do **Snacks** em float, no root do projeto (`astrocore.root`/cwd); herda env do shell.
- Só registra keymap de CLIs presentes no PATH (`agents.detect()`).
- `<Leader>Al` → picker (Snacks) com agentes detectados → abre TUI em float.
- `<Leader>AT` → reabre/esconde o último terminal de agente (toggle).
- `<Leader>A.` → insere o caminho do arquivo atual (contexto rápido).
- Fallback `vim.cmd.terminal` se o Snacks não estiver pronto.

## avante (local / estilo Cursor)

Spec próprio (`plugins/avante.lua`).

- `provider = "ollama"` quando `ollama_up()`, senão `claude` (env). `selector.provider = "snacks"`.
- `auto_suggestions = false` por padrão (caro); ligável.
- Keymaps **próprias** (não as `<Leader>A*` do AstroCommunity), sem sobreposição com o
  CodeCompanion: `<Leader>AE` ask/edit na seleção · `<Leader>At` toggle sidebar ·
  `<Leader>As` toggle suggestions · `<Leader>Af` focus · `<Leader>Az` zen mode.
- **Divisão de trabalho:** CodeCompanion = agentes/CLIs/chat amplo; avante = edição inline
  estilo Cursor com modelo local.

## Instaladores — menu de agentes

`install.ps1` e `install.sh`: após instalar o essencial (Neovim/toolchain), mostram um checklist.

```
  Agentes a instalar  (↑/↓ navega · espaço marca · enter confirma)
  ─────────────────────────────────────────────────────────────
  [x] Claude Code        (npm  @anthropic-ai/claude-code)
  [ ] opencode           (npm  opencode-ai)
  [x] Codex CLI          (npm  @openai/codex)
  [ ] Gemini CLI         (npm  @google/gemini-cli)
  [ ] Ollama + modelo    (local, offline)
```

- **Interação A:** tecla crua, arrows + espaço + enter. Item já instalado aparece
  `[✓] (instalado)` e é pulado.
- **Fallbacks:** sem TTY → lista numerada (`digite "1 3 5"`); ainda sem TTY →
  `ASTROGRAPH_AGENTS` (env, ex.: `claude,opencode,ollama:qwen2.5-coder`); senão **pula agentes**.
- **Node.js LTS** entra automaticamente se algum CLI npm for marcado e faltar.
- **Ollama/modelo:** inicia serviço se preciso, mostra mini-menu de modelos sugeridos
  (ex.: `qwen2.5-coder:7b`, `qwen3:8b`, "outro/nenhum") → `ollama pull`.
- **Nunca autentica:** imprime/abre os próximos passos (`claude`, `opencode auth login`, etc.).
- **Persistência:** grava o modelo local escolhido no `config.json` lido por `agents.lua`.
- **Idempotente**, falha de um item não aborta os outros; comandos exatos dos CLIs validados
  na implementação.

Bash: como `curl | bash` consome o stdin, os prompts leem de `/dev/tty`; sem TTY cai no modo
não interativo.

## Robustez

- Detecção cacheada e com timeout — **não bloqueia o boot**.
- CodeCompanion/avante **lazy** (`cmd`/`User AstroFile`): custo de startup zero.
- Fail-safes: sem Ollama → pula adapter/default local; sem API key → erro amigável; CLI
  ausente → keymap não criado; plugin ausente → keymap protegido com `pcall`.
- `/dev/tty` no bash evita consumir o script em `curl | bash`.

## Guardas

- **Correção de keymap existente:** `animations.lua` hoje põe scroll suave em `<Leader>uC`,
  que o core usa para *Toggle autocompletion (global)* (`core/.../blink.lua:219`). Mover o
  scroll suave para `<Leader>uS` (livre) e atualizar comment/README.
- `claudecode-nvim` removido do `community.lua` (superseded pelo CodeCompanion) — evita
  colisão de `<Leader>A*` e plugin duplicado.

## Verificação

1. `loadfile` sem erro em todos os arquivos novos.
2. Headless: `nvim --headless "+lua print(vim.inspect(require('astrograph.agents').detect()))" +qa`
   imprime a tabela de detecção.
3. Headless: specs de CodeCompanion/avante carregam sem erro (`:messages` limpo).
4. Manual: `<Leader>A` mostra o grupo no which-key; `<Leader>Ac` abre o chat; `ga` lista
   adapters/modelos incl. `ollama`; `<Leader>Al` abre um CLI; `<Leader>AT` alterna o terminal.
5. Instalador: `bash -n install.sh` + parse do `install.ps1`; menu aparece e o modo
   não-interativo (`ASTROGRAPH_AGENTS`) funciona.

## Fora de escopo

- Alterar o `core/` do AstroNvim (exceto nada; a correção de keymap é no `template/`).
- Implementação da dashboard animada (spec separada, retomada depois).
- RAG/embeddings locais, MCP servers, ou treino de modelos.
