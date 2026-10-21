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
| App e agente nativo (Rust) | baixa do GitHub Releases (SHA256 conferido); sem release, instala o Build Tools do Visual C++ e o Rust e compila | instala o Rust (rustup) e compila |
| Plugins | `Lazy! restore` + ferramentas do Mason | igual |
| Agentes | checklist opcional (Claude Code, opencode, Codex, Gemini, Ollama) | igual |
| Login dos agentes | instala a ponte do chat e oferece login em cada agente instalado | igual |

{: .note }
Os dois scripts podem rodar de novo sem problema: atualizam o clone, mantêm a
config ligada e restauram arquivos da config que tenham sumido do clone.

## Já tenho uma config do Neovim

Se já existir uma config em `%LOCALAPPDATA%\nvim` (ou `~/.config/nvim`), o
instalador pergunta o que fazer:

| Opção | O que acontece |
| --- | --- |
| **1. Limpar e substituir** (padrão) | a config vai para `nvim.backup-<data>` e os dados (plugins, Mason, histórico) para `nvim-data.backup-<data>`; o Astrograph começa do zero |
| **2. Importar** | faz o mesmo backup da config e copia para os plugins locais (`astrograph-local`): os specs de `lua/plugins`, `polish.lua`, `community.lua`, os demais módulos de `lua/`, `options`/`keymaps`/`autocmds` do LazyVim e as pastas `after/`, `ftplugin/`, `snippets/`, `spell/`... A lista fica em `astrograph-local/IMPORTADO.txt` |
| **3. Manter** | não mexe na config do Neovim; instala o resto (clone, app, agentes) |

Uma cópia antiga do próprio Astrograph vai direto para backup (importar
duplicaria os plugins). Sem terminal interativo vale a opção 1; para escolher,
defina `ASTROGRAPH_CONFIG_EXISTENTE=limpar`, `importar` ou `manter`.

## Pasta do Astrograph que já existe

Se `~/Astrograph` (ou `%LOCALAPPDATA%\Astrograph`) já tem arquivos e não é um clone limpo (arquivos
modificados, histórico próprio ou nem é um repositório git), o instalador pergunta:

| Opção | O que faz |
|---|---|
| **1. Apagar e substituir** | a pasta atual vai para `<pasta>.backup-<data>` e o Astrograph é clonado de novo |
| **2. Usar os existentes** | não mexe em nada (e não atualiza) |

Sem terminal interativo vale a opção 2 (nada é apagado sem você pedir); para escolher,
defina `ASTROGRAPH_CLONE_EXISTENTE=substituir` ou `usar`. Um clone limpo só é atualizado (`git pull`).

`:Astrograph atualizar` segue a `main`: traz o código novo e recompila o app em Rust (motor nativo
incluído) quando ele mudou. Sem `cargo` instalado ele baixa o `.exe` (ou o binário do Linux/macOS)
que o CI publica a cada push na `main` (release móvel `main-latest`), conferindo o SHA256; se o CI ainda
estiver compilando esse commit, avisa para tentar de novo em alguns minutos. Os instaladores também usam
`main-latest` enquanto não houver uma release com tag `v*`.

{: .note }
Um `init.lua` que monta o próprio lazy.nvim (estilo kickstart) não é importado:
ele fica no backup para você copiar o que quiser.

## Compilação do app (Rust)

O instalador baixa o app, o agente e os serviços do release mais novo
(Windows x64, Linux x64 e macOS arm64) e **confere o SHA256** de cada um contra o
`SHA256SUMS.txt` do release; sem release, plataforma publicada ou hash válido, ele prepara a máquina e compila: instala o **Visual Studio Build
Tools** (linker do Visual C++; ~2 GB, o Windows pede permissão de
administrador) e o **Rust** pela rustup. A toolchain GNU (MinGW) não é usada
porque o linker dela falha quando o caminho do perfil tem acento
(`C:\Users\João`). No Linux/macOS o instalador instala o Rust (sem root) e
compila. `ASTROGRAPH_SEM_BAIXAR=1` (Linux/macOS) força compilar. Se a compilação falhar, ele mostra o fim
do log. `ASTROGRAPH_SEM_COMPILAR=1` pula essa etapa.

## Só o app nativo, sem Neovim

`ASTROGRAPH_SEM_NVIM=1` instala apenas o app em Rust com o motor nativo: não
instala Neovim, ripgrep, fd nem plugins, não toca na config do Neovim e liga
`[engine] native = true` no `astrograph-ui.toml` ao lado do executável. O caminho
de volta é `ASTROGRAPH_ENGINE=nvim astrograph` (precisa de um Neovim 0.11+).

```bash
ASTROGRAPH_SEM_NVIM=1 bash -c "$(curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh)"
```

```powershell
$env:ASTROGRAPH_SEM_NVIM = '1'; irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
```

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
4. Se você tinha uma config antes, ela está em `nvim.backup-<data>` ao lado (e os
   dados em `nvim-data.backup-<data>`, se escolheu limpar).
