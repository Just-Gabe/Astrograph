#!/usr/bin/env bash
# Astrograph — instalador para Linux/macOS (uma linha)
#
#   curl -fsSL https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.sh | bash
#
# O que ele faz:
#   1. Detecta o gerenciador de pacotes e instala: git, ripgrep, fd, compilador C e curl/unzip
#   2. Garante Neovim 0.11+ (usa o do sistema; se for antigo, baixa o tarball oficial)
#   3. Instala a Nerd Font JetBrainsMono em ~/.local/share/fonts (+ fc-cache)
#   4. Clona o Astrograph em ~/Astrograph
#   5. Liga a config: ~/.config/nvim  ->  ~/Astrograph/template. Se já houver
#      uma config do Neovim, pergunta: limpar e substituir, importar ou manter
#      (ASTROGRAPH_CONFIG_EXISTENTE=limpar|importar|manter responde sem perguntar)
#   6. Instala o Rust (rustup) e compila o agente nativo e o app Astrograph
#   7. Baixa os plugins do Neovim (Lazy) + ferramentas do Mason
#
# É idempotente: pode rodar de novo sem medo.

set -euo pipefail

REPO="https://github.com/Just-Gabe/Astrograph.git"
ROOT="$HOME/Astrograph"
CONFIG="$HOME/.config/nvim"
FONT_DIR="$HOME/.local/share/fonts"
BIN_DIR="$HOME/.local/bin"

info() { printf '\033[36m[astrograph]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[astrograph]\033[0m %s\n' "$*"; }

as_root() {
  if [ "$(id -u)" -eq 0 ]; then "$@"; elif command -v sudo >/dev/null 2>&1; then sudo "$@"; else "$@"; fi
}

# npm global: sem sudo quando o prefixo é gravável (nvm, Homebrew, ~/.npm-global)
npm_global() {
  local prefix
  prefix="$(npm prefix -g 2>/dev/null || true)"
  if [ "$OS" = "Darwin" ] || [ "$(id -u)" -eq 0 ] || { [ -n "$prefix" ] && [ -w "$prefix/lib" ]; }; then
    npm "$@"
  else
    as_root npm "$@"
  fi
}

OS="$(uname -s)"
ARCH="$(uname -m)"

TTY=""
if exec 3<>/dev/tty 2>/dev/null; then TTY=1; fi

# --- 0. Config do Neovim que já existe na máquina --------------------------
# Pergunta logo no início: escolher "manter" não deve custar a instalação toda.
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
DATA_DIR="$DATA_HOME/nvim"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/nvim"
LOCAL_DIR="${ASTROGRAPH_LOCAL:-$DATA_HOME/astrograph-local}"
EXISTING=""      # pasta com a config de antes (ou destino do link antigo)
EXISTING_LINK=""
CONFIG_MODE="nova"
if [ -L "$CONFIG" ]; then
  target="$(readlink -f "$CONFIG" 2>/dev/null || readlink "$CONFIG")"
  if [ -d "$target" ] && [ "$target" != "$(cd "$ROOT/template" 2>/dev/null && pwd -P)" ]; then
    EXISTING="$target"; EXISTING_LINK=1
  fi
elif [ -d "$CONFIG" ] && [ -n "$(ls -A "$CONFIG" 2>/dev/null)" ]; then
  EXISTING="$CONFIG"
fi
if [ -n "$EXISTING" ]; then
  CONFIG_MODE="$(printf '%s' "${ASTROGRAPH_CONFIG_EXISTENTE:-}" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')"
  case "$CONFIG_MODE" in
    limpar | importar | manter | "") ;;
    *) warn "ASTROGRAPH_CONFIG_EXISTENTE='$CONFIG_MODE' não reconhecido (use limpar, importar ou manter)."; CONFIG_MODE="" ;;
  esac
  if [ -z "$CONFIG_MODE" ] && [ -f "$EXISTING/init.lua" ] && grep -q Astrograph "$EXISTING/init.lua"; then
    # cópia antiga do próprio Astrograph: importar duplicaria os plugins dele
    info "Achei uma cópia antiga do Astrograph em $EXISTING: vai para backup e é substituída."
    CONFIG_MODE="limpar"
  fi
  if [ -z "$CONFIG_MODE" ]; then
    if [ -n "$TTY" ]; then
      desc="$EXISTING"; [ -n "$EXISTING_LINK" ] && desc="um link para $EXISTING"
      printf '\n'
      warn "Já existe uma config do Neovim: $desc"
      printf '  [1] Limpar e substituir  config e dados antigos (plugins, Mason) vão para backup; começa do zero\n'
      printf '  [2] Importar             faz o backup e traz seus plugins (lua/plugins), polish, módulos,\n'
      printf '                           ftplugin/after/snippets para os plugins locais (%s)\n' "$LOCAL_DIR"
      printf '  [3] Manter               não mexe na config do Neovim (instala o resto do Astrograph)\n'
      printf '  escolha [1]: '
      IFS= read -r -u 3 pick || pick=""
      case "$pick" in 2) CONFIG_MODE="importar" ;; 3) CONFIG_MODE="manter" ;; *) CONFIG_MODE="limpar" ;; esac
    else
      info "Sem terminal interativo: a config existente vai para backup e é substituída (ASTROGRAPH_CONFIG_EXISTENTE muda isso)."
      CONFIG_MODE="limpar"
    fi
  fi
  info "Config existente: $CONFIG_MODE"
fi

# --- 1. Dependências do sistema -------------------------------------------
info "Sistema: $OS ($ARCH)"
# só chama o gerenciador de pacotes (apt update, brew...) se faltar algo
missing=""
for b in git rg curl unzip cc; do command -v "$b" >/dev/null 2>&1 || missing="$missing $b"; done
command -v fd >/dev/null 2>&1 || command -v fdfind >/dev/null 2>&1 || missing="$missing fd"
if [ -z "$missing" ]; then
  info "Dependências já instaladas (git, ripgrep, fd, curl, unzip, compilador C)"
elif [ "$OS" = "Darwin" ]; then
  if ! command -v brew >/dev/null 2>&1; then
    warn "Homebrew não encontrado. Instale em https://brew.sh e rode de novo."
    exit 1
  fi
  brew install git ripgrep fd neovim curl unzip fontconfig 2>/dev/null || \
    brew install git ripgrep fd neovim curl unzip
  xcode-select -p >/dev/null 2>&1 || warn "Rode 'xcode-select --install' para o compilador C (Treesitter)."
elif [ "$OS" = "Linux" ]; then
  if command -v apt-get >/dev/null 2>&1; then
    as_root apt-get update -y
    as_root apt-get install -y git curl unzip tar xz-utils ca-certificates \
      ripgrep fd-find build-essential fontconfig
  elif command -v dnf >/dev/null 2>&1; then
    as_root dnf install -y git curl unzip tar xz ripgrep fd-find gcc make fontconfig
  elif command -v pacman >/dev/null 2>&1; then
    as_root pacman -Sy --noconfirm --needed git curl unzip tar xz ripgrep fd base-devel fontconfig
  elif command -v zypper >/dev/null 2>&1; then
    as_root zypper install -y git curl unzip tar xz ripgrep fd gcc make fontconfig
  elif command -v apk >/dev/null 2>&1; then
    as_root apk add git curl unzip tar xz ripgrep fd gcc musl-dev make fontconfig
  else
    warn "Gerenciador de pacotes não reconhecido. Instale manualmente: git, ripgrep, fd, gcc, curl, unzip."
  fi
elif [ "$OS" != "Linux" ]; then
  warn "Sistema não suportado: $OS"; exit 1
fi
[ -n "$missing" ] && info "Faltava:$missing"

mkdir -p "$BIN_DIR"
export PATH="$BIN_DIR:$PATH"

# Debian/Ubuntu instalam o fd como `fdfind`; o picker do Snacks procura `fd`.
if ! command -v fd >/dev/null 2>&1 && command -v fdfind >/dev/null 2>&1; then
  ln -sfn "$(command -v fdfind)" "$BIN_DIR/fd"
  info "fd: atalho $BIN_DIR/fd -> fdfind"
fi

# --- 2. Neovim 0.11+ -------------------------------------------------------
need_nvim=1
if command -v nvim >/dev/null 2>&1; then
  ver="$(nvim --version 2>/dev/null | head -n1 | sed -E 's/^NVIM v//')"
  if [ "$(printf '%s\n' '0.11.0' "$ver" | sort -V | head -n1)" = "0.11.0" ]; then
    info "Neovim $ver encontrado (>= 0.11)"
    need_nvim=0
  else
    info "Neovim $ver é antigo; baixando a versão oficial..."
  fi
fi
if [ "$need_nvim" -eq 1 ] && [ "$OS" = "Darwin" ]; then
  # o tarball abaixo é de Linux; no macOS o Homebrew é a fonte do Neovim
  brew upgrade neovim 2>/dev/null || warn "Atualize o Neovim para 0.11+ (brew upgrade neovim)."
  need_nvim=0
fi
if [ "$need_nvim" -eq 1 ]; then
  case "$ARCH" in
    x86_64|amd64) pkg="nvim-linux-x86_64" ;;
    aarch64|arm64) pkg="nvim-linux-arm64" ;;
    *) pkg="" ;;
  esac
  if [ -n "$pkg" ]; then
    url="https://github.com/neovim/neovim/releases/latest/download/$pkg.tar.gz"
    mkdir -p "$HOME/.local/opt"
    curl -fL "$url" -o /tmp/nvim.tar.gz
    tar -xzf /tmp/nvim.tar.gz -C "$HOME/.local/opt"
    ln -sfn "$HOME/.local/opt/$pkg/bin/nvim" "$BIN_DIR/nvim"
    info "Neovim instalado em $HOME/.local/opt/$pkg"
  else
    warn "Arquitetura $ARCH sem tarball oficial; use o Neovim do seu gerenciador (0.11+)."
  fi
fi

# --- 3. Nerd Font JetBrainsMono -------------------------------------------
mkdir -p "$FONT_DIR"
if [ -f "$FONT_DIR/JetBrainsMonoNerdFont-Regular.ttf" ]; then
  info "Nerd Font JetBrainsMono já instalada"
else
  info "Baixando a Nerd Font JetBrainsMono..."
  curl -fL "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.zip" -o /tmp/jbmono.zip
  unzip -o /tmp/jbmono.zip -d "$FONT_DIR" >/dev/null
  command -v fc-cache >/dev/null 2>&1 && fc-cache -f "$FONT_DIR" >/dev/null 2>&1 || true
  info "Nerd Font instalada (configure a fonte do seu terminal)"
fi

# --- 4. Clonar/atualizar o Astrograph -------------------------------------
if [ -d "$ROOT/.git" ]; then
  info "Atualizando $ROOT"
  # o `:Lazy update` reescreve o lazy-lock.json versionado; sem descartar essa
  # alteração o pull falha e a instalação continua velha sem avisar
  git -C "$ROOT" checkout -- template/lazy-lock.json 2>/dev/null || true
  git -C "$ROOT" pull --ff-only || warn "Não consegui atualizar $ROOT (alterações locais?). Rode: git -C \"$ROOT\" status"
else
  info "Clonando o Astrograph em $ROOT"
  git clone --depth 1 "$REPO" "$ROOT"
fi
# Arquivo da config sumido do clone (pull interrompido...): o nvim abriria com
# "module 'polish' not found". Restaura só o que foi apagado.
gone="$(git -C "$ROOT" ls-files -d -- template 2>/dev/null || true)"
if [ -n "$gone" ]; then
  printf '%s\n' "$gone" | (cd "$ROOT" && xargs git checkout --)
  info "Restaurados $(printf '%s\n' "$gone" | wc -l | tr -d ' ') arquivo(s) da config que faltavam no clone"
fi

# --- 5. Ligar a config -----------------------------------------------------
# Traz o que dá para reaproveitar da config antiga para os plugins locais
# (astrograph-local, fora do git). Nunca sobrescreve o que já esteja lá.
IMPORT_DONE=()
IMPORT_KEPT=()
put() {
  if [ -e "$2" ]; then IMPORT_KEPT+=("$2"); return 0; fi
  mkdir -p "$(dirname "$2")"
  cp -R "$1" "$2"
  IMPORT_DONE+=("$2")
}
import_nvim_config() {
  local src="$1" f d name body=""
  mkdir -p "$LOCAL_DIR/specs" "$LOCAL_DIR/plugin" "$LOCAL_DIR/lua"
  # specs do lazy: cada arquivo de lua/plugins vira um spec local
  if [ -d "$src/lua/plugins" ]; then
    for f in "$src/lua/plugins"/*.lua; do
      if [ -f "$f" ]; then put "$f" "$LOCAL_DIR/specs/importado-$(basename "$f")"; fi
    done
    for d in "$src/lua/plugins"/*/; do
      [ -d "$d" ] || continue
      name="$(basename "$d")"
      put "${d%/}" "$LOCAL_DIR/lua/plugins/$name"
      if [ -f "$d/init.lua" ]; then put "$d/init.lua" "$LOCAL_DIR/specs/importado-$name.lua"; fi
    done
  fi
  if [ -f "$src/lua/community.lua" ]; then put "$src/lua/community.lua" "$LOCAL_DIR/specs/importado-community.lua"; fi
  # polish roda depois dos plugins: vira um plugin/ do astrograph-local
  if [ -f "$src/lua/polish.lua" ]; then put "$src/lua/polish.lua" "$LOCAL_DIR/plugin/zz-importado-polish.lua"; fi
  # demais módulos (com os nomes de origem, para os `require` continuarem valendo)
  if [ -d "$src/lua" ]; then
    for f in "$src/lua"/* "$src/lua"/.[!.]*; do
      [ -e "$f" ] || continue
      case "$(basename "$f")" in plugins | polish.lua | lazy_setup.lua | community.lua | astrograph) continue ;; esac
      put "$f" "$LOCAL_DIR/lua/$(basename "$f")"
    done
  fi
  # LazyVim: options/keymaps/autocmds são carregados pela distro; aqui, por nós
  for name in options keymaps autocmds; do
    if [ -f "$src/lua/config/$name.lua" ]; then body="${body}pcall(require, \"config.$name\")"$'\n'; fi
  done
  if [ -n "$body" ]; then
    printf -- '-- importado da config antiga (LazyVim)\n%s' "$body" >/tmp/astrograph-importado-config.lua
    put /tmp/astrograph-importado-config.lua "$LOCAL_DIR/plugin/zz-importado-config.lua"
    rm -f /tmp/astrograph-importado-config.lua
  fi
  # pastas que o Neovim lê direto do runtimepath
  for d in after ftplugin ftdetect snippets spell queries syntax indent colors autoload; do
    if [ -d "$src/$d" ]; then put "$src/$d" "$LOCAL_DIR/$d"; fi
  done
  # init.lua solto (só opções/atalhos) entra; um que monta o próprio lazy.nvim, não
  if [ -f "$src/init.lua" ] && ! grep -q lazy "$src/init.lua"; then
    put "$src/init.lua" "$LOCAL_DIR/plugin/00-importado-init.lua"
  elif [ -f "$src/init.vim" ]; then
    put "$src/init.vim" "$LOCAL_DIR/plugin/00-importado-init.vim"
  elif [ -f "$src/init.lua" ]; then
    warn "init.lua antigo monta o próprio gerenciador de plugins: não importei ($src/init.lua). Copie à mão o que quiser."
  fi
  {
    printf 'Importado em %s de: %s\n\nPara desfazer, apague os arquivos abaixo.\n\n' "$(date '+%Y-%m-%d %H:%M')" "$src"
    for f in ${IMPORT_DONE[@]+"${IMPORT_DONE[@]}"}; do printf '%s\n' "$f"; done
    if [ "${#IMPORT_KEPT[@]}" -gt 0 ]; then
      printf '\nNão copiados (já existiam):\n'
      for f in "${IMPORT_KEPT[@]}"; do printf '%s\n' "$f"; done
    fi
  } >>"$LOCAL_DIR/IMPORTADO.txt"
  info "Importados ${#IMPORT_DONE[@]} item(ns) para $LOCAL_DIR (lista em IMPORTADO.txt)"
  if [ "${#IMPORT_KEPT[@]}" -gt 0 ]; then warn "${#IMPORT_KEPT[@]} item(ns) já existiam em $LOCAL_DIR e foram mantidos"; fi
}

stamp="$(date +%Y%m%d-%H%M%S)"
if [ "$CONFIG_MODE" = "manter" ]; then
  warn "Config do Neovim mantida: o nvim continua abrindo a sua config, não a do Astrograph."
else
  mkdir -p "$(dirname "$CONFIG")"
  if [ -n "$EXISTING" ]; then
    if [ -n "$EXISTING_LINK" ]; then
      rm -f "$CONFIG" # só o link; o destino fica intacto
      info "Link antigo removido ($CONFIG -> $EXISTING; o destino ficou intacto)"
    else
      EXISTING="$CONFIG.backup-$stamp"
      mv "$CONFIG" "$EXISTING"
      info "Config anterior movida para $EXISTING"
    fi
    if [ "$CONFIG_MODE" = "importar" ]; then import_nvim_config "$EXISTING"; fi
    if [ "$CONFIG_MODE" = "limpar" ]; then
      # plugins/estado da config antiga misturados com os do Astrograph dão
      # erro difícil de achar; o backup permite voltar
      for d in "$DATA_DIR" "$STATE_DIR"; do
        if [ -d "$d" ]; then mv "$d" "$d.backup-$stamp" && info "Dados antigos movidos para $d.backup-$stamp"; fi
      done
    fi
    # cache de bytecode do vim.loader: se refaz sozinho
    rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/nvim/luac"
  fi
  # pasta vazia que sobrou (rmdir falha em vez de apagar se houver conteúdo)
  if [ -d "$CONFIG" ] && [ ! -L "$CONFIG" ]; then rmdir "$CONFIG"; fi
  ln -sfn "$ROOT/template" "$CONFIG"
  info "Ligado: $CONFIG -> $ROOT/template"
fi

# --- 6. Agente nativo e app Astrograph (Rust) ------------------------------
# Não há executável publicado para Linux/macOS: instala o Rust (rustup, sem
# root) e compila do código. O editor procura o agente em ~/Astrograph/bin.
APP_BIN="$ROOT/bin"
mkdir -p "$APP_BIN"
COMMIT="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo desconhecido)"
RUST_READY=""
ensure_rust() {
  if [ "$RUST_READY" = 1 ]; then return 0; fi
  if [ "$RUST_READY" = 0 ]; then return 1; fi
  RUST_READY=0
  # shellcheck disable=SC1091
  if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
  if ! command -v cargo >/dev/null 2>&1; then
    info "Instalando o Rust (rustup, toolchain estável)..."
    if ! curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
      sh -s -- -y --profile minimal --default-toolchain stable >/tmp/astrograph-rustup.log 2>&1; then
      warn "Não consegui instalar o Rust (log em /tmp/astrograph-rustup.log)."
      return 1
    fi
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
    info "Rust instalado"
  fi
  command -v cargo >/dev/null 2>&1 || return 1
  RUST_READY=1
}
# build_binary <rótulo> <crate> <executável instalado>
build_binary() {
  local label="$1" crate="$2" bin="$3"
  local stampf="$APP_BIN/$3.version" log="/tmp/astrograph-build-$2.log"
  if [ -x "$APP_BIN/$bin" ] && [ "$(cat "$stampf" 2>/dev/null)" = "src-$COMMIT" ]; then
    info "$label já compilado desta versão"
    return 0
  fi
  if ! ensure_rust; then warn "$label não instalado: falta o Rust para compilar."; return 1; fi
  info "Compilando o $label (Rust; ~2 a 5 min na primeira vez)..."
  if cargo build --release --manifest-path "$ROOT/prototype/$crate/Cargo.toml" >"$log" 2>&1 &&
    cp "$ROOT/prototype/$crate/target/release/$crate" "$APP_BIN/$bin"; then
    printf 'src-%s\n' "$COMMIT" >"$stampf"
    info "$label compilado e instalado"
  else
    warn "Não consegui compilar o $label. Final do log ($log):"
    tail -n 15 "$log" | sed 's/^/    /'
    return 1
  fi
}
if [ "${ASTROGRAPH_SEM_COMPILAR:-}" = "1" ]; then
  warn "ASTROGRAPH_SEM_COMPILAR=1: pulei o agente nativo e o app."
else
  # ponte com os agentes (Claude Code, opencode...): usada por <Leader>Ae no editor
  build_binary "agente nativo" astrograph-agent astrograph-agent || true
  if build_binary "app Astrograph" astrograph-ui astrograph; then
    ln -sfn "$APP_BIN/astrograph" "$BIN_DIR/astrograph"
    info "Comando \"astrograph\" em $BIN_DIR"
  else
    warn "Sem o app, o Astrograph segue funcionando no terminal (nvim)."
  fi
fi

# --- 7. Baixar plugins (Lazy) e ferramentas (Mason) -----------------------
if [ "$CONFIG_MODE" = "manter" ]; then
  info "Config do Neovim mantida: pulei os plugins do Astrograph (rodariam na sua config)."
elif command -v nvim >/dev/null 2>&1; then
  info "Baixando plugins, ferramentas do Mason e compilando os realces de sintaxe (pode demorar alguns minutos)..."
  # O último passo compila os parsers do Treesitter (alguns segundos) para o
  # primeiro arquivo aberto já vir com o realce de sintaxe pronto.
  nvim --headless "+Lazy! restore" "+MasonToolsInstallSync" "+lua require('astrograph.treesitter').preinstall()" +qa 2>/dev/null || true

  # Confere que a config subiu com o tema certo (senão caiu no fallback)
  info "Verificando o tema..."
  theme_out="$(nvim --headless "+lua vim.wait(1500); io.write('ARTHEME='..(vim.g.colors_name or 'nil'))" +qa 2>&1 || true)"
  case "$theme_out" in
    *ARTHEME=*) theme_name="${theme_out##*ARTHEME=}"; theme_name="${theme_name%%[[:space:]]*}" ;;
    *) theme_name="" ;;
  esac
  if [ -z "$theme_name" ]; then theme_name="desconhecido"; fi
  missing_mod="$(printf '%s' "$theme_out" | sed -n "s/.*module '\([^']*\)' not found.*/\1/p" | head -n1)"
  if [ -n "$missing_mod" ]; then
    warn "O nvim abriu com erro: módulo '$missing_mod' não encontrado. A config ligada em $CONFIG está incompleta."
    warn "Confira com: git -C \"$ROOT\" status   (ou rode o instalador de novo com ASTROGRAPH_CONFIG_EXISTENTE=limpar)"
  elif printf '%s' "$theme_name" | grep -q astrograph; then
    info "Tema astrograph ativo ($theme_name)."
  else
    warn "Tema astrograph NÃO carregou (colorscheme='$theme_name'). Rode: nvim e :colorscheme astrograph"
  fi
else
  warn "nvim não encontrado no PATH. Abra um terminal novo e rode: nvim"
fi

# --- 8. Agentes (opcional, via checklist) ---------------------------------
AGENT_IDS=(claude opencode codex gemini ollama)
AGENT_BINS=(claude opencode codex gemini ollama)
AGENT_LABELS=("Claude Code" "opencode" "Codex CLI" "Gemini CLI" "Ollama")
AGENT_NPM=("@anthropic-ai/claude-code" "opencode-ai" "@openai/codex" "@google/gemini-cli" "")
AGENT_DETAIL=("npm @anthropic-ai/claude-code" "npm opencode-ai" "npm @openai/codex" "npm @google/gemini-cli" "modelo local (offline)")
AGENT_MODELS=(qwen2.5-coder:7b qwen3:8b llama3.1:8b)

TTY=""
if exec 3<>/dev/tty 2>/dev/null; then TTY=1; fi

SELECTED_IDS=()
ENV_MODEL=""

has_sel() { local x; for x in "${SELECTED_IDS[@]:-}"; do [ "$x" = "$1" ] && return 0; done; return 1; }

run_agent_menu() {
  local n=${#AGENT_IDS[@]} cursor=0 i key seq
  local selected=() installed=()
  for ((i = 0; i < n; i++)); do
    selected[$i]=0
    if command -v "${AGENT_BINS[$i]}" >/dev/null 2>&1; then installed[$i]=1; else installed[$i]=0; fi
  done
  draw() {
    for ((i = 0; i < n; i++)); do
      local mark='[ ]' ptr=' ' suffix=''
      { [ "${installed[$i]}" = 1 ] || [ "${selected[$i]}" = 1 ]; } && mark='[x]'
      [ "$i" = "$cursor" ] && ptr='>'
      [ "${installed[$i]}" = 1 ] && suffix='  (já instalado)'
      printf '\r  %s %s %-13s %s%s\033[K\n' "$ptr" "$mark" "${AGENT_LABELS[$i]}" "${AGENT_DETAIL[$i]}" "$suffix"
    done
  }
  printf '\n  Agentes a instalar  (↑/↓ navega · espaço marca · enter confirma · a=todos)\n'
  printf '  ------------------------------------------------------------\n'
  draw
  while true; do
    IFS= read -rsn1 -u 3 key || break
    case "$key" in
      $'\x1b')
        IFS= read -rsn2 -u 3 seq || seq=''
        case "$seq" in
          '[A') cursor=$(((cursor - 1 + n) % n)) ;;
          '[B') cursor=$(((cursor + 1) % n)) ;;
        esac
        ;;
      ' ')
        if [ "${installed[$cursor]}" = 0 ]; then
          if [ "${selected[$cursor]}" = 1 ]; then selected[$cursor]=0; else selected[$cursor]=1; fi
        fi
        ;;
      'a') for ((i = 0; i < n; i++)); do [ "${installed[$i]}" = 0 ] && selected[$i]=1; done ;;
      'q' | '') break ;;
    esac
    printf '\033[%dA' "$n"
    draw
  done
  SELECTED_IDS=()
  for ((i = 0; i < n; i++)); do
    { [ "${selected[$i]}" = 1 ] && [ "${installed[$i]}" = 0 ]; } && SELECTED_IDS+=("${AGENT_IDS[$i]}")
  done
}

if [ -n "${ASTROGRAPH_AGENTS:-}" ]; then
  IFS=',; ' read -ra toks <<<"$ASTROGRAPH_AGENTS"
  for t in "${toks[@]}"; do
    case "$t" in
      ollama:*) SELECTED_IDS+=(ollama); ENV_MODEL="${t#ollama:}" ;;
      *) SELECTED_IDS+=("$t") ;;
    esac
  done
elif [ -n "$TTY" ]; then
  run_agent_menu
else
  warn "Sem terminal interativo — pulando agentes (use ASTROGRAPH_AGENTS=... para automatizar)."
fi

if [ "${#SELECTED_IDS[@]}" -gt 0 ]; then
  need_npm=0
  for id in "${SELECTED_IDS[@]}"; do
    for ((i = 0; i < ${#AGENT_IDS[@]}; i++)); do
      if [ "${AGENT_IDS[$i]}" = "$id" ] && [ -n "${AGENT_NPM[$i]}" ]; then need_npm=1; fi
    done
  done
  if [ "$need_npm" = 1 ] && ! command -v node >/dev/null 2>&1; then
    info "Instalando Node.js (necessário para os CLIs npm)..."
    if [ "$OS" = "Darwin" ]; then brew install node 2>/dev/null || brew install node
    elif command -v apt-get >/dev/null 2>&1; then as_root apt-get install -y nodejs npm
    elif command -v dnf >/dev/null 2>&1; then as_root dnf install -y nodejs npm
    elif command -v pacman >/dev/null 2>&1; then as_root pacman -Sy --noconfirm --needed nodejs npm
    elif command -v zypper >/dev/null 2>&1; then as_root zypper install -y nodejs npm
    elif command -v apk >/dev/null 2>&1; then as_root apk add nodejs npm
    fi
  fi
  npm_install() { npm_global install -g "$1"; }
  for id in "${SELECTED_IDS[@]}"; do
    idx=-1
    for ((i = 0; i < ${#AGENT_IDS[@]}; i++)); do [ "${AGENT_IDS[$i]}" = "$id" ] && idx=$i; done
    [ "$idx" -lt 0 ] && continue
    bin="${AGENT_BINS[$idx]}"
    if command -v "$bin" >/dev/null 2>&1; then
      info "${AGENT_LABELS[$idx]} já instalado"
      continue
    fi
    if [ -n "${AGENT_NPM[$idx]}" ]; then
      info "Instalando ${AGENT_LABELS[$idx]} (npm ${AGENT_NPM[$idx]})..."
      npm_install "${AGENT_NPM[$idx]}" 2>/dev/null || warn "Falha ao instalar ${AGENT_LABELS[$idx]}"
    elif [ "$id" = "ollama" ]; then
      info "Instalando Ollama..."
      if [ "$OS" = "Darwin" ]; then brew install ollama 2>/dev/null || brew install ollama
      else curl -fsSL https://ollama.com/install.sh | sh || warn "Falha ao instalar Ollama"; fi
    fi
  done

  # Configuração: modelo local do Ollama
  if has_sel ollama && command -v ollama >/dev/null 2>&1; then
    model="${ENV_MODEL:-}"
    if [ -z "$model" ] && [ -z "${ASTROGRAPH_AGENTS:-}" ] && [ -n "$TTY" ]; then
      printf '\n  Modelo local do Ollama (enter = pular):\n'
      i=1
      for m in "${AGENT_MODELS[@]}"; do printf '  [%d] %s\n' "$i" "$m"; i=$((i + 1)); done
      printf '  [o] outro\n  escolha: '
      IFS= read -r -u 3 pick || pick=''
      if printf '%s' "$pick" | grep -qE '^[0-9]+$' && [ "$pick" -ge 1 ] && [ "$pick" -le "${#AGENT_MODELS[@]}" ]; then
        model="${AGENT_MODELS[$((pick - 1))]}"
      elif [ "$pick" = "o" ]; then
        printf '  nome do modelo: '
        IFS= read -r -u 3 model || model=''
      fi
    fi
    if [ -n "$model" ]; then
      info "Baixando o modelo $model ..."
      ollama pull "$model" || warn "Falha ao baixar $model"
      cfg="${XDG_CONFIG_HOME:-$HOME/.config}/astrograph"
      mkdir -p "$cfg"
      printf '{\n  "local_model": "%s"\n}\n' "$model" >"$cfg/config.json"
      info "Modelo local salvo: $model"
    fi
  fi

fi

# --- 9. Configurar os agentes instalados (ponte do chat + login) ----------
# Roda para TODOS os agentes no PATH, inclusive os que já existiam: o chat do
# editor (<Leader>Ac) precisa da ponte ACP, e cada CLI precisa de login.
# Sem terminal interativo, ou com ASTROGRAPH_NO_LOGIN=1, só mostra os comandos.
SETUP_IDS=(claude opencode codex gemini)
SETUP_LABELS=("Claude Code" "opencode" "Codex CLI" "Gemini CLI")
SETUP_LOGIN=("claude auth login" "opencode auth login" "codex login" "gemini")
SETUP_ACP_BIN=("claude-agent-acp" "" "codex-acp" "")
SETUP_ACP_NPM=("@agentclientprotocol/claude-agent-acp" "" "@agentclientprotocol/codex-acp" "")
SETUP_LEGACY=("@zed-industries/claude-agent-acp" "" "" "")

CAN_ASK=""
if [ -n "$TTY" ] && [ "${ASTROGRAPH_NO_LOGIN:-}" != "1" ]; then CAN_ASK=1; fi

# 0 = logado, 1 = sem login, 2 = não dá para saber
agent_login() {
  case "$1" in
    claude)
      [ -n "${ANTHROPIC_API_KEY:-}${CLAUDE_CODE_OAUTH_TOKEN:-}" ] && return 0
      out="$(claude auth status 2>&1 || true)"
      case "$out" in
        *'"loggedIn": true'* | *'"loggedIn":true'*) return 0 ;;
        *'"loggedIn": false'* | *'"loggedIn":false'*) return 1 ;;
      esac
      return 2
      ;;
    opencode)
      n="$(opencode auth list 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -oE '[0-9]+ credentials?' | grep -oE '^[0-9]+' | head -n1)"
      [ -z "$n" ] && return 2
      [ "$n" -gt 0 ] && return 0
      return 1
      ;;
    codex)
      [ -n "${OPENAI_API_KEY:-}" ] && return 0
      codex login status >/dev/null 2>&1 && return 0
      return 1
      ;;
    gemini)
      [ -n "${GEMINI_API_KEY:-}${GOOGLE_API_KEY:-}" ] && return 0
      [ -f "$HOME/.gemini/oauth_creds.json" ] && return 0
      return 1
      ;;
  esac
  return 2
}

PENDING=()
any_agent=""
for ((i = 0; i < ${#SETUP_IDS[@]}; i++)); do
  id="${SETUP_IDS[$i]}"
  command -v "$id" >/dev/null 2>&1 || continue
  if [ -z "$any_agent" ]; then
    printf '\n'
    info "Configurando os agentes de IA..."
    any_agent=1
  fi
  label="${SETUP_LABELS[$i]}"
  # 1) ponte ACP para o chat do editor
  # pacote antigo (renomeado) da ponte: troca pelo novo
  legacy_on=""
  if [ -n "${SETUP_LEGACY[$i]}" ] && [ -d "$(npm root -g 2>/dev/null)/${SETUP_LEGACY[$i]}" ]; then legacy_on=1; fi
  if [ -n "${SETUP_ACP_NPM[$i]}" ] && { [ -n "$legacy_on" ] || ! command -v "${SETUP_ACP_BIN[$i]}" >/dev/null 2>&1; }; then
    info "$label: instalando a ponte do chat (${SETUP_ACP_NPM[$i]})..."
    if [ -n "${SETUP_LEGACY[$i]}" ]; then npm_global uninstall -g "${SETUP_LEGACY[$i]}" >/dev/null 2>&1 || true; fi
    if npm_global install -g "${SETUP_ACP_NPM[$i]}" >/dev/null 2>&1; then
      info "$label: ponte do chat instalada"
    else
      warn "$label: falha ao instalar ${SETUP_ACP_NPM[$i]} (rode depois: npm install -g ${SETUP_ACP_NPM[$i]})"
    fi
  fi
  # 2) login
  agent_login "$id" && st=0 || st=$?
  if [ "$st" = 0 ]; then
    info "$label: login ok"
    continue
  fi
  if [ -n "$CAN_ASK" ]; then
    printf '  %s está sem login. Entrar agora? (abre o navegador) [S/n] ' "$label"
    IFS= read -r -u 3 ans || ans="n"
    case "$ans" in
      "" | s | S | y | Y)
        [ "$id" = "gemini" ] && info "O Gemini abre a tela de login; depois de entrar, saia com /quit."
        ${SETUP_LOGIN[$i]} <&3 || true
        agent_login "$id" && st=0 || st=$?
        if [ "$st" = 0 ]; then info "$label: login ok"; continue; fi
        ;;
    esac
  fi
  PENDING+=("$i")
done

if [ -n "$any_agent" ]; then
  if [ "${#PENDING[@]}" -gt 0 ]; then
    printf '\n'
    info "Falta entrar nestes agentes (rode num terminal, ou no nvim: :Astrograph login):"
    for i in "${PENDING[@]}"; do printf '  %-20s -> %s\n' "${SETUP_LOGIN[$i]}" "${SETUP_LABELS[$i]}"; done
  fi
  if has_sel ollama; then printf '  ollama serve         -> se o Ollama não estiver no ar\n'; fi
  printf '\n'
  info "No nvim: <Leader>Al abre o agente no terminal, <Leader>Ac abre o chat, :Astrograph agentes mostra o status."
fi

info "Pronto! Abra um terminal novo e rode: nvim"
