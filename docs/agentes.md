---
title: Agentes de IA
nav_order: 4
---

# Agentes de IA
{: .no_toc }

O "modo agentic" do Astrograph junta três formas de trabalhar com IA no editor,
todas no prefixo **`<Leader>A`** (Espaço + A maiúsculo).

1. TOC
{:toc}

![Menu de agentes]({{ '/img/agentes.png' | relative_url }})

## Configurar em 1 minuto

O instalador já faz isto. Para conferir ou refazer, dentro do `nvim`:

```vim
:Astrograph configurar
```

Ele verifica cada agente instalado e oferece, num terminal flutuante:

- **instalar a ponte do chat** (ACP) que faltar;
- **fazer login** nos agentes que ainda não estão logados (abre o navegador);
- **instalar outros agentes** (Claude Code, opencode, Codex, Gemini).

Depois, reinicie o Neovim. `:Astrograph agentes` mostra a situação de cada um:

```
Agentes de IA:
  ✓ Claude Code  logado · chat ok
  ✓ opencode     SEM LOGIN · chat ok
  · Codex CLI    não instalado
```

Para entrar em um agente específico: `:Astrograph login claude` (ou `opencode`,
`codex`, `gemini`).

### O que cada agente precisa

| Agente | Login | Ponte do chat (instalada automaticamente) |
| --- | --- | --- |
| Claude Code | `claude auth login` (conta claude.ai ou API) | `@agentclientprotocol/claude-agent-acp` |
| opencode | `opencode auth login` | embutida (`opencode acp`) |
| Codex CLI | `codex login` (ChatGPT) ou `OPENAI_API_KEY` | `@agentclientprotocol/codex-acp` |
| Gemini CLI | na 1ª execução do `gemini` (saia com `/quit`) ou `GEMINI_API_KEY` | embutida |

{: .note }
O chat, o terminal e as ações (corrigir, explicar, revisar, commit) usam **o
login do próprio CLI**: não é preciso token nem chave de API. O assistente
inline (`<Leader>Aq`) fica mais rápido com uma chave (`ANTHROPIC_API_KEY`,
`OPENAI_API_KEY` ou `GEMINI_API_KEY`) ou com o Ollama; sem nenhum dos dois, ele
usa o agente atual pelo avante.

{: .important }
**Agente sem login não responde.** O Astrograph checa o login em segundo plano
logo depois de abrir. Se o agente escolhido estiver sem login, abrir o chat (ou
pedir uma ação) mostra as opções: entrar nele, usar outro agente que já está
logado ou seguir assim mesmo. Se uma resposta passar de 40 segundos, um aviso
aparece em vez de uma espera sem fim.

## Um agente para tudo

Você escolhe **um** agente e ele vale para o modo agentic inteiro: terminal
(`<Leader>Al`/`<Leader>AT`), chat (`<Leader>Ac`), edição inline do avante
(`<Leader>AE`) e a interação CLI. A escolha fica salva e continua valendo ao
reabrir o Neovim.

| Como trocar | |
| --- | --- |
| `<Leader>Ag` | Escolher o agente numa lista (o atual aparece com ●) |
| `:Astrograph agente opencode` | Trocar direto pelo nome (`Tab` completa) |
| `ga` dentro do chat | Trocar o agente do chat também troca o de todo o resto |

A troca vale na hora, sem reiniciar. Sem a ponte do chat instalada para o
agente escolhido, o terminal já muda e o aviso diz para rodar
`:Astrograph configurar`.

## Peça ao agente

Ações prontas para o dia a dia. Cada uma abre um chat novo com o agente atual,
já com o contexto certo, e envia o pedido:

| Atalho | O que faz |
| --- | --- |
| `<Leader>Ax` | **Corrigir** o problema (diagnóstico) da linha do cursor: manda o erro e o trecho ao redor; o agente explica a causa e aplica a correção |
| `<Leader>Ai` | **Explicar** a seleção (modo visual) ou o arquivo inteiro |
| `<Leader>Ar` | **Revisar** as alterações ainda não commitadas: bugs e riscos, do mais grave ao menos grave, com arquivo e linha |
| `<Leader>Am` | **Mensagem de commit** das alterações (preparadas com `git add`, senão as demais), em português e no padrão conventional commits |
| `<Leader>Ad` | **Ver as alterações** (suas ou do agente) trecho a trecho, com o diff ao lado |
| `<Leader>A.` | **Mencionar** o arquivo atual no terminal do agente (`@caminho`); no modo visual, com as linhas (`@caminho#L10-20`) |

Enquanto o agente trabalha, a barra de status mostra `✦ Claude Code ⋯`.

## Os três jeitos

| Jeito | Quando usar | Atalho |
| --- | --- | --- |
| **Terminal do agente** | Conversar com o Claude Code, opencode, Codex ou Gemini no TUI deles, com o projeto aberto ao lado. | `<Leader>Al` |
| **Chat (CodeCompanion)** | Chat dentro do Neovim, com o buffer/seleção como contexto. Funciona com APIs (Anthropic, OpenAI, Gemini), com agentes via ACP e com Ollama. | `<Leader>Ac` |
| **Edição inline (avante)** | Pedir uma alteração num trecho selecionado, estilo Cursor. Usa o Ollama quando ele está no ar; senão o Claude Code pelo seu login. | `<Leader>AE` |

## Todos os atalhos

| Atalho | O que faz |
| --- | --- |
| `<Leader>Ag` | Escolher o agente (vale para terminal, chat e edição) |
| `<Leader>Al` | Abrir o agente atual no terminal |
| `<Leader>AT` | Alternar o terminal do agente atual |
| `<Leader>A.` | Mencionar o arquivo (ou a seleção) no terminal do agente |
| `<Leader>Ax` | Corrigir o problema da linha |
| `<Leader>Ai` | Explicar a seleção ou o arquivo |
| `<Leader>Ar` | Revisar as alterações não commitadas |
| `<Leader>Am` | Mensagem de commit |
| `<Leader>Ad` | Ver as alterações (diff) |
| `<Leader>Ac` | Chat (abrir/fechar) |
| `<Leader>An` | Novo chat |
| `<Leader>Aq` | Assistente inline (CodeCompanion com chave/Ollama; senão o agente pelo avante) |
| `<Leader>Ap` | Ações: trocar adapter/modelo, prompts prontos |
| `<Leader>Aa` | (visual) Adicionar a seleção ao chat |
| `<Leader>AE` | avante: perguntar (normal) / editar seleção (visual) |
| `<Leader>At` | avante: alternar a barra lateral |
| `<Leader>Af` | avante: focar |
| `<Leader>Az` | avante: modo zen |
| `<Leader>As` | avante: alternar sugestões automáticas |

Dentro do chat do CodeCompanion, `ga` troca o agente/modelo. As respostas vêm em
português.

![Chat com o Claude Code]({{ '/img/chat.png' | relative_url }})

## Detecção automática

Você não precisa configurar nada: o Astrograph vê o que existe na máquina e só
habilita o que funciona.

- **CLIs no PATH**: `claude`, `opencode`, `codex`, `gemini` aparecem no
  `<Leader>Al` e como agentes do CodeCompanion.
- **Chat padrão**: o primeiro agente com a ponte do chat instalada (Claude
  Code, opencode, Codex, Gemini, nessa ordem); senão o Ollama local; senão o
  primeiro provedor com chave de API no ambiente.
- **Ollama**: detectado pela porta (`OLLAMA_HOST` ou `localhost:11434`), sem
  atrasar a abertura do editor; a checagem só roda quando um plugin de IA carrega.

Para ver o que foi detectado:

```vim
:Astrograph agentes
```

## IA local com Ollama

1. Instale o [Ollama](https://ollama.com) (ou marque-o no instalador).
2. Baixe um modelo de código, por exemplo `ollama pull qwen2.5-coder:7b`.
3. Pronto: o avante e o chat passam a usar o modelo local.

O modelo escolhido vem, nesta ordem, de:

1. variável `ASTROGRAPH_LOCAL_MODEL`;
2. `config.json` gravado pelo instalador (`%APPDATA%\astrograph` no Windows,
   `~/.config/astrograph` no Linux/macOS);
3. primeiro modelo listado pelo Ollama.

## Outros servidores locais (LM Studio, llama.cpp, vLLM)

Qualquer servidor compatível com a API da OpenAI vira o adapter
`astrograph_local` do CodeCompanion:

```bash
export ASTROGRAPH_LOCAL_URL=http://localhost:1234
export ASTROGRAPH_LOCAL_MODEL=qwen2.5-coder-7b-instruct
export ASTROGRAPH_LOCAL_KEY=opcional
```

## Chaves de API (opcional)

Só são necessárias para o assistente inline (`<Leader>Aq`) e para usar um
provedor de API direto no chat. Leia-as do ambiente:
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`.

{: .warning }
Não coloque chaves de API dentro dos arquivos da config (eles estão num
repositório git). Use variáveis de ambiente.

## Arquivos editados pelo agente

Quando um agente altera um arquivo que está aberto, o Astrograph recarrega o
buffer sozinho: ao voltar o foco para o Neovim, ao sair do terminal do agente,
ao trocar de buffer e quando o cursor fica parado. Para conferir o que mudou,
use `<Leader>Ad`.

## Processos que não ficam para trás

O chat conversa com o agente por uma "ponte" (um processo `node` com o CLI do
agente, ~150 MB). No Windows, se o Neovim for fechado de forma abrupta
(terminal fechado, travamento), essa ponte não morria junto e se acumulava a
cada vez. Agora, alguns segundos depois de abrir, o Astrograph encerra em
silêncio as pontes cujo Neovim já não existe; as de um editor aberto não são
tocadas.
