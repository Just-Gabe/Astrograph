---
title: Comandos e diagnóstico
nav_order: 5
---

# Comandos e diagnóstico

## `:Astrograph`

| Comando | O que faz |
| --- | --- |
| `:Astrograph atualizar` | Atualiza em segundo plano e sem risco: guarda suas alterações, avança o clone, sincroniza os plugins, recompila o app em Rust se ele mudou e confere que a config sobe; se não subir, volta para a versão anterior. Veja abaixo |
| `:Astrograph saude` | Abre o diagnóstico (`:checkhealth astrograph`) |
| `:Astrograph agentes` | Mostra cada agente de IA: instalado, logado e com o chat pronto |
| `:Astrograph preparo` | Mostra o que o preparo em segundo plano detectou (linguagens, git) e o que está rodando |
| `:Astrograph configurar` | Instala as pontes do chat e faz os logins que faltam |
| `:Astrograph login [agente]` | Abre o login de um agente (`claude`, `opencode`, `codex`, `gemini`) |
| `:Astrograph agente [agente]` | Escolhe o agente de todo o modo agentic (fica salvo) |
| `:Astrograph modo [modo]` | Escolhe o modo do agente em todas as telas (fica salvo): `manual` (Manual), `acceptEdits` (Aceitar edições), `plan` (Plano), `auto` (Auto), `bypass` (Sem permissões, pede confirmação) |
| `:Astrograph atalhos` | Busca entre todos os atalhos |
| `:Astrograph docs` | Abre esta documentação no navegador |

Sem argumento, `:Astrograph` abre o diagnóstico. O `Tab` completa os subcomandos.

## `:checkhealth astrograph`

Confere, em português, tudo o que o Astrograph precisa:

- versão do Neovim, de onde vêm o core e o tema;
- `git`, `ripgrep`, `fd`, compilador C e `lazygit` (opcional);
- agentes de IA: instalados, logados e com a ponte do chat; Ollama e modelo local.

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

### Como o `:Astrograph atualizar` protege você

| Situação | O que acontece |
| --- | --- |
| Sem internet | Avisa e não muda nada |
| Você editou arquivos da config (ou deixou arquivos soltos que atrapalhariam um novo) | Ficam guardados (`git stash`), a versão nova entra e as suas alterações voltam; se conflitarem, a versão nova fica limpa e as suas ficam no `git stash list` |
| O histórico do Astrograph foi reescrito (clone do instalador) | Segue para a versão nova e guarda a anterior em `astrograph-backup-<data>` (os 3 últimos) |
| Clone de desenvolvimento com commits seus | Recusa e não mexe em nada: atualize com `git merge`/`rebase` |
| A versão nova não carrega no seu computador | Volta para a anterior, devolve suas alterações e mostra o erro |
| Dois Neovim atualizando ao mesmo tempo | O segundo espera |
| O app em Rust mudou e há `cargo` | Recompila só o que mudou; o executável novo entra por rename, sem derrubar o app aberto |
