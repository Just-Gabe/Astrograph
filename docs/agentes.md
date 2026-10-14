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

## Os três jeitos

| Jeito | Quando usar | Atalho |
| --- | --- | --- |
| **Terminal do agente** | Conversar com o Claude Code, opencode, Codex ou Gemini no TUI deles, com o projeto aberto ao lado. | `<Leader>Al` |
| **Chat (CodeCompanion)** | Chat dentro do Neovim, com o buffer/seleção como contexto. Funciona com APIs (Anthropic, OpenAI, Gemini), com agentes via ACP e com Ollama. | `<Leader>Ac` |
| **Edição inline (avante)** | Pedir uma alteração num trecho selecionado, estilo Cursor. Prefere o modelo local (Ollama) quando ele está no ar. | `<Leader>AE` |

## Todos os atalhos

| Atalho | O que faz |
| --- | --- |
| `<Leader>Al` | Abrir agente no terminal (pergunta qual, se houver mais de um) |
| `<Leader>AT` | Alternar o último terminal de agente |
| `<Leader>A.` | Enviar o caminho do arquivo atual para o agente |
| `<Leader>Ac` | Chat (abrir/fechar) |
| `<Leader>An` | Novo chat |
| `<Leader>Aq` | Assistente inline (CodeCompanion) |
| `<Leader>Ap` | Ações: trocar adapter/modelo, prompts prontos |
| `<Leader>Aa` | (visual) Adicionar a seleção ao chat |
| `<Leader>AE` | avante: perguntar (normal) / editar seleção (visual) |
| `<Leader>At` | avante: alternar a barra lateral |
| `<Leader>Af` | avante: focar |
| `<Leader>Az` | avante: modo zen |
| `<Leader>As` | avante: alternar sugestões automáticas |

Dentro do chat do CodeCompanion, `ga` troca o adapter/modelo.

## Detecção automática

Você não precisa configurar nada: o Astrograph vê o que existe na máquina e só
habilita o que funciona.

- **CLIs no PATH**: `claude`, `opencode`, `codex`, `gemini` aparecem no
  `<Leader>Al` e como agentes do CodeCompanion.
- **Chat padrão**: `claude_code` (via ACP) se o Claude Code estiver instalado;
  senão o Ollama local; senão a API da Anthropic.
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

## Chaves de API

Os adapters HTTP do CodeCompanion e o avante leem as chaves do ambiente:
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`. Os agentes de terminal
(Claude Code, opencode, …) usam o login de cada um:

```bash
claude              # autentica o Claude Code
opencode auth login
```

{: .warning }
Não coloque chaves de API dentro dos arquivos da config (eles estão num
repositório git). Use variáveis de ambiente.

## Arquivos editados pelo agente

Quando um agente altera um arquivo que está aberto, o Astrograph recarrega o
buffer sozinho: ao voltar o foco para o Neovim, ao sair do terminal do agente,
ao trocar de buffer e quando o cursor fica parado.
