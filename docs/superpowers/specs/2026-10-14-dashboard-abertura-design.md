# Design — Dashboard de abertura animado (lua + estrelas + ASTROGRAPH)

- **Data:** 2026-10-14
- **Status:** aprovado em revisão pelo usuário (escolhas via mockups ao vivo)
- **Repo:** `Just-Gabe/Astrograph` (config viva: `%LOCALAPPDATA%\nvim` → `template/`)

## Contexto

Abrir `nvim` hoje mostra o dashboard do AstroNvim com o banner `ASTRO/NVIM` estático.
Este design define um **dashboard próprio do Astrograph**: cena com lua crescente à
esquerda + estrelas + banner `ASTROGRAPH` com animação de entrada, substituindo apenas
o cabeçalho do dashboard (o resto — atalhos, startup, `<Leader>h` — permanece igual).

## Decisões do usuário (validadas em mockup)

| Tema | Escolha |
|---|---|
| Direção visual | **A** — banner `ANSI Shadow`, gradiente azul→rosa, lua grande à esquerda |
| Composição | **V1** — lua grande (~11 linhas) à esquerda, centralizada com o texto, 2 col de vão; texto começa logo em seguida (~113 col) |
| Movimento | **M1** — lua em *fade* + banner **datilografado** (revelação linha a linha, esquerda→direita) + estrelas piscando |
| Velocidade | **A** — rápida, ~0,4 s no total |
| Estrelas depois da intro | **A** — continuam piscando (brilho sutil) enquanto o dashboard estiver aberto |

Mockups usados: `C:\Users\gabri\Astrograph\.superpowers\brainstorm\1454-1791987883\content\*.html`.

## Alternativas consideradas

1. **Seção `Gen` + `Snacks.animate` + `uv.timer`** — *escolhida*: usa a lib de animação
   já ligada nas outras animações do Astrograph, honra `vim.g.snacks_animate` sem código
   extra, sem plugin adicional.
2. Seção `Gen` só com `uv.timer` — easing manual, mais código, toggle checado na mão.
3. Só estrelas + banner estático — mais barato, mas perde a datilografia (descartada).

## Arquivos

| Arquivo | Papel |
|---|---|
| `template/lua/plugins/dashboard.lua` | Nova spec do `folke/snacks.nvim`: `opts` como função que recebe `opts.dashboard` do AstroNvim e **substitui só `sections[1]`** por uma função `Gen` nossa. `preset.keys`, `startup` e demais campos intactos. |
| `template/lua/astrograph/art.lua` | Arte "assada" como strings Lua: banner `ASTROGRAPH` em 4 larguras (`ansi_shadow` 83 col, `standard` 65, `slant` 64, `mini` 31) + lua crescente em 2 tamanhos (28 e 24 col). Sem figlet em runtime. |
| `tools/gen_art.py` | Gerador (Python + `pyfiglet`) que (re)escreve `art.lua`. Só quem mexe na arte roda. |

Nada do `core/` é modificado.

## Cena

- **Canvas:** 16 linhas × 120 col.
  - Lua grande em `x=0, y=2` (11 linhas); banner em `x=30, y=5` (6 linhas) — linha 7,5 do
    conjunto = centro da lua e do banner.
  - Estrelas (`✦ ✧ · ✶`): sorteadas só em células vazias com **seed fixa** (mesma cena a
    cada abertura; nada de "pulo").
- **Cores via highlights próprios** (`Astrograph*`), independentes do tema:
  - `AstrographGrad1..6` — gradiente por linha do banner (`#7dd3fc → #f472b6`);
  - `AstrographMoon` — `#c7d2fe` (estado final);
  - `AstrographStar1..3` — fases de brilho (amarelo `#f9e2af` ↔ apagado `#6b6b8a`).
- Cada linha do banner é um chunk `snacks.dashboard.Text` com seu `hl`.
- **Centralização:** o item `Gen` usa `align = "center"` do Snacks (fallback: padding
  manual pela largura do canvas).
- **Fallback progressivo por `vim.o.columns`** (testado na hora):
  1. `< 115` col → banner `ansi_shadow` sem lua, centrado;
  2. `< 90` col → banner `standard` sem lua;
  3. `< 75` col → lua pequena (24) + banner `mini` (31) lado a lado;
  4. `< 60` col → só banner `mini` centrado.

## Animação

- **Intro (uma vez por abertura, ~0,4 s):** um `Snacks.animate(0→1, 400 ms, easing outCubic)`
  alimenta:
  - **Fade da lua (0–150 ms):** interpola o `fg` de `AstrographMoon` de `#3a3a5c` → `#c7d2fe`
    via `nvim_set_hl` a cada tick;
  - **Datilografia (0–400 ms):** a linha `i` do banner começa em `i*50 ms` e leva `150 ms`
    para revelar da esquerda para a direita (revelação por coluna; o texto do quadro é
    reconstruído só quando o progresso avança).
- **Twinkle contínuo:** `uv.timer` a 8 Hz enquanto o dashboard existir — seno por grupo de
  estrela com fase deslocada, alterando só 3 `nvim_set_hl` por tick.

## Ciclo de vida

- Timers/autocmd criados na **primeira chamada da seção `Gen`**; parados em `BufWipeout`
  do buffer do dashboard (abrir arquivo ou sair) — sem vazamento e **sem segurar**
  `nvim --headless` dos instaladores (`+Lazy! restore` etc.).
- Reabrir com `<Leader>h` → replay da intro.
- `nvim arquivo` → a seção não roda → custo zero.

## Guardas

- `vim.g.snacks_animate = false` → quadro final estático (intro e twinkle desligados;
  toggle ao vivo durante o dashboard também desliga).
- Falha em qualquer etapa (pcall) → cai para o header padrão do AstroNvim, nunca deixa a
  abertura quebrada.

## Verificação

1. `luafile`/`loadfile` sem erro em `dashboard.lua` e `art.lua`.
2. Boot headless: `type(sections[1]) == "function"` e `:messages` sem erro.
3. Teste visual manual: usuário roda `nvim`, confere cena/animação, reporta (ou print).
4. Cenários: `nvim`, `nvim arquivo.txt`, `<Leader>h`, toggles `<Leader>ua`/`uC`
   desligando animações, janela estreita (~80 col), `nvim --headless +qa` saindo limpo.

## Fora de escopo

- Alterar o `core/` do AstroNvim.
- Música/som, splash de tela cheia separada do dashboard, animações em outros menus.
- Instaladores/README (já entregues) — salvo menção na lista de mudanças ao final.
