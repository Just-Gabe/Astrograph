# Astrograph

**Um Neovim pronto para usar**, baseado no [AstroNvim](https://github.com/AstroNvim/AstroNvim),
com **agentes de IA dentro do editor**, abertura mais rápida e tudo em **português**.

📖 **Documentação:** <https://just-gabe.github.io/Astrograph/>

![Abertura animada do Astrograph](docs/img/abertura.gif)

## Instalar

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
```

**Linux / macOS** (bash):

```bash
curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh | bash
```

O instalador cuida de tudo: Neovim 0.11+, Git, ripgrep, fd, compilador C, a
JetBrainsMono Nerd Font, o clone do Astrograph e a ligação da config (com backup da
antiga). No fim, um checklist opcional instala agentes de IA: Claude Code, opencode,
Codex, Gemini e Ollama. Em seguida ele instala o que o chat precisa e oferece
fazer o **login de cada agente** ali mesmo. Depois é só abrir um terminal novo e
rodar `nvim`.

[Mais detalhes da instalação →](https://just-gabe.github.io/Astrograph/instalacao)

## O que vem pronto

- **Agentes de IA no prefixo `<Leader>A`**: Claude Code, opencode, Codex ou Gemini
  no terminal flutuante e no chat, usando o login de cada CLI (sem chave de API);
  edição inline com o avante; IA local com Ollama. `:Astrograph configurar`
  instala e faz login no que faltar.
- **Em português**: menu de atalhos, tela inicial, `:checkhealth astrograph` e
  mensagens.
- **Mais rápido**: abre em ~256 ms no Windows (o AstroNvim original, ~392 ms);
  arquivos abrem em estágios, com o essencial em ~59 ms.
- **Moderno no terminal**: linha de comando com destaque, cursor animado, bordas
  arredondadas, ghost text na completação e animações a 60 fps.
- **Resposta imediata**: o que você usa é pré-carregado quando o editor está
  ocioso, e o código aparece sem ruído (sem CodeLens nem `~` por padrão).
- **Movimento**: janelas com fade, explorador deslizante e `dd` animado (a
  linha colapsa e some), sempre sem atrasar a edição.
- **Um agente para tudo**: escolha com `<Leader>Ag` e terminal, chat e edição
  inline passam a usar o mesmo agente; a escolha fica salva.
- **Código próprio onde importa**: cores e TODOs desenhados pelo motor nativo
  do Neovim, no lugar de dois plugins; rolar a tela ficou ~10× mais leve.
- **Tema próprio**: `astrograph` (escuro) e `astrograph-light`, com contraste
  verificado.
- **Arquivos editados pelo agente recarregam sozinhos** no editor.
- **`:Astrograph`**: `atualizar`, `saude`, `agentes`, `configurar`, `login`, `atalhos`, `docs`.

| Editor | Menu de atalhos (`Espaço`) |
| --- | --- |
| ![Editor](docs/img/editor.png) | ![Atalhos](docs/img/atalhos.png) |
| **Agentes de IA (`Espaço` `A`)** | **Chat com o Claude Code (`Espaço` `A` `c`)** |
| ![Agentes](docs/img/agentes.png) | ![Chat](docs/img/chat.png) |
| **Completação com ghost text** | **Diagnóstico (`:Astrograph saude`)** |
| ![Completação](docs/img/completar.png) | ![Diagnóstico](docs/img/saude.png) |
| **Explorador deslizando (`<Leader>e`)** | **`dd`: a linha colapsa e some** |
| ![Explorador](docs/img/explorador.gif) | ![Apagar linha](docs/img/apagar-linha.gif) |

## Primeiros passos

| Atalho | O que faz |
| --- | --- |
| `Espaço` | Mostra todos os atalhos (espere meio segundo) |
| `<Leader>ff` / `<Leader>fw` | Buscar arquivo / texto |
| `<Leader>e` | Explorador de arquivos |
| `<Leader>w` / `<Leader>q` | Salvar / fechar janela |
| `<Leader>Al` | Abrir um agente de IA no terminal |
| `<Leader>Ac` | Chat com IA |
| `<Leader>AE` | Pedir uma edição para a IA (selecione o trecho antes) |
| `<Leader>um` | Ligar/desligar as animações |

`<Leader>` é a tecla **Espaço**. Veja a [lista completa](https://just-gabe.github.io/Astrograph/uso)
e o [guia dos agentes](https://just-gabe.github.io/Astrograph/agentes).

## Estrutura

```
core/       fork do core do AstroNvim (v6.1.0) com as otimizações
template/   a config que o Neovim abre
theme/      colorschemes astrograph e astrograph-light
tools/      geradores da arte da tela inicial e dos prints
docs/       documentação (GitHub Pages) e prints
```

[Como funciona por dentro →](https://just-gabe.github.io/Astrograph/desenvolvimento) ·
[Performance →](https://just-gabe.github.io/Astrograph/performance) ·
[Astrograph e AstroNvim →](https://just-gabe.github.io/Astrograph/astronvim) ·
[Solução de problemas →](https://just-gabe.github.io/Astrograph/problemas)

## Créditos e licença

Baseado no [AstroNvim](https://github.com/AstroNvim/AstroNvim) (GPL-3.0), de Micah
Halter ([mehalter](https://github.com/mehalter)) e colaboradores; usa o lazy.nvim e o
snacks.nvim de Folke Lemaitre. Mantido por [Just-Gabe](https://github.com/Just-Gabe).
O `core/` mantém a licença original; veja o [`LICENSE`](./LICENSE).
