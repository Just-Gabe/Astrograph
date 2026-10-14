# Design — Tema `astrograph` (sub-projeto 1 do rebrand)

- **Data:** 2026-10-14
- **Status:** pronto para revisão
- **Escopo:** primeiro dos 4 sub-projetos do rebrand Astrograph. Entrega um colorscheme
  próprio (`astrograph` / `astrograph-light`) com a identidade do dashboard, reaproveitando
  o motor do `astrotheme` (AstroNvim), e uma fonte única de cores (`palette.json`) para os
  demais sub-projetos (editor, instalador, repo).
- **Fora de escopo:** dashboard, telas do instalador, UI do repo. Ficam para os sub-projetos 2–4.

## Decisões (aprovadas no brainstorm)

1. Escopo do rebrand = **tudo** (editor + instalador/programa + repo).
2. Identidade = **a do dashboard**: lua crescente + estrelas + gradiente `#7DD3FC → #F472B6`.
3. Colorscheme = **tema próprio `astrograph`** (não apenas manter o Astro + trocar acentos).
4. Arquitetura = **plugin próprio em `theme/`** no monorepo, com `palette.json` **gerado**
   como fonte única consumida por instalador e repo.
5. Base = **manter a base do `astrodark`** (fundos/cinzas); trocar os matizes de sintaxe/UI.
6. Distribuição de cor = **opção "azul manda"**: palavras-chave/tipos em azul; rosa/ouro
   pontuam. Sintaxe harmonizada mas com **contraste seguro** (sem "sumir").
7. **Tons das estrelas** entram na sintaxe (ver §2): ouro claro `#F9E2AF`, ouro profundo
   `#E8C982`, cinza-estrela `#6B6B8A`.
8. Texto neutro = **lavanda** `#D8DCF0` (variáveis/identificadores) para máximo conforto de leitura.
9. Duas variantes: **dark (default)** e **light**.

## §1 — Arquitetura e integração

Plugin local, descoberto junto do `core/` (mesma mecânica de `../core` → `../theme`).

```
theme/
├── lua/
│   ├── astrograph/
│   │   ├── init.lua                 # setup()/load() → delega ao motor do astrotheme
│   │   └── palettes/
│   │       ├── astrograph.lua       # PALETA (dark)  — fonte de verdade em Lua
│   │       └── astrograph-light.lua # PALETA (light)
│   └── lualine/themes/
│       └── astrograph.lua (+ astrograph-light.lua)  # derivado da paleta
├── colors/
│   ├── astrograph.lua               # `:colorscheme astrograph`
│   └── astrograph-light.lua
├── palette.json                     # GERADO (installer/repo)
└── tools/
    ├── gen_palette.lua              # paleta Lua → palette.json
    └── check_contrast.lua           # valida ≥4.5:1
```

**Integração com o `astrotheme`** (o motor já resolve paletas via
`require("astrotheme.palettes.<nome>")`):

- Registrar a paleta no `package.preload` para que o `astrotheme` a enxergue:
  `package.preload["astrotheme.palettes.astrograph"]` / `...astrograph-light`.
- `colors/astrograph.lua` faz:
  ```lua
  vim.o.background = "dark"
  require("astrotheme").setup {
    palette = "astrograph",
    background = { dark = "astrograph", light = "astrograph-light" },
  }
  require("astrotheme").load()  -- sem arg: usa config.background (evita lookup em style_background)
  ```
  (`astrograph-light.lua` idem com `vim.o.background = "light"`.)
- `template/lua/plugins/astroui.lua`: hoje inerte (`if true then return {} end`) → reativar
  com `colorscheme = "astrograph"`.
- `template/lazy_setup.lua`: adicionar o diretório `../theme` à descoberta (espelhando `../core`).
- **Fallback:** se `theme/` não existir, mantém `astrodark` (nada quebra em clones antigos).

## §2 — Paleta

Base dark `#1A1D23`; base light `#F7F8F8`. Regra: **todo fg de sintaxe ≥ 4.5:1**;
**comment é exceção** (de-emphasized, alvo ≥ 3:1).

### Dark (`astrograph`)

| Papel | Hex | Contraste |
|---|---|---|
| Palavra-chave / tipo (`Statement`, `Type`) | `#8AB4FF` | 8.08 |
| Função / `Special` | `#7DD3FC` | 10.13 |
| String / Character | `#7CE38B` | 10.63 |
| Número / Boolean / Float / Constant | `#F9E2AF` | 13.28 |
| Macro / PreProc / Structure (ouro profundo) | `#E8C982` | 10.54 |
| Variável / identificador (lavanda) | `#D8DCF0` | 12.39 |
| Operador / pontuação | `#BFC5D6` | 9.79 |
| Parâmetro / label (lua) | `#C7D2FE` | 11.32 |
| Comentário (cinza-estrela) | `#6B6B8A` | 3.29 *(exceção)* |

> Números de contraste recalculados no `check_contrast.lua`; a tabela acima é a referência.

### Light (`astrograph-light`)

| Papel | Hex | Contraste |
|---|---|---|
| Palavra-chave / tipo | `#2E5FBF` | 5.63 |
| Função / `Special` | `#0E6E8C` | 5.44 |
| String / Character | `#2E7D3F` | 4.79 |
| Número / Boolean / Float / Constant | `#8A6000` | 5.26 |
| Macro / PreProc / Structure | `#8A6A1F` | 4.74 |
| Variável / identificador | `#3A3F4B` | 9.91 |
| Operador / pontuação | `#5A6070` | 5.91 |
| Parâmetro / label | `#5B5FC7` | 5.05 |
| Comentário | `#6F7488` | 4.36 *(exceção)* |

## §3 — UI e lualine

Acentos de UI derivam da paleta (não fixos):

- `ui.accent` = `#7DD3FC` (dark) / `#2E5FBF` (light) — "azul manda".
- `StatusNormal` = fundo accent, fg `#111317`; `StatusInsert` verde, `StatusReplace` `#FF7E9D`,
  `StatusCommand` `#F9E2AF`.
- `Visual` = `#F472B6` (dark) / `#C24E8A` (light); `search`/`incsearch` em rosa.
- `CursorLineNr` = ouro `#F9E2AF` (dark) / `#8A6000` (light) — a "estrelinha" marcando o cursor.
- `selection` = `#243447` (dark) / `#E7E9EB` (light); `current_line` discreto na base.

**lualine** (`theme/lua/lualine/themes/astrograph.lua`): tema custom derivado 1:1 da paleta —
seções `a` (accent cheio), `b` (superfície), `c` (base); por modo: normal=accent, insert=verde,
visual=rosa, replace=`#FF7E9D`, command=ouro, inactive=cinza. Sem cores hardcoded fora da paleta.

## §4 — Geração, validação e fallback

- **`palette.json` (gerado):** `tools/gen_palette.lua` roda headless
  (`nvim --headless -l theme/tools/gen_palette.lua`) e serializa as paletas Lua (dark+light)
  para `theme/palette.json` — incluindo `base`, tokens de sintaxe, acentos UI e o gradiente
  `["#7DD3FC","#F472B6"]`. Consumidores (sub-projetos 2–4) leem só o JSON.
- **`check_contrast.lua`**: lê a paleta e falha se qualquer fg de sintaxe (exceto comment)
  ficar < 4.5:1 sobre sua base. Comment alvo ≥3:1. Rodado após gerar.
- **Fallback:** paleta ausente → `astrodark` (clones antigos seguem funcionando).

## Fluxo de desenvolvimento

1. Criar `theme/` + paletas Lua (dark/light).
- 2. `colors/*.lua` + `init.lua` (preload + setup + load).
3. Reativar `astroui.lua` e ajustar `lazy_setup.lua`.
4. `lualine/themes/astrograph.lua`.
5. `tools/gen_palette.lua` → `palette.json`; `tools/check_contrast.lua` verde.
6. Teste: `:colorscheme astrograph` / `astrograph-light`; `vim.o.background` troca limpo;
   fallback sem `theme/` usa `astrodark`.

## Critérios de aceite

- [x] `:colorscheme astrograph` e `astrograph-light` carregam sem erro.
- [x] `check_contrast.lua` passa (comment como exceção documentada).
- [x] `palette.json` gerado a partir da paleta Lua.
- [x] Sem `theme/` → cai em `astrodark`, sem erro.
- [x] Lualine reflete os modos em dark e light.
- [x] Nenhuma cor hardcoded fora dos módulos de paleta (extras de `syntax` na paleta).

## Implementação

Arquivos criados:

- `theme/lua/astrograph/lib/color.lua` — helper de cor auto-contido (lighten/darken/blend).
- `theme/lua/astrograph/palettes/{astrograph,astrograph-light}.lua` — paletas (fonte única).
- `theme/lua/astrograph/init.lua` — `register()` (package.preload) + `setup()` (astrotheme).
- `theme/colors/{astrograph,astrograph-light}.lua` — entradas `:colorscheme`.
- `theme/lua/lualine/themes/{astrograph,astrograph-light}.lua` — temas lualine.
- `theme/tools/gen_palette.lua` — gera `theme/palette.json`.
- `theme/tools/check_contrast.lua` — valida ≥4.5:1 (comment ≥3:1).
- `theme/palette.json` — artefato gerado (consumido por instalador/repo).

Alterados:

- `template/lua/lazy_setup.lua` — descobre `../theme` (ou `$ASTROGRAPH_THEME`) e o registra
  como spec com `priority = 1000`, expondo `vim.g.astrograph_theme` (fallback).
- `template/lua/plugins/astroui.lua` — reativado; `colorscheme = "astrograph"` com fallback
  para `astrodark`.

Comandos:

- Gerar JSON: `nvim --headless -u NONE -l tools/gen_palette.lua` (dentro de `theme/`).
- Validar: `nvim --headless -u NONE -l tools/check_contrast.lua`.

Notas de integração com o motor (`astrotheme`):

- As paletas são injetadas via `package.preload["astrotheme.palettes.<nome>"]`.
- `config.palettes.<nome>` precisa existir (mesmo vazio) — o motor faz `tbl_deep_extend`.
- `Operator`/`Identifier` compartilham `syntax.text`; `Macro`/`Number` compartilham
  `syntax.orange`; por isso `operator`, `macro`, `cursorline` e `parameter` são slots extras
  da paleta, aplicados como **overrides de highlight** em `astrograph/init.lua`.
- Chamar `astrotheme.load()` sem argumento usa `config.background[vim.o.background]`,
  evitando o lookup em `style_background` (que não conhece `astrograph`).
