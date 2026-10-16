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
#   6. Instala o app Astrograph (UI em Rust): atalho no menu Iniciar e o comando `astrograph`
#   7. Baixa os plugins do Neovim (Lazy) + ferramentas do Mason
#
# É idempotente: o que já existe na máquina (programas, fonte, clone) não é
# baixado de novo; rodar de novo só atualiza.
#
# Arquivo salvo SEM BOM: com `irm | iex`, o BOM gruda no `#` da 1ª linha e o
# PowerShell tenta executar o comentário como comando.

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

# --- 1. Pacotes (winget): só o que falta ------------------------------------
function Refresh-Path {
  $env:Path = ([Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
               [Environment]::GetEnvironmentVariable('Path', 'User'))
}

# Versão do Neovim instalado (0.0 se não houver)
function Nvim-Version {
  $cmd = Get-Command nvim -ErrorAction SilentlyContinue
  if (-not $cmd) { return [version]'0.0' }
  $line = (& $cmd.Source --version 2>$null | Select-Object -First 1)
  if ($line -match 'v(\d+)\.(\d+)\.?(\d*)') {
    $patch = if ($Matches[3]) { $Matches[3] } else { '0' }
    return [version]"$($Matches[1]).$($Matches[2]).$patch"
  }
  return [version]'0.0'
}

$packages = @(
  @{ id = 'Neovim.Neovim';                    cmd = 'nvim'; why = 'editor' },
  @{ id = 'Git.Git';                          cmd = 'git';  why = 'obrigatório' },
  @{ id = 'BurntSushi.ripgrep.MSVC';          cmd = 'rg';   why = 'busca de texto' },
  @{ id = 'sharkdp.fd';                       cmd = 'fd';   why = 'busca de arquivos' },
  @{ id = 'BrechtSanders.WinLibs.POSIX.UCRT'; cmd = 'gcc';  why = 'parsers do Treesitter' }
)
Refresh-Path
foreach ($pkg in $packages) {
  $have = Get-Command $pkg.cmd -ErrorAction SilentlyContinue
  if ($pkg.cmd -eq 'nvim' -and $have -and (Nvim-Version) -lt [version]'0.11') {
    Info "Neovim $(Nvim-Version) é antigo (precisa de 0.11+): atualizando"
    winget upgrade --id $pkg.id -e --silent `
      --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
    continue
  }
  if ($have) {
    Info "$($pkg.cmd) já instalado ($($pkg.why))"
    continue
  }
  Info "Instalando $($pkg.id) ($($pkg.why))"
  winget install --id $pkg.id -e --silent `
    --accept-package-agreements --accept-source-agreements --disable-interactivity | Out-Null
}

# Atualiza o PATH da sessão para já usar nvim/git/rg/fd/gcc
Refresh-Path

# --- 2. Nerd Font (por usuário, sem admin) ---------------------------------
$fontDir = Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Fonts'
$fontKey = 'HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Fonts'
New-Item -ItemType Directory -Force -Path $fontDir | Out-Null
New-Item -Path $fontKey -Force | Out-Null

# Já instalada (para o usuário ou para todos, em qualquer variante: Mono, NL…)?
function Has-NerdFont {
  foreach ($dir in @($fontDir, (Join-Path $env:WINDIR 'Fonts'))) {
    if (Get-ChildItem $dir -Filter 'JetBrainsMono*NerdFont*-Regular.ttf' -ErrorAction SilentlyContinue | Select-Object -First 1) {
      return $true
    }
  }
  foreach ($key in @($fontKey, 'HKLM:\Software\Microsoft\Windows NT\CurrentVersion\Fonts')) {
    $props = Get-ItemProperty $key -ErrorAction SilentlyContinue
    if ($props -and ($props.PSObject.Properties.Name -match 'JetBrainsMono.*Nerd Font')) { return $true }
  }
  return $false
}

if (Has-NerdFont) {
  Info 'Nerd Font JetBrainsMono já instalada'
} else {
  Info 'Baixando a Nerd Font JetBrainsMono...'
  $zip = Join-Path $env:TEMP 'JetBrainsMonoNF.zip'
  $tmp = Join-Path $env:TEMP 'JetBrainsMonoNF'
  Invoke-WebRequest -Uri $FontUrl -OutFile $zip
  if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force }
  Expand-Archive -Path $zip -DestinationPath $tmp -Force
  Get-ChildItem $tmp -Recurse -Include '*.ttf','*.otf' | ForEach-Object {
    $dest = Join-Path $fontDir $_.Name
    # uma fonte em uso não pode ser sobrescrita ("seção mapeada"): se o arquivo
    # já existe, fica o que está lá
    if (-not (Test-Path $dest)) {
      try { Copy-Item $_.FullName $dest -Force } catch { Warn "Fonte $($_.Exception.Message)"; return }
    }
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
$appOpen = Get-Process -Name 'astrograph' -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($Root, [StringComparison]::OrdinalIgnoreCase) }
if ($appOpen) {
  # o app aberto segura arquivos da config; o pull trocaria só parte deles e a
  # config ficaria misturada (ex.: "module 'polish' not found"). Não fecha à
  # força: pode haver arquivo sem salvar.
  Warn 'O Astrograph está aberto: pulei a atualização. Feche o app e rode o instalador de novo.'
} elseif (Test-Path (Join-Path $Root '.git')) {
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
$Template = Join-Path $Root 'template'
$linked = $false
if (Test-Path $Config) {
  $item = Get-Item $Config -Force
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    if ((@($item.Target) -join '').TrimEnd('\') -eq $Template.TrimEnd('\')) {
      $linked = $true
    } else {
      # só o link: o Remove-Item numa junction pergunta "tem filhos, apagar?" e,
      # confirmado, apagaria o conteúdo do destino
      [IO.Directory]::Delete($Config)
    }
  } else {
    $backup = "$Config.backup-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    Move-Item $Config $backup
    Info "Config anterior movida para $backup"
  }
}
if ($linked) {
  Info "Config já ligada: $Config -> $Template"
} else {
  cmd /c mklink /J "$Config" $Template | Out-Null
  Info "Ligado: $Config -> $Template"
}

# --- 5b. App Astrograph (UI em Rust) ---------------------------------------
# O Astrograph é a janela própria (Rust) com o Neovim como motor. Baixa o
# executável do GitHub Releases só quando sai uma versão nova; sem release
# publicado, compila a partir do código se houver Rust (cargo) na máquina.
$BinDir   = Join-Path $Root 'bin'
$AppExe   = Join-Path $BinDir 'astrograph.exe'
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

# o rustup instala em ~\.cargo\bin, mas nem sempre põe a pasta no PATH
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
  if (Test-Path (Join-Path $cargoBin 'cargo.exe')) { $env:Path = "$cargoBin;$env:Path" }
}
$hasCargo = [bool](Get-Command cargo -ErrorAction SilentlyContinue)

$release = $null
try {
  $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/Just-Gabe/Astrograph/releases/latest' `
    -Headers @{ 'User-Agent' = 'astrograph-installer' } -TimeoutSec 15
} catch { $release = $null }
$commit = (git -C $Root rev-parse --short HEAD 2>$null)

# Hashes publicados no release (SHA256SUMS.txt): todo executável baixado é
# conferido; sem hash ou hash diferente, o download é recusado.
$sums = @{}
if ($release) {
  $sumsAsset = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' } | Select-Object -First 1
  if ($sumsAsset) {
    try {
      $text = (Invoke-WebRequest -Uri $sumsAsset.browser_download_url -UseBasicParsing -TimeoutSec 15).Content
      if ($text -is [byte[]]) { $text = [Text.Encoding]::ASCII.GetString($text) }
      foreach ($line in ($text -split "`r?`n")) {
        if ($line -match '^([0-9a-fA-F]{64})\s+\*?(\S+)$') { $sums[$Matches[2]] = $Matches[1].ToLower() }
      }
    } catch { }
  }
}

# Instala um executável do Astrograph: baixa do release quando a versão mudou
# (conferindo o SHA256); sem release válido, compila do código se houver Rust.
function Install-Binary($label, $exeName, $assetName, $crate, $builtName) {
  $exe   = Join-Path $BinDir $exeName
  $stamp = Join-Path $BinDir ([IO.Path]::ChangeExtension($exeName, '.version'))
  $installed = if (Test-Path $stamp) { (Get-Content $stamp -Raw).Trim() } else { '' }
  $place = {
    param($src)
    try { Copy-Item $src $exe -Force; $true }
    catch { Warn "Não consegui trocar o $label (está aberto?). Feche o Astrograph e rode o instalador de novo."; $false }
  }
  $asset = if ($release) { $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1 } else { $null }
  if ($asset -and -not $sums[$assetName]) {
    Warn "O release $($release.tag_name) não traz o hash do $label (SHA256SUMS.txt): não vou baixar sem conferir."
    $asset = $null
  }
  if ($asset) {
    if ((Test-Path $exe) -and $installed -eq $release.tag_name) { Info "$label já está na versão $($release.tag_name)"; return }
    Info "Baixando o $label $($release.tag_name)..."
    $tmp = Join-Path $env:TEMP "astrograph-download-$exeName"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tmp -UseBasicParsing
    $hash = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLower()
    if ($hash -ne $sums[$assetName]) {
      Warn "O $label baixado não confere com o SHA256 publicado: descartado."
      Remove-Item $tmp -ErrorAction SilentlyContinue
      return
    }
    if (& $place $tmp) { Set-Content $stamp $release.tag_name; Info "$label instalado (SHA256 conferido)" }
    Remove-Item $tmp -ErrorAction SilentlyContinue
  } elseif ($hasCargo) {
    if ((Test-Path $exe) -and $installed -eq "src-$commit") { Info "$label já compilado desta versão"; return }
    Info "Compilando o $label (Rust; ~2 min na primeira vez)..."
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    cargo build --release --manifest-path (Join-Path $Root "prototype\$crate\Cargo.toml") 2>&1 | Out-Null
    $ErrorActionPreference = $prevEap
    $built = Join-Path $Root "prototype\$crate\target\release\$builtName"
    if ((Test-Path $built) -and (& $place $built)) { Set-Content $stamp "src-$commit"; Info "$label compilado e instalado" }
    else { Warn "Não consegui compilar o $label." }
  } else {
    Warn "$label ainda sem versão publicada e sem Rust para compilar."
  }
}

Install-Binary 'app Astrograph' 'astrograph.exe' 'astrograph-windows-x64.exe' 'astrograph-ui' 'astrograph-ui.exe'
# ponte com os agentes (Claude Code, opencode...): usada por <Leader>Ae no editor
Install-Binary 'agente nativo' 'astrograph-agent.exe' 'astrograph-agent-windows-x64.exe' 'astrograph-agent' 'astrograph-agent.exe'
if (-not (Test-Path $AppExe)) { Warn 'Sem o app, o Astrograph segue funcionando no terminal (nvim).' }

if (Test-Path $AppExe) {
  # atalho no menu Iniciar (com o ícone do app)
  $lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Astrograph.lnk'
  try {
    $shell = New-Object -ComObject WScript.Shell
    $sc = $shell.CreateShortcut($lnk)
    $sc.TargetPath = $AppExe
    $sc.WorkingDirectory = $env:USERPROFILE
    $sc.IconLocation = "$AppExe,0"
    $sc.Description = 'Astrograph: editor com agentes de IA'
    $sc.Save()
    Info 'Atalho "Astrograph" no menu Iniciar'
  } catch { Warn "Não consegui criar o atalho do menu Iniciar ($($_.Exception.Message))" }
  # comando `astrograph` nos terminais novos
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not (($userPath -split ';') -contains $BinDir)) {
    [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ';' + $BinDir), 'User')
    Info 'Comando "astrograph" disponível nos terminais novos'
  }
}

# --- 6. Baixar plugins (Lazy) e ferramentas (Mason) ------------------------
if (Get-Command nvim -ErrorAction SilentlyContinue) {
  Info 'Baixando plugins, ferramentas do Mason e compilando os realces de sintaxe (pode demorar alguns minutos)...'
  # No PowerShell 5.1 um comando nativo que escreve em stderr aborta o script
  # quando $ErrorActionPreference='Stop' (o nvim/treesitter escreve bastante).
  # Isola as chamadas nativas para o instalador não parar no meio.
  $prevEap = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  # O último passo compila os parsers do Treesitter (alguns segundos) para o
  # primeiro arquivo aberto já vir com o realce de sintaxe pronto.
  nvim --headless "+Lazy! restore" "+MasonToolsInstallSync" "+lua require('astrograph.treesitter').preinstall()" +qa 2>&1 | Out-Null

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
  [pscustomobject]@{ Id = 'ollama';   Bin = 'ollama';   Label = 'Ollama';      Detail = 'modelo local (offline)';       Npm = $null },
  # gateway de modelos (opcional): vira um provedor a mais no chat, nunca o padrao
  [pscustomobject]@{ Id = 'omniroute'; Bin = 'omniroute'; Label = 'OmniRoute';  Detail = 'npm omniroute (gateway)';      Npm = 'omniroute' }
)
$agentModels = @('qwen2.5-coder:7b', 'qwen3:8b', 'llama3.1:8b')

function Test-Bin($b) { [bool](Get-Command $b -ErrorAction SilentlyContinue) }

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
  if ($selectedIds -contains 'omniroute') {
    Write-Host '  omniroute            -> sobe o gateway (porta 20128); crie uma chave com: omniroute keys'
    Write-Host '                          e defina OMNIROUTE_API_KEY. No chat: ga -> omniroute'
  }
  if ($selectedIds -contains 'ollama') { Write-Host '  ollama serve         -> se o Ollama não estiver no ar' }
  Write-Host ''
  Info 'No nvim: <Leader>Al abre o agente no terminal, <Leader>Ac abre o chat, :Astrograph agentes mostra o status.'
}

Info 'Pronto! Abra um terminal novo e rode: nvim'
