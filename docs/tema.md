---
title: Tema e animações
nav_order: 6
---

# Tema e animações
{: .no_toc }

1. TOC
{:toc}

## Tema astrograph

Dois colorschemes próprios, em `theme/`:

- `astrograph`: escuro (padrão);
- `astrograph-light`: claro.

```vim
:colorscheme astrograph-light
```

`<Leader>ub` alterna fundo claro/escuro e `<Leader>ft` abre a busca de temas com
pré-visualização.

![Tema claro]({{ '/img/tema-claro.png' | relative_url }})

As paletas ficam em `theme/lua/astrograph/palettes/`. Ao mudar uma cor, rode as
ferramentas a partir da raiz do repositório:

```bash
nvim --headless -u NONE -l theme/tools/gen_palette.lua     # regenera theme/palette.json
nvim --headless -u NONE -l theme/tools/check_contrast.lua  # confere o contraste mínimo (WCAG)
```

Para trocar o tema padrão, edite `colorscheme` em `template/lua/plugins/astroui.lua`.

## Tela inicial animada

A tela inicial mostra uma lua crescente, estrelas e o banner ASTROGRAPH com
gradiente. Ao abrir, a lua aparece em fade e o banner é revelado linha a linha;
depois as estrelas piscam devagar.

- O tamanho se adapta à largura do terminal (banner grande, médio, inclinado ou
  mínimo).
- A entrada roda a 60 fps e é baseada em tempo real: se o editor ficar ocupado
  por um instante, ela pula quadros em vez de pausar. Ela só começa depois que
  os plugins do startup terminam de carregar.
- As estrelas pausam quando o terminal perde o foco ou a tela inicial não está
  visível, sem gastar CPU em segundo plano.
- A arte fica em `template/lua/astrograph/art.lua`, gerada por
  `python tools/gen_art.py`.

## Animações da interface

Feitas pelo Snacks, no estilo "Claude/opencode": rápidas, suaves e sem
"quique".

| O quê | Como |
| --- | --- |
| Base | 60 fps, `outQuad`, no máximo 160 ms |
| Scroll suave | `outCubic`, 140 ms; segurando a tecla fica mais curto (60 ms) |
| Guias de indentação | animadas ao mover o cursor |
| Janelas flutuantes | fundo levemente escurecido (notificações, Lazy, busca) |

| Atalho | O que faz |
| --- | --- |
| `<Leader>um` | Liga/desliga **todas** as animações |
| `<Leader>uM` | Liga/desliga só o scroll suave |

{: .note }
Via SSH as animações começam desligadas (cada quadro vira tráfego de rede).
`<Leader>um` religa. Para desligar sempre, ponha `vim.g.snacks_animate = false`
no `template/lua/polish.lua`.
