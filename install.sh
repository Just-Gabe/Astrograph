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
else
  warn "nvim não encontrado no PATH. Abra um terminal novo e rode: nvim"
fi

info "Pronto! Abra um terminal novo e rode: nvim"
