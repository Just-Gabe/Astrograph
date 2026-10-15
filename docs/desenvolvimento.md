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
python tools/screenshot.py --out docs/img/dashboard.png --step "wait:3000"
python tools/screenshot.py --out docs/img/atalhos.png \
  --file template/lua/polish.lua --step "wait:3000" --step "keys:<Space>" --step "wait:1500"
```

Passos aceitos: `wait:<ms>`, `keys:<teclas>`, `cmd:<comando Ex>`, `lua:<código>`
e `rec:<ms>`, que grava a tela e gera um **GIF animado**:

```bash
python tools/screenshot.py --out docs/img/abertura.gif --scale 0.6 --step "rec:1600"
```
Precisa da JetBrainsMono Nerd Font instalada.

## Licença

GPL-3.0, como o AstroNvim. Veja o [`LICENSE`](https://github.com/Just-Gabe/Astrograph/blob/main/LICENSE).
