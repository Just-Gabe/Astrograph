# Astrograph — instalador para Windows (uma linha)
#
#   irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
#
# O que ele faz:
#   1. Instala (via winget): Neovim, Git, ripgrep, fd e o gcc do WinLibs
#   2. Instala a Nerd Font JetBrainsMono (por usuário, sem admin) + registra no Windows
#   3. (Opcional) aponta a fonte do Windows Terminal para a Nerd Font
#   4. Clona o Astrograph em %LOCALAPPDATA%\Astrograph
#   5. Liga a config: %LOCALAPPDATA%\nvim  ->  Astrograph\template (com backup do antigo)
#   6. Baixa os plugins do Neovim (Lazy) + ferramentas do Mason
#
# É idempotente: pode rodar de novo sem medo.

#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$Repo   = 'https://github.com/Just-Gabe/Astrograph.git'
$Root   = Join-Path $env:LOCALAPPDATA 'Astrograph'
$Config = Join-Path $env:LOCALAPPDATA 'nvim'
$FontUrl = 'https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.zip'

function Info($m) { Write-Host "[astrograph] $m" -ForegroundColor Cyan }
function Warn($m) { Write-Host "[astrograph] $m" -ForegroundColor Yellow }

# --- 0. Pré-requisitos -----------------------------------------------------
if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
  throw 'winget não encontrado. Instale o "App Installer" pela Microsoft Store e rode o script de novo.'
}

# --- 1. Pacotes (winget) ---------------------------------------------------
$packages = @(
  'Neovim.Neovim',              # editor
  'Git.Git',                    # obrigatório
  'BurntSushi.ripgrep.MSVC',    # live grep (<Leader>fw)
  'sharkdp.fd',                 # busca de arquivos (<Leader>ff)
  'BrechtSanders.WinLibs.POSIX.UCRT' # gcc para os parsers do Treesitter
)
foreach ($pkg in $packages) {
  Info "winget: $pkg"
  winget install --id $pkg -e --silent `
    --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
}

# Atualiza o PATH da sessão para já usar nvim/git/rg/fd/gcc
$env:Path = ([Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
             [Environment]::GetEnvironmentVariable('Path', 'User'))

# --- 2. Nerd Font (por usuário, sem admin) ---------------------------------
$fontDir = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Fonts'
$fontKey = 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts'
New-Item -ItemType Directory -Force -Path $fontDir | Out-Null
New-Item -Path $fontKey -Force | Out-Null

if (Test-Path (Join-Path $fontDir 'JetBrainsMonoNerdFont-Regular.ttf')) {
  Info 'Nerd Font JetBrainsMono já instalada'
} else {
  Info 'Baixando a Nerd Font JetBrainsMono...'
  $zip = Join-Path $env:TEMP 'JetBrainsMonoNF.zip'
  $tmp = Join-Path $env:TEMP 'JetBrainsMonoNF'
  Invoke-WebRequest -Uri $FontUrl -OutFile $zip
  if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
  Expand-Archive -Path $zip -DestinationPath $tmp -Force
  Get-ChildItem $tmp -Recurse -Include '*.ttf','*.otf' | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $fontDir $_.Name) -Force
    $face = [IO.Path]::GetFileNameWithoutExtension($_.Name)
    New-ItemProperty -Path $fontKey -Name "$face (TrueType)" -Value $_.Name -PropertyType String -Force | Out-Null
  }
  Info 'Nerd Font instalada e registrada'
}

# --- 3. Windows Terminal: usar a Nerd Font ---------------------------------
$wtSettings = Join-Path $env:LOCALAPPDATA 'Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json'
if (Test-Path $wtSettings) {
  try {
    $json = Get-Content $wtSettings -Raw | ConvertFrom-Json
    if ($json.profiles -and $json.profiles.defaults) {
      if (-not $json.profiles.defaults.font) {
        $font = [pscustomobject]@{ face = 'JetBrainsMono Nerd Font' }
        if ($json.profiles.defaults.PSObject.Properties['font']) {
          $json.profiles.defaults.font = $font
        } else {
          $json.profiles.defaults | Add-Member -NotePropertyName font -NotePropertyValue $font
        }
        $text = $json | ConvertTo-Json -Depth 100
        [IO.File]::WriteAllText($wtSettings, $text, (New-Object System.Text.UTF8Encoding($false)))
        Info 'Windows Terminal: fonte padrão definida para JetBrainsMono Nerd Font'
      }
    }
  } catch {
    Warn "Não consegui ajustar o Windows Terminal ($($_.Exception.Message)). Ajuste a fonte manualmente."
  }
} else {
  Warn 'Windows Terminal não encontrado — configure a Nerd Font no seu terminal manualmente.'
}

# --- 4. Clonar/atualizar o Astrograph --------------------------------------
if (Test-Path (Join-Path $Root '.git')) {
  Info "Atualizando $Root"
  git -C $Root pull --ff-only
} else {
  Info "Clonando o Astrograph em $Root"
  git clone --depth 1 $Repo $Root
}

# --- 5. Ligar a config -----------------------------------------------------
if (Test-Path $Config) {
  $item = Get-Item $Config -Force
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    Remove-Item $Config -Force
  } else {
    $backup = "$Config.backup-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    Move-Item $Config $backup
    Info "Config anterior movida para $backup"
  }
}
cmd /c mklink /J "$Config" (Join-Path $Root 'template') | Out-Null
Info "Ligado: $Config -> $Root\template"

# --- 6. Baixar plugins (Lazy) e ferramentas (Mason) ------------------------
if (Get-Command nvim -ErrorAction SilentlyContinue) {
  Info 'Baixando plugins e ferramentas (pode demorar alguns minutos)...'
  nvim --headless "+Lazy! restore" "+MasonToolsInstallSync" +qa 2>$null
} else {
  Warn 'nvim não está no PATH desta sessão. Abra um terminal novo e rode: nvim'
}

Info 'Pronto! Abra um terminal novo e rode: nvim'
