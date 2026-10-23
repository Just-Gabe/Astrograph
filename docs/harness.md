---
title: Agentes operando o editor
nav_order: 4.5
---

# Agentes operando o editor (harness)
{: .no_toc }

O app em Rust (motor nativo, o padrão) pode ser operado por um agente de IA, não só usado por ele: o agente vê a tela,
abre arquivos e pastas, usa os menus, manda teclas do Vim, roda comandos no terminal do app,
salva e faz commit. O mesmo intérprete de comandos serve a três portas:

| Porta | Para quem | Como |
| --- | --- | --- |
| **MCP** (`astrograph --mcp`) | Claude Code e qualquer cliente MCP | ferramentas `astrograph_*` |
| **CLI** (`astrograph --ctl <comando>`) | opencode, Codex, Gemini, scripts | uma linha de comando, resposta em JSON |
| **Roteiro** (`astrograph --roteiro arquivo.txt`) | testes e CI, sem janela | um comando por linha, prints em PNG |

1. TOC
{:toc}

## Claude Code: já vem ligado

Ao abrir o Claude Code pelo painel do agente (`<Espaço>Al`), o Astrograph passa a ele um
`--mcp-config` com o próprio executável em `--mcp`. Na conversa, o agente já tem:

| Ferramenta | O que faz |
| --- | --- |
| `astrograph_estado` | pasta, arquivo, modo, cursor, trecho em volta do cursor, abas, buscador ou menu aberto, explorador, terminal, avisos, diagnósticos |
| `astrograph_print` | a janela como imagem (PNG) |
| `astrograph_teclas` / `astrograph_texto` | teclas na notação do Vim (`:w<CR>`, `<Space>ff`, `<C-d>`) / texto literal |
| `astrograph_comando` | um comando `:` (`tabnew`, `vsplit`, `%s/a/b/g`, `Format`) |
| `astrograph_abrir` / `astrograph_abrir_pasta` | abre arquivo (na linha pedida) / pasta como projeto |
| `astrograph_salvar` | grava a aba ativa ou todas |
| `astrograph_buffer` | o texto de um arquivo aberto, com o que ainda não foi salvo |
| `astrograph_atalhos` / `astrograph_acao` | lista o menu da líder / executa um atalho dele (`ff`, `e`, `fc`...); recusa se você estiver no meio de uma inserção ou de um menu |
| `astrograph_terminal` | roda um comando num terminal de shell do app (nunca dentro do agente) e devolve a tela |
| `astrograph_git_status` / `astrograph_git_diff` / `astrograph_git_commit` | git do projeto aberto; o commit leva o que está no disco (não grava abas) e avisa as que estão sem salvar; `tudo` faz `git add -A` (respeita o `.gitignore`) |
| `astrograph_esperar` | espera (buscas, servidor de linguagem) e devolve o estado |

`comando`, `abrir_pasta`, `salvar` e `acao` vão sempre para o editor, mesmo com o terminal em foco
(o terminal recebe o foco de volta, menos depois de `acao`); `teclas` e `texto` vão para onde o foco
estiver. `abrir_pasta` responde erro quando a pasta não troca (aba sem salvar, por exemplo). Com o
editor ocupado (inserção, buscador, linha de comando, tecla pendente — inclusive escondido atrás do
terminal), `comando`, `abrir_pasta`, `salvar` e `acao` recusam em vez de digitar no arquivo; `comando`
aceita uma linha só.

As ações que mudam a tela devolvem o estado logo depois, para o agente conferir o efeito. Cada uma
aparece como aviso na sua tela ("Agente: ...") e vai para o registro `agentes.log` na pasta de
estado. Ferramenta MCP não passa pelas permissões de Bash do Claude Code: aprovar
`astrograph_terminal` para sempre equivale a liberar o shell.

Para usar o MCP com um Claude Code aberto fora do app:

```bash
claude mcp add astrograph -- astrograph --mcp
```

## CLI

Dentro do terminal do app (ou de um agente aberto por ele) o endereço já está no ambiente e aponta
para essa janela; fora dele, com várias janelas abertas, o CLI fala com a **aberta por último**.

```bash
astrograph --ctl estado
astrograph --ctl abrir src/main.rs 42
astrograph --ctl teclas "ciwnovo_nome<Esc>"
astrograph --ctl salvar todos
astrograph --ctl acao ff            # abre o buscador de arquivos
astrograph --ctl print              # grava um PNG e diz onde
astrograph --ctl terminal cargo test
astrograph --ctl terminal_tela
astrograph --ctl git_commit --tudo -- corrige o login
```

Vários comandos numa conexão: `astrograph --ctl -` lê um por linha da entrada (`espera 500`
espera no cliente).

Comandos: `estado`, `print [arquivo.png]`, `teclas`, `texto`, `comando`, `abrir <arquivo> [linha]`,
`pasta <pasta>`, `salvar [todos]`, `buffer [arquivo]`, `atalhos`, `acao <teclas>`,
`terminal <comando>`, `terminal_tela`, `git_status`, `git_diff [arquivo]`,
`git_commit [--tudo] [--em pasta] -- <mensagem>` (as opções só valem antes do `--`; sem ele, tudo é
mensagem), `clique <linha> <coluna>`, `rolar <linhas>`. O git roda sem pedir senha ou credencial e é
encerrado depois de 30 s.

## Roteiro (testes sem janela)

```bash
astrograph --roteiro teste.txt --saida prints --tamanho 1280x800 src/main.rs
```

Além dos comandos acima: `espera <ms>` (o tempo corre e a tela é redesenhada),
`quadros <nome> <ms> [passo]` (um PNG por quadro, para ver uma animação) e
`congelou <ms>` (pinta como o app pinta e diz se uma animação parou antes do fim ou nunca para).
Cada comando imprime uma linha JSON; o processo sai com 1 se algum falhar.

## Segurança

- O servidor só escuta em `127.0.0.1`, numa porta aleatória, e exige o token da sessão
  (256 bits, comparado em tempo constante). Pedido sem o token certo fecha a conexão sem executar
  nada; pedido em HTTP (um navegador tentando a porta) também.
- O token fica em `controle.json` na pasta de estado do usuário (`%LOCALAPPDATA%\astrograph-state`
  ou `~/.local/state/astrograph`, com permissão só do dono) e no ambiente dos terminais que o app
  abre (`ASTROGRAPH_CONTROLE`, `ASTROGRAPH_TOKEN`), o que inclui o que roda neles (scripts de
  `npm`, `cargo`...). Os arquivos somem quando o app fecha (se ele travar, sobra um `controle.json`
  apontando para uma porta morta, que o CLI acusa).
- Um pedido que não termina em 60 s é descartado (não roda depois), e o MCP só tenta de novo as
  ferramentas que não mudam nada.
- Os prints ficam na pasta `prints/` (os 50 mais novos); o nome pedido não escolhe outra pasta.

## Ligar e desligar

Vem **ligado**. Para desligar (e religar) há três jeitos, e a escolha fica gravada:

- **`<Espaço>AM`** liga ou desliga na hora;
- **Configurações** (`<Espaço>fc`): "Agentes operam o editor (MCP)";
- no `ui.toml`:

```toml
[agentes]
controle = false
```

Desligado, não há servidor, porta, token nem `controle.json`: um agente já aberto recebe a recusa com
o aviso de como religar, e o Claude Code aberto pelo painel não recebe as ferramentas. Religar sobe o
servidor com outro token. `ASTROGRAPH_CONTROLE=0` no ambiente desliga só aquela sessão.
