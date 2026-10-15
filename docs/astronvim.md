---
title: Astrograph e AstroNvim
nav_order: 9
---

# Astrograph e AstroNvim
{: .no_toc }

O que vem do AstroNvim, por que continua sendo a base e o que o Astrograph faz
diferente.

1. TOC
{:toc}

## O que vem do AstroNvim

| Parte | Onde fica | Situação no Astrograph |
| --- | --- | --- |
| **Core** (specs de ~37 plugins, atalhos, autocmds, opções) | `core/` (~3.800 linhas) | Fork da v6.1.0, com mudanças pequenas e marcadas |
| **astrocore / astroui / astrolsp** (motores de atalhos, UI e LSP) | plugins baixados pelo lazy.nvim | Usados como estão |
| **astrocommunity** | plugin | Só o pacote de Lua |
| **template** (estrutura da config) | `template/` | Reescrito: módulos `astrograph.*`, agentes, visual, comandos |
| **astrotheme** | plugin | Substituído pelo tema `astrograph` (fica como reserva) |

## Por que manter o AstroNvim como base

- **Os motores são bons e bem mantidos.** O astrocore/astrolsp resolvem bem
  atalhos, LSP, formatação e detecção de raiz do projeto. Reescrever isso
  traria o mesmo resultado e passaria a manutenção inteira para cá.
- **Atualizações continuam chegando.** Os plugins do ecossistema (astrocore,
  astroui, astrolsp) e os ~46 plugins de terceiros seguem as versões do
  AstroNvim; o fork do core é pequeno o bastante para acompanhar o upstream.
- **O gargalo não estava na arquitetura.** Medindo, o que pesava eram detalhes:
  processos externos no caminho crítico, checagens de binário no Windows e
  plugins carregados cedo demais. Isso se corrige sem trocar de base.

Alternativas consideradas: montar tudo só com o snacks.nvim (mais leve, mas
perde o motor de LSP/atalhos), LazyVim (outro framework com o mesmo tipo de
manutenção) ou kickstart.nvim (mínimo demais para um editor pronto para uso).

## O que o Astrograph faz melhor

Medido no Windows 11 (veja [Performance](performance)).

| Ponto | AstroNvim | Astrograph |
| --- | --- | --- |
| Abrir o editor | ~392 ms | **~258 ms** |
| 1ª busca de arquivo / explorador | sem pré-aquecimento | pré-carregados no ocioso; busca abre antes dos resultados |
| Primeiro arquivo (caminho crítico) | ~120 ms, tudo de uma vez | **~59 ms**; o resto vem em seguida, sem travar |
| Saber se o arquivo está num repositório git | `cmd.exe /C git rev-parse` em todo arquivo (~39 ms) | procura `.git` em Lua (~0,2 ms) |
| Checar programas externos (lazygit, node…) | no boot (~20 ms cada ausente, no Windows) | depois da animação de entrada, com cache |
| Tema do lazygit | regravado em todo startup, mesmo sem lazygit | só se o lazygit existir |
| none-ls, TODOs, cores, breadcrumbs, autotag | carregados juntos ao abrir o arquivo | estágio adiado, depois de o arquivo aparecer |
| Agentes de IA | não vem com agentes | chat, terminal e edição inline; login automatizado no instalador e no `:Astrograph configurar` |
| Animações | guias de indentação estáticas; sem cursor animado | scroll, guias e janelas animados a 60 fps; cursor animado; tela inicial animada |
| Interface | em inglês | em português (atalhos, tela inicial, diagnóstico, chat) |
| Linha de comando | clássica, com "Press ENTER" | ui2 do Neovim 0.12: destaque ao digitar, sem interrupções |

## Plugins substituídos por código próprio

| Plugin do AstroNvim | Substituto | Por quê |
| --- | --- | --- |
| nvim-highlight-colors | `astronvim.decor` | 2,6 ms por evento de rolagem; o nosso usa o provedor de decoração nativo (zero autocmds ao rolar) |
| todo-comments.nvim | `astronvim.decor` | mesmo tipo de trabalho; juntos num motor só, com cache por linha |
| provedor de área de transferência do Neovim | `astrograph.clipboard` | executava um programa a cada cópia (~11 ms por `dd`); agora 0,8 ms |
| polling de largura do neo-tree | evento `WinResized` | timer de 50 ms rodando sempre; cortava os nomes no deslize |

O critério foi medir antes: plugins que pesavam pouco (autopairs, which-key,
heirline) ficaram, porque reescrever não traria ganho que você sentisse.

## O que ficou como está, de propósito

- **neo-tree** e **toggleterm**: só carregam quando usados (custo zero no boot).
- **matchit**: é o que faz o `%` pular entre `if`/`end` e tags HTML.
- **mason-tool-installer**: tentei adiar a checagem dele e ficou pior
  (128 ms de uma vez, em vez de 21 ms no boot). Voltou para o original.
- **astrocore no init (~53 ms)**: o custo é espalhado (atalhos, autocmds,
  mason no PATH). Não há um ponto único que valha um fork do plugin.

## Acompanhar o AstroNvim

Toda mudança no `core/` tem um comentário `Astrograph:` explicando o motivo:

```bash
grep -rn "Astrograph:" core/lua
```

Para trazer uma versão nova do AstroNvim: copie o `lua/` da versão nova para um
branch, reaplique os trechos marcados (são poucos arquivos) e compare as
medições com `nvim --startuptime`.
