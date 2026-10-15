---
title: Primeiros passos
nav_order: 3
---

# Primeiros passos
{: .no_toc }

1. TOC
{:toc}

## A tecla Líder

Quase tudo começa com a **tecla Líder**, que no Astrograph é o **Espaço**. Aperte
`Espaço` e espere meio segundo: aparece o menu com todos os atalhos disponíveis,
agrupados e em português. Nos atalhos abaixo, `<Leader>` = Espaço.

![Menu de atalhos]({{ '/img/atalhos.png' | relative_url }})

{: .tip }
Esqueceu um atalho? `<Leader>fk` busca entre **todos** os atalhos pelo nome, e
`:Astrograph atalhos` faz o mesmo.

## Tela inicial

Ao abrir o `nvim` sem arquivo aparece a tela inicial. Cada item tem uma tecla:

| Tecla | Ação |
| --- | --- |
| `n` | Novo arquivo |
| `f` | Buscar arquivo |
| `o` | Arquivos recentes |
| `w` | Buscar palavra (em todos os arquivos) |
| `a` | Agentes de IA (abre o chat) |
| `'` | Marcadores |
| `s` | Última sessão |

`<Leader>h` volta para a tela inicial a qualquer momento.

## O básico

| Atalho | O que faz |
| --- | --- |
| `<Leader>w` | Salvar |
| `<Leader>q` | Fechar janela |
| `<Leader>Q` | Sair do Astrograph |
| `<Leader>n` | Novo arquivo |
| `<Leader>c` | Fechar buffer |
| `<Leader>e` | Alternar explorador de arquivos |
| `<Leader>o` | Focar explorador |
| `<Leader>/` | Comentar/descomentar linha |
| `\|` / `\` | Dividir janela na vertical / horizontal |
| `Ctrl` + `h` `j` `k` `l` | Ir para a janela da esquerda/baixo/cima/direita |
| `]b` / `[b` | Próximo / anterior buffer |
| `dd` | Apagar linha (a linha colapsa e some) |
| `]T` / `[T` | Próximo / anterior TODO |

## Completação

Enquanto você digita, o menu de sugestões aparece com a documentação ao lado e
a primeira sugestão em cinza na própria linha (ghost text).

| Tecla | O que faz |
| --- | --- |
| `Tab` / `Shift-Tab` | Próxima / anterior sugestão |
| `Enter` | Confirmar |
| `Ctrl-Espaço` | Abrir o menu |
| `Ctrl-e` | Fechar o menu |

## Buscar (`<Leader>f`)

![Busca de arquivos]({{ '/img/busca.png' | relative_url }})

| Atalho | O que faz |
| --- | --- |
| `<Leader>ff` | Buscar arquivos |
| `<Leader>fw` | Buscar texto no projeto (ripgrep) |
| `<Leader>fo` | Arquivos recentes |
| `<Leader>fb` | Buffers abertos |
| `<Leader>fs` | Buffers + recentes + arquivos (busca "inteligente") |
| `<Leader>fh` | Ajuda do Neovim |
| `<Leader>fk` | Atalhos |
| `<Leader>ft` | Temas |
| `<Leader>fu` | Histórico de desfazer |
| `<Leader>fT` | Buscar TODOs no projeto |

## Código (`<Leader>l`)

Com um servidor de linguagem (LSP) ativo:

| Atalho | O que faz |
| --- | --- |
| `gd` | Ir para a definição |
| `gr` / `grr` | Referências |
| `K` | Documentação do símbolo |
| `<Leader>la` | Ação de código |
| `<Leader>lr` | Renomear símbolo |
| `<Leader>lf` | Formatar buffer |
| `<Leader>ld` | Ver diagnóstico da linha |
| `<Leader>lD` | Buscar diagnósticos |
| `<Leader>lS` | Estrutura de símbolos |
| `]d` / `[d` | Próximo / anterior diagnóstico |

Para instalar o servidor de uma linguagem: `<Leader>pm` (Mason) ou
`:LspInstall <nome>`. Os do Lua (`lua-language-server`, `stylua`) já vêm instalados.

## Git (`<Leader>g`)

| Atalho | O que faz |
| --- | --- |
| `<Leader>gt` | Status |
| `<Leader>gc` | Commits do repositório |
| `<Leader>gb` | Branches |
| `<Leader>gl` | Ver blame da linha |
| `<Leader>gp` | Pré-visualizar trecho alterado |
| `<Leader>gs` | Preparar trecho (stage) |
| `]g` / `[g` | Próximo / anterior trecho alterado |
| `<Leader>gg` | lazygit (se estiver instalado) |

## Terminal (`<Leader>t`)

| Atalho | O que faz |
| --- | --- |
| `<Leader>tf` | Terminal flutuante |
| `<Leader>th` / `<Leader>tv` | Terminal embaixo / ao lado |
| `F7` | Alternar o terminal |
| `Esc` `Esc` | Sair do modo terminal |

## Interface (`<Leader>u`)

| Atalho | O que faz |
| --- | --- |
| `<Leader>ub` | Alternar fundo claro/escuro |
| `<Leader>um` | Alternar **todas** as animações |
| `<Leader>uM` | Alternar só o scroll suave |
| `<Leader>uw` | Quebra de linha |
| `<Leader>un` | Numeração de linhas |
| `<Leader>uZ` | Modo zen |
| `<Leader>ud` | Diagnósticos |
| `<Leader>uz` / `<Leader>uT` | Destaque de cores / de TODOs |

## Plugins (`<Leader>p`)

| Atalho | O que faz |
| --- | --- |
| `<Leader>ps` | Status dos plugins |
| `<Leader>pa` | Atualizar Lazy e Mason |
| `<Leader>pm` | Instalador Mason (LSPs, formatadores, debuggers) |
