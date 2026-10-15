---
title: Início
layout: home
nav_order: 1
---

# Astrograph
{: .fs-9 }

Um Neovim pronto para usar, baseado no AstroNvim, com **agentes de IA dentro do
editor**, abertura mais rápida e tudo em **português**.
{: .fs-6 .fw-300 }

[Instalar agora](instalacao){: .btn .btn-primary .fs-5 .mb-4 .mb-md-0 .mr-2 }
[Ver no GitHub](https://github.com/Just-Gabe/Astrograph){: .btn .fs-5 .mb-4 .mb-md-0 }

![Abertura animada do Astrograph]({{ '/img/abertura.gif' | relative_url }})

## O que vem pronto

| | |
| --- | --- |
| **Agentes de IA** | Claude Code, opencode, Codex e Gemini no terminal flutuante e no chat; edição inline com o avante; IA local com Ollama. O instalador e o `:Astrograph configurar` cuidam dos logins, e o editor avisa antes se o agente escolhido estiver sem login. Tudo no prefixo `<Leader>A`. |
| **Em português** | Menus de atalhos, tela inicial, diagnóstico (`:checkhealth astrograph`) e mensagens. |
| **Rápido** | ~256 ms para abrir no Windows (o AstroNvim original, ~392 ms). Arquivos abrem em estágios: o essencial em ~59 ms. |
| **Visual no estilo Apple** | Tema grafite inspirado no Xcode, tela inicial limpa, barra de status discreta. Janelas com fade, explorador deslizante, `dd` animado, cursor animado, bordas arredondadas e ghost text. |
| **Ações com o agente** | Corrigir o erro da linha, explicar, revisar suas alterações e escrever a mensagem de commit, cada uma num atalho. |
| **Código próprio onde importa** | Cores e TODOs desenhados pelo motor nativo do Neovim: rolar a tela ficou ~10× mais leve. |
| **Tema próprio** | `astrograph` (escuro) e `astrograph-light`, com contraste verificado. |
| **Instalação de uma linha** | Windows, Linux e macOS: o instalador cuida de Neovim, Git, ripgrep, fd, compilador C e fonte. |

## Como ele é pensado

O Astrograph segue a mesma ideia dos produtos da Apple: parecer moderno e rápido
não por enfeite, mas por cuidado com o que você sente ao usar.

- **Resposta imediata.** A primeira busca é quase tão rápida quanto as seguintes:
  o que você costuma usar é pré-carregado quando o editor está ocioso.
- **O conteúdo em primeiro lugar.** Sem ruído entre as linhas de código, sem
  `~` no fim do arquivo, mensagens discretas.
- **Movimento com propósito.** Janelas surgem com fade, o explorador desliza, a
  linha apagada desce e some. Tudo curto, a 60 fps, desacelerando no fim, e
  nunca atrasando o que você está fazendo.
- **Acabamento uniforme.** Bordas arredondadas, rótulos que cabem e textos em
  português em toda a interface.
- **Calma para os olhos.** Um fundo grafite neutro, cores de sintaxe suaves e
  uma cor de destaque só; o que é secundário fica em cinza.

## UI nativa (em desenvolvimento)

Uma janela própria do Astrograph, escrita em Rust, com o Neovim como motor. Fora
do terminal, ela faz o que um terminal não consegue: o texto **desliza por
pixel** ao rolar, o cursor **desliza e pisca com fade**, as janelas flutuantes
ganham **sombra** de verdade, e a barra de título e a borda acompanham o tema.

| Abertura | Rolagem por pixel |
| --- | --- |
| ![Abertura na UI nativa]({{ '/img/ui/abertura.gif' | relative_url }}) | ![Rolagem suave]({{ '/img/ui/rolagem.gif' | relative_url }}) |
| **Cursor: desliza, muda de forma e pisca com fade** | **Busca com sombra** |
| ![Cursor]({{ '/img/ui/cursor.gif' | relative_url }}) | ![Painel com sombra]({{ '/img/ui/painel.gif' | relative_url }}) |

O código fica em `prototype/astrograph-ui` (branch `epic/rust-ui`).

## Em ação

![Editor com explorador de arquivos]({{ '/img/editor.png' | relative_url }})

Aperte `Espaço` e o menu mostra todos os atalhos, em português:

![Menu de atalhos]({{ '/img/atalhos.png' | relative_url }})

## Próximos passos

1. [Instale](instalacao) com um comando.
2. Veja os [primeiros passos](uso) e os atalhos principais.
3. Configure os [agentes de IA](agentes).
4. Algo estranho? Rode `:Astrograph saude` ou veja [Solução de problemas](problemas).
