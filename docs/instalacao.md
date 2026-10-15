---
title: Instalação
nav_order: 2
---

# Instalação
{: .no_toc }

1. TOC
{:toc}

## Um comando

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
```

**Linux / macOS** (bash):

```bash
curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh | bash
```

Depois abra **um terminal novo** e rode `nvim`. Na primeira abertura os parsers do
Treesitter terminam de compilar em segundo plano.

## O que o instalador faz

| Passo | Windows | Linux / macOS |
| --- | --- | --- |
| Dependências | `winget`: Neovim, Git, ripgrep, fd, gcc (WinLibs) | apt/dnf/pacman/zypper/apk ou Homebrew |
| Neovim 0.11+ | via winget | o do sistema; se for antigo, baixa o oficial (Linux) ou `brew upgrade` (macOS) |
| Fonte | JetBrainsMono Nerd Font (por usuário, sem admin) e ajusta o Windows Terminal | `~/.local/share/fonts` + `fc-cache` |
| Código | clona em `%LOCALAPPDATA%\Astrograph` | clona em `~/Astrograph` |
| Config | liga `%LOCALAPPDATA%\nvim` → `Astrograph\template` | liga `~/.config/nvim` → `~/Astrograph/template` |
| Plugins | `Lazy! restore` + ferramentas do Mason | igual |
| Agentes | checklist opcional (Claude Code, opencode, Codex, Gemini, Ollama) | igual |
| Login dos agentes | instala a ponte do chat e oferece login em cada agente instalado | igual |

{: .note }
Os dois scripts podem rodar de novo sem problema: atualizam o clone, mantêm a
config ligada e fazem **backup** de uma config antiga do Neovim antes de trocar.

## Instalação sem perguntas

Sem terminal interativo (ou para automatizar), escolha os agentes por variável:

```powershell
$env:ASTROGRAPH_AGENTS = 'claude,ollama:qwen2.5-coder:7b'
irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
```

```bash
ASTROGRAPH_AGENTS="claude,opencode" bash -c "$(curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh)"
```

Valores aceitos: `claude`, `opencode`, `codex`, `gemini`, `ollama` e
`ollama:<modelo>` (baixa o modelo e o grava como modelo local).

## Login dos agentes

No fim, o instalador passa por **todos** os agentes instalados (inclusive os
que já existiam):

1. instala a ponte do chat que faltar (Claude Code e Codex);
2. confere o login de cada um;
3. pergunta se quer entrar agora (`[S/n]`) e abre o login no navegador.

O que ficar pendente aparece listado no fim, e dá para resolver depois no
editor com `:Astrograph configurar`. Para pular as perguntas, defina
`ASTROGRAPH_NO_LOGIN=1`.

## Atualizar

Dentro do editor:

```vim
:Astrograph atualizar
```

Isso faz `git pull` no clone e sincroniza os plugins com o `lazy-lock.json` do
repositório. Reinicie o Neovim depois. Rodar o instalador de novo também atualiza.

## Requisitos

- Neovim **0.11+**
- Uma [Nerd Font](https://www.nerdfonts.com/font-downloads) configurada no terminal (o instalador instala a JetBrainsMono)
- `git`; `ripgrep` e `fd` para as buscas (o Mason também os instala)
- Um compilador C para os parsers do Treesitter

## Desinstalar

1. Apague a ligação da config: `%LOCALAPPDATA%\nvim` (Windows) ou `~/.config/nvim`.
2. Apague o clone: `%LOCALAPPDATA%\Astrograph` ou `~/Astrograph`.
3. Opcional: apague os dados (plugins, Mason, histórico) em `%LOCALAPPDATA%\nvim-data`
   ou `~/.local/share/nvim`.
4. Se você tinha uma config antes, ela está em `nvim.backup-<data>` ao lado.
