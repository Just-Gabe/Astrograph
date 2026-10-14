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
#   5. Liga a config: ~/.config/nvim  ->  ~/Astrograph/template (com backup do antigo)
#   6. Baixa os plugins do Neovim (Lazy) + ferramentas do Mason
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

OS="$(uname -s)"
ARCH="$(uname -m)"

# --- 1. Dependências do sistema -------------------------------------------
info "Sistema: $OS ($ARCH)"
if [ "$OS" = "Darwin" ]; then
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
else
  warn "Sistema não suportado: $OS"; exit 1
fi

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
  git -C "$ROOT" pull --ff-only || true
else
  info "Clonando o Astrograph em $ROOT"
  git clone --depth 1 "$REPO" "$ROOT"
fi

# --- 5. Ligar a config -----------------------------------------------------
mkdir -p "$(dirname "$CONFIG")"
if [ -e "$CONFIG" ] && [ ! -L "$CONFIG" ]; then
  backup="$CONFIG.backup-$(date +%Y%m%d-%H%M%S)"
  mv "$CONFIG" "$backup"
  info "Config anterior movida para $backup"
fi
ln -sfn "$ROOT/template" "$CONFIG"
info "Ligado: $CONFIG -> $ROOT/template"

# --- 6. Baixar plugins (Lazy) e ferramentas (Mason) -----------------------
if command -v nvim >/dev/null 2>&1; then
  info "Baixando plugins e ferramentas (pode demorar alguns minutos)..."
  nvim --headless "+Lazy! restore" "+MasonToolsInstallSync" +qa 2>/dev/null || true

  # Confere que a config subiu com o tema certo (senão caiu no fallback)
  info "Verificando o tema..."
  theme_out="$(nvim --headless "+lua vim.wait(1500); io.write('ARTHEME='..(vim.g.colors_name or 'nil'))" +qa 2>/dev/null || true)"
  case "$theme_out" in
    *ARTHEME=*) theme_name="${theme_out##*ARTHEME=}"; theme_name="${theme_name%%[[:space:]]*}" ;;
    *) theme_name="" ;;
  esac
  if [ -z "$theme_name" ]; then theme_name="desconhecido"; fi
  if printf '%s' "$theme_name" | grep -q astrograph; then
    info "Tema astrograph ativo ($theme_name)."
  else
    warn "Tema astrograph NÃO carregou (colorscheme='$theme_name'). Rode: nvim e :colorscheme astrograph"
  fi
else
  warn "nvim não encontrado no PATH. Abra um terminal novo e rode: nvim"
fi

# --- 7. Agentes (opcional, via checklist) ---------------------------------
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
  npm_install() {
    if [ "$OS" = "Darwin" ] || [ "$(id -u)" -eq 0 ]; then npm install -g "$1"; else as_root npm install -g "$1"; fi
  }
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

  printf '\n'
  info "Próximos passos (login dos agentes):"
  if has_sel claude; then printf '  claude            -> autentica o Claude Code\n'; fi
  if has_sel opencode; then printf '  opencode auth login\n'; fi
  if has_sel codex; then printf '  codex             -> autentica o Codex\n'; fi
  if has_sel gemini; then printf '  gemini            -> autentica o Gemini CLI\n'; fi
  if has_sel ollama; then printf '  ollama serve      -> se o serviço não estiver no ar\n'; fi
fi

info "Pronto! Abra um terminal novo e rode: nvim"
