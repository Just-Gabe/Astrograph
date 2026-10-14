# Astrograph

**Astrograph** é a minha versão pessoal do [AstroNvim](https://github.com/AstroNvim/AstroNvim):
um monorepo que reúne o **core** do AstroNvim e o **template** (a configuração do usuário),
com otimizações e ajustes próprios.

> Não é um fork oficial no GitHub (sem o vínculo “forked from”). É um repositório novo que
> **vendoriza** o core para podermos evoluí-lo junto com a configuração.

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
- **`lua/polish.lua`** — otimização geral:
  - `vim.loader.enable()` (cache de bytecode Lua → startup mais rápido);
  - desabilita os providers Perl/Ruby não usados (evita checagem no startup);
  - mantém o `CC=gcc` no Windows (treesitter compila com WinLibs).

### `lua/lazy_setup.lua` (template)
- Passa a **preferir o core local** do monorepo (`../core`) quando existir, caindo para o
  AstroNvim oficial caso contrário. Isso permite testar mudanças no core sem publicar nada.

## Requisitos

- Neovim **0.11+**
- Uma [Nerd Font](https://www.nerdfonts.com/font-downloads) instalada e configurada no terminal
- Um compilador C no PATH (para os parsers do Treesitter)
- `git` (obrigatório); `fd` e `ripgrep` são instalados automaticamente via Mason

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
