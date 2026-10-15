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
  # o `:Lazy update` reescreve o lazy-lock.json versionado; sem descartar essa
  # alteração o pull falha e a instalação continua velha sem avisar
  git -C $Root checkout -- template/lazy-lock.json
  git -C $Root pull --ff-only
  if ($LASTEXITCODE -ne 0) { Warn "Não consegui atualizar $Root (alterações locais?). Rode: git -C `"$Root`" status" }
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
  # No PowerShell 5.1 um comando nativo que escreve em stderr aborta o script
  # quando $ErrorActionPreference='Stop' (o nvim/treesitter escreve bastante).
  # Isola as chamadas nativas para o instalador não parar no meio.
  $prevEap = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  nvim --headless "+Lazy! restore" "+MasonToolsInstallSync" +qa 2>&1 | Out-Null

  # Confere que a config subiu com o tema certo (senão caiu no fallback)
  Info 'Verificando o tema...'
  $themeOut = ((nvim --headless "+lua vim.wait(1500); io.write('ARTHEME='..(vim.g.colors_name or 'nil'))" +qa 2>&1) -join '')
  $ErrorActionPreference = $prevEap
  $themeName = if ($themeOut -match 'ARTHEME=(\S+)') { $Matches[1] } else { 'desconhecido' }
  if ($themeName -match 'astrograph') {
    Info "Tema astrograph ativo ($themeName)."
  } else {
    Warn "Tema astrograph NÃO carregou (colorscheme='$themeName'). Abra o nvim e rode: :colorscheme astrograph"
  }
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
  Write-Host ('  ' + ('-' * 60))
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
      # PS 5.1: com 'Stop', qualquer aviso do npm no stderr redirecionado vira
      # exceção e interrompe a instalação no meio. Usa o código de saída.
      $prevEap = $ErrorActionPreference
      $ErrorActionPreference = 'Continue'
      npm install -g $d.Npm 2>&1 | Out-Null
      $npmCode = $LASTEXITCODE
      $ErrorActionPreference = $prevEap
      if ($npmCode -eq 0) { Info "$($d.Label) instalado" }
      else { Warn "Falha ao instalar $($d.Label) (npm saiu com código $npmCode)" }
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
      # %APPDATA% (Roaming): %LOCALAPPDATA%/astrograph seria a pasta do próprio clone
      $cfgDir = Join-Path $env:APPDATA 'astrograph'
      New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
      [IO.File]::WriteAllText((Join-Path $cfgDir 'config.json'),
        (@{ local_model = $model } | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))
      Info "Modelo local salvo: $model"
    }
  }

}

# --- 8. Configurar os agentes instalados (ponte do chat + login) -----------
# Roda para TODOS os agentes no PATH, inclusive os que já existiam: o chat do
# editor (<Leader>Ac) precisa da ponte ACP, e cada CLI precisa de login.
# Sem terminal interativo, ou com ASTROGRAPH_NO_LOGIN=1, só mostra os comandos.
$agentSetup = @(
  [pscustomobject]@{ Id = 'claude';   Label = 'Claude Code'; Login = 'claude auth login';   AcpBin = 'claude-agent-acp'; AcpNpm = '@agentclientprotocol/claude-agent-acp'; Legacy = '@zed-industries/claude-agent-acp' },
  [pscustomobject]@{ Id = 'opencode'; Label = 'opencode';    Login = 'opencode auth login'; AcpBin = $null;              AcpNpm = $null;                                   Legacy = $null },
  [pscustomobject]@{ Id = 'codex';    Label = 'Codex CLI';   Login = 'codex login';         AcpBin = 'codex-acp';        AcpNpm = '@agentclientprotocol/codex-acp';        Legacy = $null },
  [pscustomobject]@{ Id = 'gemini';   Label = 'Gemini CLI';  Login = 'gemini';              AcpBin = $null;              AcpNpm = $null;                                   Legacy = $null }
)

$canAsk = (-not [Console]::IsInputRedirected) -and ($env:ASTROGRAPH_NO_LOGIN -ne '1')
if ($canAsk) { try { $null = [Console]::KeyAvailable } catch { $canAsk = $false } }

function Test-AgentLogin($id) {
  if (-not (Test-Bin $id)) { return $false }
  switch ($id) {
    'claude' {
      if ($env:ANTHROPIC_API_KEY -or $env:CLAUDE_CODE_OAUTH_TOKEN) { return $true }
      $raw = (claude auth status 2>&1 | Out-String)
      if ($raw -match '"loggedIn"\s*:\s*true') { return $true }
      if ($raw -match '"loggedIn"\s*:\s*false') { return $false }
      return $null
    }
    'opencode' {
      $raw = (opencode auth list 2>&1 | Out-String) -replace ([string][char]27 + '\[[\d;]*m'), ''
      if ($raw -match '(\d+)\s+credentials?') { return ([int]$Matches[1] -gt 0) }
      return $null
    }
    'codex' {
      if ($env:OPENAI_API_KEY) { return $true }
      codex login status 2>&1 | Out-Null
      return ($LASTEXITCODE -eq 0)
    }
    'gemini' {
      if ($env:GEMINI_API_KEY -or $env:GOOGLE_API_KEY) { return $true }
      return (Test-Path (Join-Path $env:USERPROFILE '.gemini\oauth_creds.json'))
    }
  }
  return $null
}

function Ask-YesNo($question) {
  $answer = Read-Host "  $question [S/n]"
  return ($answer -eq '' -or $answer -match '^[sSyY]')
}

$present = $agentSetup | Where-Object { Test-Bin $_.Id }
if ($present) {
  Write-Host ''
  Info 'Configurando os agentes de IA...'
  # stderr dos CLIs (avisos do npm, barras de progresso) não pode abortar o script
  $prevEap = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  $pending = @()
  foreach ($a in $present) {
    # 1) ponte ACP para o chat do editor
    # pacote antigo (renomeado) da ponte: troca pelo novo
    $legacyOn = $a.Legacy -and (Test-Path (Join-Path $env:APPDATA "npm\node_modules\$($a.Legacy)"))
    if ($a.AcpNpm -and ((-not (Test-Bin $a.AcpBin)) -or $legacyOn)) {
      Info "$($a.Label): instalando a ponte do chat ($($a.AcpNpm))..."
      if ($a.Legacy) { npm uninstall -g $a.Legacy 2>&1 | Out-Null }
      npm install -g $a.AcpNpm 2>&1 | Out-Null
      if ($LASTEXITCODE -eq 0) { Info "$($a.Label): ponte do chat instalada" }
      else { Warn "$($a.Label): falha ao instalar $($a.AcpNpm) (rode depois: npm install -g $($a.AcpNpm))" }
    }
    # 2) login
    $logged = Test-AgentLogin $a.Id
    if ($logged -eq $true) {
      Info "$($a.Label): login ok"
    } elseif ($canAsk -and (Ask-YesNo "$($a.Label) está sem login. Entrar agora? (abre o navegador)")) {
      if ($a.Id -eq 'gemini') { Info 'O Gemini abre a tela de login; depois de entrar, saia com /quit.' }
      Invoke-Expression $a.Login
      if ((Test-AgentLogin $a.Id) -eq $true) { Info "$($a.Label): login ok" }
      else { $pending += $a }
    } else {
      $pending += $a
    }
  }
  $ErrorActionPreference = $prevEap
  Refresh-Path

  if ($pending.Count -gt 0) {
    Write-Host ''
    Info 'Falta entrar nestes agentes (rode num terminal, ou no nvim: :Astrograph login):'
    foreach ($a in $pending) { Write-Host ("  {0,-20} -> {1}" -f $a.Login, $a.Label) }
  }
  if ($selectedIds -contains 'ollama') { Write-Host '  ollama serve         -> se o Ollama não estiver no ar' }
  Write-Host ''
  Info 'No nvim: <Leader>Al abre o agente no terminal, <Leader>Ac abre o chat, :Astrograph agentes mostra o status.'
}

Info 'Pronto! Abra um terminal novo e rode: nvim'
