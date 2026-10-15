---
title: Solução de problemas
nav_order: 10
---

# Solução de problemas
{: .no_toc }

Comece sempre por `:Astrograph saude`: ele diz o que está faltando e o que isso afeta.

1. TOC
{:toc}

## Aparecem quadrados ou `?` no lugar dos ícones

O terminal não está usando uma Nerd Font. O instalador instala a
**JetBrainsMono Nerd Font**; selecione-a nas configurações do terminal
(no Windows Terminal: *Configurações → Padrões → Aparência → Fonte*).

## As cores estão erradas ou o tema é o astrodark

- `:Astrograph saude` mostra se a pasta `theme/` foi encontrada.
- O terminal precisa de cores verdadeiras (24 bits). Windows Terminal, WezTerm,
  Kitty, Alacritty e o iTerm2 suportam; o Terminal antigo do macOS não.

## `<Leader>ff` ou `<Leader>fw` não fazem nada

Faltam o `fd` (arquivos) ou o `ripgrep` (texto). O Mason os instala sozinho na
primeira abertura; confira com `:Astrograph saude`, ou instale pelo
gerenciador de pacotes. No Debian/Ubuntu o `fd` se chama `fdfind`; o instalador
cria o atalho `~/.local/bin/fd`.

## Erro ao compilar parsers do Treesitter

Falta um compilador C. No Windows o instalador instala o gcc (WinLibs); abra um
terminal **novo** depois de instalar para o PATH valer. No Linux:
`build-essential` (Debian/Ubuntu) ou `gcc`.

## O agente não aparece no `<Leader>Al`

O executável (`claude`, `opencode`, `codex`, `gemini`) precisa estar no PATH do
terminal que abriu o Neovim. Instale-o, abra um terminal novo e confira com
`:Astrograph agentes`.

## O chat (`<Leader>Ac`) não abre

Rode `:Astrograph agentes`. Se aparecer "SEM LOGIN" ou "falta a ponte",
`:Astrograph configurar` resolve e depois é só reiniciar o Neovim. No Windows,
versões antigas do Astrograph não conseguiam iniciar a ponte do chat (os CLIs
do npm são scripts `.cmd`); atualize com `:Astrograph atualizar`.

## O chat do Claude Code responde "401 Invalid bearer token"

Versões anteriores mandavam um token inválido para a ponte do chat quando você
entrou com `claude auth login` (sem a variável `CLAUDE_CODE_OAUTH_TOKEN`).
Atualize com `:Astrograph atualizar`: a ponte passa a usar o login do CLI.

## O agente não responde no chat

Quase sempre é login: um agente sem login (por exemplo, o opencode sem
credenciais) aceita a mensagem e nunca responde. O Astrograph checa isso ao
abrir o chat e oferece entrar, trocar para um agente logado ou seguir; depois de
40 segundos sem resposta, aparece um aviso. Confira com `:Astrograph agentes` e
entre com `:Astrograph login <agente>`.

## O avante/chat não usa o Ollama

- O Ollama precisa estar rodando (`ollama serve`) e ter um modelo
  (`ollama pull qwen2.5-coder:7b`).
- `:Astrograph agentes` mostra se ele foi detectado e qual modelo será usado.
- Se mudou o `OLLAMA_HOST`, abra o Neovim no mesmo terminal em que a variável vale.

## O arquivo não atualiza depois que o agente editou

O recarregamento acontece ao voltar o foco para o Neovim, sair do terminal do
agente, trocar de buffer ou deixar o cursor parado. Se o buffer tiver alterações
**não salvas**, o Neovim pergunta antes de recarregar.

## `:Astrograph atualizar` falhou

Provavelmente há alterações locais no clone. Veja o que mudou:

```bash
git -C ~/Astrograph status            # Linux/macOS
git -C $env:LOCALAPPDATA\Astrograph status   # Windows
```

O `lazy-lock.json` é descartado automaticamente; outras alterações você decide
se guarda (`git stash`) ou descarta.

## Colar trouxe um texto antigo

A cópia em memória vale até o terminal perder o foco. Se você copiou algo em
outro programa sem sair da janela do terminal (raro), use `"+p` depois de
trocar de janela, ou desligue o recurso com `vim.g.astrograph_clipboard = false`
no topo do `template/init.lua`.

## Animações travando ou pesadas

`<Leader>um` desliga todas as animações; `<Leader>uM` só o scroll suave. Via
SSH elas já começam desligadas.

## Abrir sem nenhuma config (para comparar)

```bash
nvim --clean
```
