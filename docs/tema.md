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

O escuro segue o modo escuro do Xcode, pensado para um dia inteiro de leitura
sem cansar:

- **fundo grafite neutro** (`#1F1F24`), sem o azul-neon de antes;
- **texto quase branco**, mas não puro (`#DCDCE0`);
- **sintaxe calma**: palavras-chave rosa, strings salmão, funções e tipos azul
  suave, números em dourado claro; sem itálico nem negrito chamativos;
- **um destaque só**: o azul de sistema da Apple (`#409CFF`);
- **superfícies em vez de linhas**: barras e laterais um tom acima do editor,
  janelas flutuantes mais acima ainda (como popovers), números de linha e guias
  quase sumindo.

Todas as cores de sintaxe passam no contraste mínimo (WCAG 4,5:1; comentários
3:1).

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

## Tela inicial

![Abertura animada]({{ '/img/abertura.gif' | relative_url }})

No espírito das telas de boas-vindas da Apple: uma estrela (o ícone), o nome em
traço fino, uma linha discreta dizendo o que é e, abaixo, as ações com o atalho
alinhado à direita em cinza, como num menu do macOS. Muito espaço vazio e uma
cor de destaque só.

- **Entrada**: a estrela acende, o nome e as ações surgem com um fade curto
  (~0,6 s), uma vez só. Depois a tela fica parada: nada piscando, nada gastando
  CPU.
- **Cores do tema**: tudo vem do tema ativo, então funciona no escuro e no
  claro.
- **Seis ações**: novo arquivo, buscar arquivo, recentes, buscar palavra,
  agentes de IA e última sessão. Os marcadores continuam em `<Leader>f'`.
- O relógio da entrada é de tempo real e só começa depois que os plugins do
  startup carregam: se o editor engasgar, o fade pula para o ponto certo.
- Código em `template/lua/astrograph/dashboard.lua`; as letras vêm de
  `template/lua/astrograph/art.lua` (`python tools/gen_art.py`).

## Barra de status

Sem os blocos coloridos de modo nas pontas: o modo é um **ponto colorido** e,
fora do Normal, o nome em português ("Inserir", "Visual", "Comando"…) em cinza.
A posição aparece como `12:4`, sem porcentagem. Enquanto um agente trabalha,
aparece `✦ Claude Code ⋯`. Nas abas, o `×` é cinza: vermelho fica para erro.

## Visual moderno no terminal

| Recurso | O que muda |
| --- | --- |
| **Linha de comando (ui2)** | Destaque de sintaxe enquanto você digita e mensagens sem o "Press ENTER". Nativo do Neovim 0.12. |
| **Cursor animado** | Um rastro curto e suave ao pular pelo texto (smear-cursor). Na UI nativa o cursor desliza sem rastro, desenhado pela própria UI. |
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
| **Apagar linha** (`dd`) | apaga na hora; a linha colapsa dentro dela mesma e some |

![Explorador deslizando]({{ '/img/explorador.gif' | relative_url }})

![Linha apagada colapsando]({{ '/img/apagar-linha.gif' | relative_url }})

No `dd`, o texto sai do arquivo **na hora**; o que anima é uma cópia da linha
no mesmo lugar, que achata em direção à base (`▄` → `▂` → `▁`) mantendo as
cores da sintaxe e some, em ~160 ms.
Contagem (`3dd`), registradores (`"add`), `.` e `u` funcionam como sempre.
Blocos com mais de 30 linhas e dobras fechadas são apagados sem animação.

No explorador, a árvore é desenhada uma vez já na largura final e a janela só
revela o que está pronto: os nomes não são cortados ("rea…") durante o
deslize. Apertar `<Leader>e` no meio da animação a interrompe em vez de
empilhar outra.

Todas usam um relógio de tempo real: se o editor engasgar, a animação pula
quadros em vez de pausar. Ficam em `template/lua/astrograph/motion.lua`.

## Animações do Snacks

Feitas pelo Snacks, no estilo "Claude/opencode": rápidas, suaves e sem
"quique".

| O quê | Como |
| --- | --- |
| Base | 60 fps no terminal, 120 fps na UI nativa; `outQuad`, no máximo 160 ms |
| Scroll suave | `outCubic`, 140 ms; segurando a tecla fica mais curto (60 ms). Na UI nativa a rolagem é por pixel e feita pela UI |
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
