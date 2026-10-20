---
title: Como funciona
nav_order: 8
---

# Como funciona
{: .no_toc }

1. TOC
{:toc}

## Estrutura do repositório

```
Astrograph/
├── core/        fork do core do AstroNvim (base v6.1.0), com as otimizações
├── template/    a config que o Neovim abre (ligada a ~/.config/nvim)
├── theme/       colorschemes astrograph e astrograph-light
├── tools/       geradores (arte da tela inicial, prints da documentação)
├── docs/        este site (GitHub Pages) e os prints em docs/img
├── install.ps1  instalador Windows
└── install.sh   instalador Linux/macOS
```

## Como o Neovim encontra tudo

1. `template/init.lua` liga o `vim.loader`, desliga providers sem uso e instala o
   lazy.nvim se faltar.
2. `template/lua/lazy_setup.lua` procura o **core local** (`$ASTROGRAPH_CORE`,
   depois `../core` ao lado do template, depois `~/Astrograph/core`) e o **tema
   local** (`$ASTROGRAPH_THEME`, `../theme`, `~/Astrograph/theme`). Sem eles, cai
   para o AstroNvim oficial do GitHub e para o tema astrodark.
3. O lazy.nvim importa os plugins do core (`astronvim.plugins`), depois
   `template/lua/community.lua` e por fim `template/lua/plugins/`.
4. `template/lua/polish.lua` roda por último: recarregamento dos arquivos
   editados por agentes e o comando `:Astrograph`.

O caminho real da config é resolvido mesmo através de junction/symlink, então
`../core` funciona com a config ligada pelo instalador.

## Módulos próprios (`template/lua/astrograph/`)

| Módulo | Função |
| --- | --- |
| `agents.lua` | Detecta CLIs de IA, Ollama e modelo local (com cache) |
| `commands.lua` | `:Astrograph atualizar / saude / agentes / atalhos / docs` |
| `health.lua` | `:checkhealth astrograph` |
| `prepare.lua` | Preparo em segundo plano por projeto: parsers, Mason, git, build em Rust, ícone de carregamento |
| `warmup.lua` | Pré-aquecimento no ocioso (busca, explorador, `fd`/`rg`/`git`) |
| `motion.lua` | Fade das janelas, explorador deslizante e `dd` animado |
| `clipboard.lua` | Área de transferência sem espera (cópia agrupada, colar da memória) |
| `dashboard.lua` | Cena animada da tela inicial e rodapé em português |
| `plugins/visual.lua` | ui2, bordas arredondadas, ghost text e cursor animado |
| `art.lua` | Arte gerada por `tools/gen_art.py` (não edite à mão) |

## Diferenças do core em relação ao AstroNvim

O `core/` é um fork do AstroNvim v6.1.0. As mudanças são pequenas e marcadas com
comentários `Astrograph:` no código:

- `astronvim.decor` + `plugins/decor.lua`: cores e TODOs próprios, no lugar do
  nvim-highlight-colors e do todo-comments;
- `astronvim.executable()`: cache de `vim.fn.executable`;
- `plugins/_astrocore_autocmds.lua`: arquivo em estágios (`AstroFile` →
  `AstroFileIdle`) e detecção do git sem processo externo;
- `plugins/toggleterm.lua`: atalhos de ferramentas externas registrados após o startup;
- `plugins/_astroui.lua` e `_astrocore_autocmds.lua`: dobras e EditorConfig ignoram
  janelas flutuantes e buffers internos (abrir a busca ficou mais leve);
- `astronvim/lazygit.lua` + `plugins/_astroui.lua`: tema do lazygit só quando ele existe;
- descrições dos atalhos, tela inicial e mensagens em português.

A análise completa está em [Astrograph e AstroNvim](astronvim).

## Desenvolver com o checkout local

Aponte o Neovim para um clone de desenvolvimento sem mexer na sua config:

```powershell
# Windows: um perfil separado (NVIM_APPNAME) ligado ao template do clone
cmd /c mklink /J "$env:LOCALAPPDATA\astrograph-dev" "C:\caminho\Astrograph\template"
$env:NVIM_APPNAME = "astrograph-dev"; nvim
```

```bash
ln -s ~/code/Astrograph/template ~/.config/astrograph-dev
NVIM_APPNAME=astrograph-dev nvim
```

Ou, para testar só um core diferente: `ASTROGRAPH_CORE=/caminho/core nvim`.

## Regenerar os prints da documentação

Os prints em `docs/img/` saem direto da grade de UI do Neovim, sem captura de
tela do sistema. Assim ficam nítidos e iguais em qualquer máquina:

```bash
pip install msgpack pillow fonttools

# Tela inicial (banner) e a animação de abertura
python tools/screenshot.py --out docs/img/dashboard.png --step "wait:10000"
python tools/screenshot.py --out docs/img/abertura.gif --scale 0.6 --step "rec:1600"

# Editor com o explorador aberto
python tools/screenshot.py --out docs/img/editor.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "keys:<Space>e" --step "wait:1200"

# Menus which-key: raiz (<Space>) e o prefixo de IA (<Space>A)
python tools/screenshot.py --out docs/img/atalhos.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "keys:<Space>" --step "wait:1500"
python tools/screenshot.py --out docs/img/agentes.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "keys:<Space>A" --step "wait:1500"

# Busca de arquivos, saúde e tema claro
python tools/screenshot.py --out docs/img/busca.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "keys:<Space>ff" --step "wait:1500"
python tools/screenshot.py --out docs/img/saude.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "cmd:checkhealth astrograph" --step "wait:1500"
python tools/screenshot.py --out docs/img/tema-claro.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "cmd:colorscheme astrograph-light" --step "wait:800"

# Linha de comando (deixada aberta), completação do LSP e chat do CodeCompanion
python tools/screenshot.py --out docs/img/linha-de-comando.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "keys::Astrograph " --step "wait:800"
python tools/screenshot.py --out docs/img/completar.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "keys:ovim.api." --step "wait:1200" --step "keys:<C-Space>" --step "wait:1000"
python tools/screenshot.py --out docs/img/chat.png \
  --file template/lua/astrograph/agents.lua --step "wait:10000" --step "cmd:CodeCompanionChat" --step "wait:2000"

# GIFs das animações: o explorador deslizando e o `dd` colapsando a linha
python tools/screenshot.py --out docs/img/explorador.gif \
  --file template/lua/astrograph/motion.lua --scale 0.5 --interval 30 \
  --step "wait:10000" \
  --step "lua:vim.defer_fn(function() require('astrograph.motion').toggle_explorer() end, 220)" \
  --step "rec:560"
python tools/screenshot.py --out docs/img/apagar-linha.gif \
  --file template/lua/astrograph/agents.lua --scale 0.5 --interval 20 \
  --step "wait:10000" --step "keys:26G" --step "wait:300" \
  --step "lua:vim.defer_fn(function() require('astrograph.motion').delete_lines() end, 350)" \
  --step "rec:900"
```

Passos aceitos: `wait:<ms>`, `keys:<teclas>`, `cmd:<comando Ex>`, `lua:<código>`
e `rec:<ms>`, que grava a tela e gera um **GIF animado**.

O primeiro passo é sempre um `wait:10000`: dá tempo de o servidor de linguagem
(`lua_ls`) terminar de carregar o workspace, para os prints não saírem com o
"Loading workspace" na barra de status.

As animações de abertura e de exclusão duram ~160 ms, e o `rec` só começa depois
do passo anterior; por isso os GIFs disparam a ação já dentro da gravação, com
`lua:vim.defer_fn(function() ... end, <atraso ms>)`.

Precisa da JetBrainsMono Nerd Font instalada.

## Licença

GPL-3.0, como o AstroNvim. Veja o [`LICENSE`](https://github.com/Just-Gabe/Astrograph/blob/main/LICENSE).
