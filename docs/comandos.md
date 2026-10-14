---
title: Comandos e diagnóstico
nav_order: 5
---

# Comandos e diagnóstico

## `:Astrograph`

| Comando | O que faz |
| --- | --- |
| `:Astrograph atualizar` | Baixa a versão nova (`git pull` no clone) e sincroniza os plugins com o `lazy-lock.json` |
| `:Astrograph saude` | Abre o diagnóstico (`:checkhealth astrograph`) |
| `:Astrograph agentes` | Mostra os agentes de IA detectados (refaz a detecção) |
| `:Astrograph atalhos` | Busca entre todos os atalhos |
| `:Astrograph docs` | Abre esta documentação no navegador |

Sem argumento, `:Astrograph` abre o diagnóstico. O `Tab` completa os subcomandos.

## `:checkhealth astrograph`

Confere, em português, tudo o que o Astrograph precisa:

- versão do Neovim, de onde vêm o core e o tema;
- `git`, `ripgrep`, `fd`, compilador C e `lazygit` (opcional);
- agentes de IA no PATH, Ollama no ar e modelo local.

![Diagnóstico do Astrograph]({{ '/img/saude.png' | relative_url }})

Itens com ✅ estão ok; ⚠️ indica algo que limita um recurso; ❌ algo que impede o
editor de funcionar direito. Cada linha diz qual atalho ou recurso depende daquilo.

## Comandos úteis do Neovim/AstroNvim

| Comando | O que faz |
| --- | --- |
| `:Lazy` | Gerenciador de plugins |
| `:Mason` | Instalar LSPs, formatadores e debuggers |
| `<Leader>li` | Servidores de linguagem ativos no buffer |
| `:checkhealth` | Diagnóstico completo do Neovim e de todos os plugins |
| `:AstroVersion` | Versão do core |
