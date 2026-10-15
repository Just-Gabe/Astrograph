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

![Abertura animada]({{ '/img/abertura.gif' | relative_url }})

A tela inicial mostra uma lua crescente, estrelas e o banner ASTROGRAPH com
gradiente. Ao abrir, a lua aparece em fade, o banner é revelado linha a linha e,
por último, a linha discreta "Neovim · agentes de IA · em português"; depois as
estrelas piscam devagar.

- O banner usa letras sólidas e limpas (ANSI Regular), sem as sombras de linha
  dupla de antes.
- O tamanho se adapta à largura do terminal: com a lua (≥ 100 colunas), só o
  traço fino (≥ 44) ou a versão mínima.
- A entrada roda a 60 fps e é baseada em tempo real: se o editor ficar ocupado
  por um instante, ela pula quadros em vez de pausar. Ela só começa depois que
  os plugins do startup terminam de carregar.
- As estrelas pausam quando o terminal perde o foco ou a tela inicial não está
  visível, sem gastar CPU em segundo plano.
- A arte fica em `template/lua/astrograph/art.lua`, gerada por
  `python tools/gen_art.py`.

## Visual moderno no terminal

| Recurso | O que muda |
| --- | --- |
| **Linha de comando (ui2)** | Destaque de sintaxe enquanto você digita e mensagens sem o "Press ENTER". Nativo do Neovim 0.12. |
| **Cursor animado** | Um rastro curto e suave ao pular pelo texto (smear-cursor). |
| **Bordas arredondadas** | Em hovers do LSP, diagnósticos, completação e documentação. |
| **Ghost text** | A primeira sugestão da completação aparece em cinza na própria linha. |
| **Linha do cursor** | Destacada só na janela ativa. |
| **Menos ruído** | Sem `~` depois do fim do arquivo; CodeLens ("N references") desligado por padrão (`<Leader>uL` liga); menu de completação levemente translúcido. |
| **Tela inicial** | Rodapé discreto: só "pronto em X ms". |

![Linha de comando com destaque]({{ '/img/linha-de-comando.png' | relative_url }})

![Completação com ghost text]({{ '/img/completar.png' | relative_url }})

{: .note }
A ui2 ainda é experimental no Neovim. Se algo estranho aparecer nas mensagens,
desligue com `vim.g.astrograph_ui2 = false` no começo do `template/init.lua`.

## Movimento da interface

| O quê | Como |
| --- | --- |
| **Janelas flutuantes** (busca, hover, diagnósticos, notificações, chat) | surgem com fade em ~140 ms |
| **Menus** (completação, atalhos) | fade mais curto, ~90 ms, para não atrasar a digitação |
| **Explorador lateral** (`<Leader>e`) | abre e fecha deslizando a largura |
| **Apagar linha** (`dd`) | apaga na hora; a linha apagada desce e some |

![Explorador deslizando]({{ '/img/explorador.gif' | relative_url }})

![Linha apagada descendo e sumindo]({{ '/img/apagar-linha.gif' | relative_url }})

No `dd`, o texto sai do arquivo **na hora**; o que anima é uma cópia da linha
(com o mesmo destaque de sintaxe), que desce e some por cima do texto real.
Contagem (`3dd`), registradores (`"add`), `.` e `u` funcionam como sempre.
Blocos com mais de 30 linhas e dobras fechadas são apagados sem animação.

Todas usam um relógio de tempo real: se o editor engasgar, a animação pula
quadros em vez de pausar. Ficam em `template/lua/astrograph/motion.lua`.

## Animações do Snacks

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
| `<Leader>um` | Liga/desliga **todas** as animações (fade, explorador, `dd`, cursor animado) |
| `<Leader>uM` | Liga/desliga só o scroll suave |

{: .note }
Via SSH as animações e o cursor animado começam desligados (cada quadro vira
tráfego de rede).
`<Leader>um` religa. Para desligar sempre, ponha `vim.g.snacks_animate = false`
no `template/lua/polish.lua`.
