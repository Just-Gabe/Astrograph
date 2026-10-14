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

# --- 7. Agentes (opcional, via checklist) ----------------------------------
$agentDefs = @(
  [pscustomobject]@{ Id = 'claude';   Bin = 'claude';   Label = 'Claude Code'; Detail = 'npm @anthropic-ai/claude-code'; Npm = '@anthropic-ai/claude-code' },
  [pscustomobject]@{ Id = 'opencode'; Bin = 'opencode'; Label = 'opencode';    Detail = 'npm opencode-ai';              Npm = 'opencode-ai' },
  [pscustomobject]@{ Id = 'codex';    Bin = 'codex';    Label = 'Codex CLI';   Detail = 'npm @openai/codex';            Npm = '@openai/codex' },
  [pscustomobject]@{ Id = 'gemini';   Bin = 'gemini';   Label = 'Gemini CLI';  Detail = 'npm @google/gemini-cli';       Npm = '@google/gemini-cli' },
  [pscustomobject]@{ Id = 'ollama';   Bin = 'ollama';   Label = 'Ollama';      Detail = 'modelo local (offline)';       Npm = $null }
)
$agentModels = @('qwen2.5-coder:7b', 'qwen3:8b', 'llama3.1:8b')

function Test-Bin($b) { [bool](Get-Command $b -ErrorAction SilentlyContinue) }
function Refresh-Path {
  $env:Path = ([Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
               [Environment]::GetEnvironmentVariable('Path', 'User'))
}

# Checklist TUI: ↑/↓ navega · espaço marca · enter confirma · a=todos · q/esc cancela
function Read-AgentMenu {
  param([object[]]$Items)
  $n = $Items.Count
  $checked = New-Object 'bool[]' $n
  $cursor = 0
  Write-Host ''
  Write-Host '  Agentes a instalar  (↑/↓ navega · espaço marca · enter confirma · a=todos)' -ForegroundColor Cyan
  Write-Host '  ' + ('-' * 60)
  foreach ($it in $Items) { Write-Host '' }
  $top = [Console]::CursorTop - $n
  $width = 79
  try { $width = [Console]::WindowWidth - 1 } catch {}
  function Draw {
    for ($i = 0; $i -lt $n; $i++) {
      $it = $Items[$i]
      $mark = if ($it.Installed -or $checked[$i]) { '[x]' } else { '[ ]' }
      $ptr = if ($i -eq $cursor) { '>' } else { ' ' }
      $suffix = if ($it.Installed) { '  (já instalado)' } else { '' }
      $line = '  {0} {1} {2,-13} {3}{4}' -f $ptr, $mark, $it.Label, $it.Detail, $suffix
      [Console]::SetCursorPosition(0, $top + $i)
      Write-Host $line.PadRight($width)
    }
  }
  Draw
  while ($true) {
    $key = [Console]::ReadKey($true)
    switch ($key.Key) {
      'UpArrow' { $cursor = ($cursor - 1 + $n) % $n }
      'DownArrow' { $cursor = ($cursor + 1) % $n }
      'Spacebar' { if (-not $Items[$cursor].Installed) { $checked[$cursor] = -not $checked[$cursor] } }
      'Enter' { return $checked }
      'Escape' { return (New-Object 'bool[]' $n) }
      default {
        if ($key.KeyChar -eq 'a') { for ($i = 0; $i -lt $n; $i++) { if (-not $Items[$i].Installed) { $checked[$i] = $true } } }
        if ($key.KeyChar -eq 'q') { return (New-Object 'bool[]' $n) }
      }
    }
    Draw
  }
}

$selectedIds = @()
$envModel = $null
if ($env:ASTROGRAPH_AGENTS) {
  foreach ($tok in ($env:ASTROGRAPH_AGENTS -split '[,;\s]+' | Where-Object { $_ })) {
    if ($tok -like 'ollama:*') { $selectedIds += 'ollama'; $envModel = $tok.Substring(7) }
    else { $selectedIds += $tok }
  }
} else {
  $interactive = -not [Console]::IsInputRedirected
  if ($interactive) { try { $null = [Console]::KeyAvailable } catch { $interactive = $false } }
  if ($interactive) {
    $status = foreach ($d in $agentDefs) { $d | Add-Member -NotePropertyName Installed -NotePropertyValue (Test-Bin $d.Bin) -PassThru }
    try {
      $checked = Read-AgentMenu $status
      for ($i = 0; $i -lt $status.Count; $i++) { if ($checked[$i]) { $selectedIds += $status[$i].Id } }
    } catch {
      Warn "Menu indisponível ($($_.Exception.Message)); pulando agentes."
    }
  } else {
    Info 'Sem terminal interativo — pulando agentes (use ASTROGRAPH_AGENTS para automatizar).'
  }
}

if ($selectedIds.Count -gt 0) {
  $picked = $agentDefs | Where-Object { $selectedIds -contains $_.Id }

  if (($picked | Where-Object { $_.Npm }) -and -not (Get-Command node -ErrorAction SilentlyContinue)) {
    Info 'Instalando Node.js LTS (necessário para os CLIs npm)...'
    winget install --id OpenJS.NodeJS.LTS -e --silent `
      --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
    Refresh-Path
  }

  foreach ($d in $picked) {
    if (Test-Bin $d.Bin) { Info "$($d.Label) já instalado"; continue }
    if ($d.Npm) {
      Info "Instalando $($d.Label) (npm $($d.Npm))..."
      try { npm install -g $d.Npm 2>$null | Out-Null; Info "$($d.Label) instalado" }
      catch { Warn "Falha ao instalar $($d.Label): $($_.Exception.Message)" }
    } elseif ($d.Id -eq 'ollama') {
      Info 'Instalando Ollama (winget)...'
      try {
        winget install --id Ollama.Ollama -e --silent `
          --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
      } catch { Warn "Falha ao instalar Ollama: $($_.Exception.Message)" }
    }
  }
  Refresh-Path

  # Configuração: modelo local do Ollama
  if (($selectedIds -contains 'ollama') -and (Test-Bin 'ollama')) {
    $model = $envModel
    if (-not $model -and -not $env:ASTROGRAPH_AGENTS -and $interactive) {
      Write-Host ''
      Info 'Modelo local do Ollama (enter = pular):'
      for ($i = 0; $i -lt $agentModels.Count; $i++) { Write-Host ("  [{0}] {1}" -f ($i + 1), $agentModels[$i]) }
      Write-Host '  [o] outro (digitar)'
      $pick = Read-Host '  escolha'
      if ($pick -match '^\d+$' -and [int]$pick -ge 1 -and [int]$pick -le $agentModels.Count) {
        $model = $agentModels[[int]$pick - 1]
      } elseif ($pick -eq 'o') {
        $model = (Read-Host '  nome do modelo (ex.: qwen2.5-coder:14b)').Trim()
      }
    }
    if ($model) {
      Info "Baixando o modelo $model ..."
      try { ollama pull $model } catch { Warn "Falha ao baixar $model : $($_.Exception.Message)" }
      $cfgDir = Join-Path $env:LOCALAPPDATA 'astrograph'
      New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
      [IO.File]::WriteAllText((Join-Path $cfgDir 'config.json'),
        (@{ local_model = $model } | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))
      Info "Modelo local salvo: $model"
    }
  }

  Write-Host ''
  Info 'Próximos passos (login dos agentes):'
  if ($selectedIds -contains 'claude') { Write-Host '  claude            -> autentica o Claude Code' }
  if ($selectedIds -contains 'opencode') { Write-Host '  opencode auth login' }
  if ($selectedIds -contains 'codex') { Write-Host '  codex             -> autentica o Codex' }
  if ($selectedIds -contains 'gemini') { Write-Host '  gemini            -> autentica o Gemini CLI' }
  if ($selectedIds -contains 'ollama') { Write-Host '  ollama serve      -> se o serviço não estiver no ar' }
}

Info 'Pronto! Abra um terminal novo e rode: nvim'
