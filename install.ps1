# Astrograph - instalador para Windows (uma linha)
#
#   irm https://raw.githubusercontent.com/Just-Gabe/Astrograph/main/install.ps1 | iex
#
# O que ele faz:
#   1. Instala (via winget): Neovim, Git, ripgrep, fd e o gcc do WinLibs
#   2. Instala a Nerd Font JetBrainsMono (por usuário, sem admin) + registra no Windows
#   3. (Opcional) aponta a fonte do Windows Terminal para a Nerd Font
#   4. Clona o Astrograph em %LOCALAPPDATA%\Astrograph
#   5. Liga a config: %LOCALAPPDATA%\nvim  ->  Astrograph\template. Se já houver
#      uma config do Neovim, pergunta: limpar e substituir, importar ou manter
#      (ASTROGRAPH_CONFIG_EXISTENTE=limpar|importar|manter responde sem perguntar)
#   6. Instala o app Astrograph (UI em Rust): baixa do GitHub Releases; sem
#      release, instala o Rust + Build Tools do Visual C++ e compila
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

function Test-Interactive {
  if ([Console]::IsInputRedirected) { return $false }
  try { $null = [Console]::KeyAvailable; return $true } catch { return $false }
}

# --- 0b. Config do Neovim que já existe na máquina --------------------------
# Pergunta logo no início: escolher "manter" não deve custar a instalação toda.
$Template = Join-Path $Root 'template'
$DataDir  = Join-Path $env:LOCALAPPDATA 'nvim-data'
$LocalDir = if ($env:ASTROGRAPH_LOCAL) { $env:ASTROGRAPH_LOCAL } else { Join-Path $env:LOCALAPPDATA 'astrograph-local' }
$existing = $null        # pasta com a config de antes (ou destino da ligação antiga)
$existingIsLink = $false
$configMode = 'nova'
$cfgItem = Get-Item -LiteralPath $Config -Force -ErrorAction SilentlyContinue
if ($cfgItem) {
  if ($cfgItem.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    $target = (@($cfgItem.Target) -join '').TrimEnd('\')
    if ($target -ne $Template.TrimEnd('\') -and (Test-Path -LiteralPath $target)) {
      $existing = $target; $existingIsLink = $true
    }
  } elseif (Get-ChildItem -LiteralPath $Config -Force | Select-Object -First 1) {
    $existing = $Config
  }
}
# ASTROGRAPH_SEM_NVIM=1: só o app nativo em Rust (motor próprio), sem Neovim nem plugins
$semNvim = $env:ASTROGRAPH_SEM_NVIM -eq '1'
if ($semNvim) { $existing = $null }
if ($existing) {
  $oldInit = Join-Path $existing 'init.lua'
  $isOldAstrograph = (Test-Path $oldInit) -and ((Get-Content $oldInit -Raw) -match 'Astrograph')
  $configMode = "$env:ASTROGRAPH_CONFIG_EXISTENTE".Trim().ToLower()
  if ($configMode -and $configMode -notin @('limpar', 'importar', 'manter')) {
    Warn "ASTROGRAPH_CONFIG_EXISTENTE='$configMode' não reconhecido (use limpar, importar ou manter)."
    $configMode = ''
  }
  if (-not $configMode -and $isOldAstrograph) {
    # cópia antiga do próprio Astrograph (não a ligação): importar duplicaria
    # os plugins do Astrograph; vai para backup e a ligação nova entra
    Info "Achei uma cópia antiga do Astrograph em ${existing}: vai para backup e é substituída."
    $configMode = 'limpar'
  }
  if (-not $configMode) {
    if (Test-Interactive) {
      $desc = if ($existingIsLink) { "uma ligação para $existing" } else { $existing }
      Write-Host ''
      Warn "Já existe uma config do Neovim: $desc"
      Write-Host '  [1] Limpar e substituir  config e dados antigos (plugins, Mason) vão para backup; começa do zero'
      Write-Host '  [2] Importar             faz o backup e traz seus plugins (lua/plugins), polish, módulos,'
      Write-Host "                           ftplugin/after/snippets para os plugins locais ($LocalDir)"
      Write-Host '  [3] Manter               não mexe na config do Neovim (instala o resto do Astrograph)'
      $pick = (Read-Host '  escolha [1]').Trim()
      $configMode = switch ($pick) { '2' { 'importar' } '3' { 'manter' } default { 'limpar' } }
    } else {
      Info 'Sem terminal interativo: a config existente vai para backup e é substituída (ASTROGRAPH_CONFIG_EXISTENTE muda isso).'
      $configMode = 'limpar'
    }
  }
  Info "Config existente: $configMode"
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
if ($semNvim) {
  Info 'ASTROGRAPH_SEM_NVIM=1: sem Neovim, ripgrep, fd nem gcc (as buscas do app nativo são Rust).'
  $packages = @($packages | Where-Object { $_.cmd -eq 'git' })
}
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

# Já instalada (para o usuário ou para todos, em qualquer variante: Mono, NL...)?
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
  Warn 'Windows Terminal não encontrado - configure a Nerd Font no seu terminal manualmente.'
}

# --- 4. Clonar/atualizar o Astrograph --------------------------------------
$appOpen = Get-Process -Name 'astrograph' -ErrorAction SilentlyContinue |
  Where-Object { $_.Path -and $_.Path.StartsWith($Root, [StringComparison]::OrdinalIgnoreCase) }
if ($appOpen) {
  # o app aberto segura arquivos da config; o pull trocaria só parte deles e a
  # config ficaria misturada (ex.: "module 'polish' not found"). Não fecha à
  # força: pode haver arquivo sem salvar.
  Warn 'O Astrograph está aberto: pulei a atualização. Feche o app e rode o instalador de novo.'
} else {
  # Pasta que já tem arquivos mas não é um clone limpo (arquivos modificados, histórico próprio ou nem
  # é git): pergunta se apaga e substitui (a antiga vai para backup) ou usa o que está lá.
  $cloneMode = "$env:ASTROGRAPH_CLONE_EXISTENTE".Trim().ToLower()
  if ($cloneMode -and $cloneMode -notin @('substituir', 'usar')) {
    Warn "ASTROGRAPH_CLONE_EXISTENTE='$cloneMode' não reconhecido (use substituir ou usar)."
    $cloneMode = ''
  }
  function Choose-Clone($why) {
    if ($script:cloneMode) { return }
    if (Test-Interactive) {
      Write-Host ''
      Warn $why
      Write-Host "  [1] Apagar e substituir  a pasta atual vai para backup ($Root.backup-<data>) e o Astrograph é clonado de novo"
      Write-Host '  [2] Usar os existentes   não mexe nos arquivos (não atualiza; o :Astrograph atualizar pode recusar)'
      $pick = (Read-Host '  escolha [1]').Trim()
      $script:cloneMode = if ($pick -eq '2') { 'usar' } else { 'substituir' }
    } else {
      Info 'Sem terminal interativo: uso os arquivos existentes (ASTROGRAPH_CLONE_EXISTENTE=substituir apaga e clona de novo).'
      $script:cloneMode = 'usar'
    }
  }
  function Replace-Clone {
    $bk = "$Root.backup-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
    Info "A pasta atual vai para $bk"
    Move-Item -LiteralPath $Root -Destination $bk
    Info "Clonando o Astrograph em $Root"
    git clone --depth 1 $Repo $Root
  }
  $hasFiles = (Test-Path $Root) -and (@(Get-ChildItem -LiteralPath $Root -Force -ErrorAction SilentlyContinue).Count -gt 0)
  if ($hasFiles -and -not (Test-Path (Join-Path $Root '.git'))) {
    Choose-Clone "A pasta $Root já tem arquivos, mas não é um clone do Astrograph."
    if ($cloneMode -eq 'substituir') { Replace-Clone }
    elseif (-not (Test-Path (Join-Path $Root 'template'))) { throw "Sem $Root\template não dá para usar essa pasta. Rode de novo e escolha substituir." }
    else { Info "Usando os arquivos existentes em $Root (sem atualizar)" }
  } elseif (Test-Path (Join-Path $Root '.git')) {
    Info "Atualizando $Root"
    # o `:Lazy update` reescreve o lazy-lock.json versionado; sem descartar essa
    # alteração o pull falha e a instalação continua velha sem avisar
    git -C $Root checkout -- template/lazy-lock.json
    if (@(git -C $Root status --porcelain --untracked-files=no).Count -gt 0) {
      Choose-Clone "O clone em $Root tem arquivos modificados."
    }
    if ($cloneMode -eq 'substituir') { Replace-Clone }
    elseif ($cloneMode -eq 'usar') { Info "Usando os arquivos existentes em $Root (sem atualizar)" }
    else {
      git -C $Root pull --ff-only
      if ($LASTEXITCODE -ne 0) {
        Choose-Clone "Não consegui atualizar $Root (o histórico local diverge da main)."
        if ($cloneMode -eq 'substituir') { Replace-Clone } else { Info "Usando os arquivos existentes em $Root (sem atualizar)" }
      }
    }
  } else {
    Info "Clonando o Astrograph em $Root"
    git clone --depth 1 $Repo $Root
  }
}
# Arquivo da config sumido do clone (pull interrompido, antivírus...): o nvim
# abriria com "module 'polish' not found". Restaura só o que foi apagado.
$gone = @(git -C $Root ls-files -d -- template 2>$null | Where-Object { $_ })
if ($gone.Count -gt 0) {
  git -C $Root checkout -- $gone
  Info "Restaurados $($gone.Count) arquivo(s) da config que faltavam no clone"
}

# --- 5. Ligar a config -----------------------------------------------------
# Traz o que dá para reaproveitar da config antiga para os plugins locais
# (astrograph-local, fora do git). Nunca sobrescreve um arquivo que já esteja lá.
function Import-NvimConfig($src) {
  $specs = Join-Path $LocalDir 'specs'
  $plug  = Join-Path $LocalDir 'plugin'
  $lua   = Join-Path $LocalDir 'lua'
  New-Item -ItemType Directory -Force -Path $specs, $plug, $lua | Out-Null
  $done = New-Object System.Collections.Generic.List[string]
  $kept = New-Object System.Collections.Generic.List[string]
  function Put($from, $to) {
    if (Test-Path -LiteralPath $to) { $kept.Add($to); return }
    New-Item -ItemType Directory -Force -Path (Split-Path $to) | Out-Null
    Copy-Item -LiteralPath $from $to -Recurse
    $done.Add($to)
  }
  $srcLua = Join-Path $src 'lua'
  # specs do lazy: cada arquivo de lua/plugins vira um spec local
  $plugins = Join-Path $srcLua 'plugins'
  if (Test-Path $plugins) {
    Get-ChildItem $plugins -Filter '*.lua' -File | ForEach-Object { Put $_.FullName (Join-Path $specs "importado-$($_.Name)") }
    Get-ChildItem $plugins -Directory | ForEach-Object {
      Put $_.FullName (Join-Path $lua "plugins\$($_.Name)")
      $init = Join-Path $_.FullName 'init.lua'
      if (Test-Path $init) { Put $init (Join-Path $specs "importado-$($_.Name).lua") }
    }
  }
  $community = Join-Path $srcLua 'community.lua'
  if (Test-Path $community) { Put $community (Join-Path $specs 'importado-community.lua') }
  # polish roda depois dos plugins: vira um plugin/ do astrograph-local
  $polish = Join-Path $srcLua 'polish.lua'
  if (Test-Path $polish) { Put $polish (Join-Path $plug 'zz-importado-polish.lua') }
  # demais módulos (com os nomes de origem, para os `require` continuarem valendo)
  $ours = @('plugins', 'polish.lua', 'lazy_setup.lua', 'community.lua', 'astrograph')
  if (Test-Path $srcLua) {
    Get-ChildItem $srcLua -Force | Where-Object { $ours -notcontains $_.Name } |
      ForEach-Object { Put $_.FullName (Join-Path $lua $_.Name) }
  }
  # LazyVim: options/keymaps/autocmds são carregados pela distro; aqui, por nós
  $lazyvim = @('options', 'keymaps', 'autocmds') | Where-Object { Test-Path (Join-Path $srcLua "config\$_.lua") }
  if ($lazyvim) {
    $tmp = Join-Path $env:TEMP 'astrograph-importado-config.lua'
    $body = ($lazyvim | ForEach-Object { "pcall(require, `"config.$_`")" }) -join "`n"
    [IO.File]::WriteAllText($tmp, "-- importado da config antiga (LazyVim)`n$body`n", (New-Object System.Text.UTF8Encoding($false)))
    Put $tmp (Join-Path $plug 'zz-importado-config.lua')
    Remove-Item $tmp -ErrorAction SilentlyContinue
  }
  # pastas que o Neovim lê direto do runtimepath
  foreach ($d in @('after', 'ftplugin', 'ftdetect', 'snippets', 'spell', 'queries', 'syntax', 'indent', 'colors', 'autoload')) {
    $p = Join-Path $src $d
    if (Test-Path $p) { Put $p (Join-Path $LocalDir $d) }
  }
  # init.lua solto (só opções/atalhos) entra; um que monta o próprio lazy.nvim, não
  $initLua = Join-Path $src 'init.lua'
  $initVim = Join-Path $src 'init.vim'
  if ((Test-Path $initLua) -and ((Get-Content $initLua -Raw) -notmatch 'lazy')) {
    Put $initLua (Join-Path $plug '00-importado-init.lua')
  } elseif (Test-Path $initVim) {
    Put $initVim (Join-Path $plug '00-importado-init.vim')
  } elseif (Test-Path $initLua) {
    Warn "init.lua antigo monta o próprio gerenciador de plugins: não importei ($initLua). Copie à mão o que quiser."
  }
  $manifest = @("Importado em $(Get-Date -Format 'yyyy-MM-dd HH:mm') de: $src", '',
    'Para desfazer, apague os arquivos abaixo.', '') + $done
  if ($kept.Count) { $manifest += @('', 'Não copiados (já existiam):') + $kept }
  Add-Content -Path (Join-Path $LocalDir 'IMPORTADO.txt') -Value $manifest -Encoding UTF8
  Info "Importados $($done.Count) item(ns) para $LocalDir (lista em IMPORTADO.txt)"
  if ($kept.Count) { Warn "$($kept.Count) item(ns) já existiam em $LocalDir e foram mantidos" }
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if ($semNvim) {
  Info 'Sem Neovim: a config dele não foi tocada.'
} elseif ($configMode -eq 'manter') {
  Warn "Config do Neovim mantida: o nvim continua abrindo a sua config, não a do Astrograph."
} else {
  if ($existing) {
    if ($existingIsLink) {
      # só o link: o Remove-Item numa junction pergunta "tem filhos, apagar?" e,
      # confirmado, apagaria o conteúdo do destino
      [IO.Directory]::Delete($Config)
      Info "Ligação antiga removida ($Config -> $existing; o destino ficou intacto)"
    } else {
      $existing = "$Config.backup-$stamp"
      try { Move-Item -LiteralPath $Config $existing }
      catch { throw "Não consegui mover $Config (algum nvim aberto?). Feche o Neovim/Astrograph e rode de novo." }
      Info "Config anterior movida para $existing"
    }
    if ($configMode -eq 'importar') { Import-NvimConfig $existing }
    if ($configMode -eq 'limpar' -and (Test-Path $DataDir)) {
      # plugins/estado da config antiga misturados com os do Astrograph dão
      # erro difícil de achar; o backup permite voltar
      try {
        Move-Item -LiteralPath $DataDir "$DataDir.backup-$stamp"
        Info "Dados antigos (plugins, Mason, histórico) movidos para $DataDir.backup-$stamp"
      } catch { Warn "Não consegui mover $DataDir (algum nvim aberto?). Os dados antigos ficaram." }
    }
    # cache de bytecode do vim.loader: se refaz sozinho
    Remove-Item (Join-Path $env:TEMP 'nvim\luac') -Recurse -Force -ErrorAction SilentlyContinue
  }
  $cfgItem = Get-Item -LiteralPath $Config -Force -ErrorAction SilentlyContinue
  $linked = $cfgItem -and ($cfgItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -and
    ((@($cfgItem.Target) -join '').TrimEnd('\') -eq $Template.TrimEnd('\'))
  if ($linked) {
    Info "Config já ligada: $Config -> $Template"
  } else {
    # ligação quebrada ou pasta vazia que sobrou
    # (Delete sem recursão: falha em vez de apagar se houver conteúdo)
    if ($cfgItem) { [IO.Directory]::Delete($Config) }
    cmd /c mklink /J "$Config" $Template | Out-Null
    Info "Ligado: $Config -> $Template"
  }
}

# --- 5b. App Astrograph (UI em Rust) ---------------------------------------
# O Astrograph é a janela própria (Rust) com o Neovim como motor. Baixa o
# executável do GitHub Releases só quando sai uma versão nova; sem release
# publicado (ou com download recusado), instala o que falta e compila do código.
$BinDir   = Join-Path $Root 'bin'
$AppExe   = Join-Path $BinDir 'astrograph.exe'
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

# o rustup instala em ~\.cargo\bin, mas nem sempre põe a pasta no PATH
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $cargoBin 'cargo.exe'))) {
  $env:Path = "$cargoBin;$env:Path"
}

$isArm = $env:PROCESSOR_ARCHITECTURE -eq 'ARM64'
$rustHost = if ($isArm) { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$vcComponent = if ($isArm) { 'Microsoft.VisualStudio.Component.VC.Tools.ARM64' } else { 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64' }

# Linker do Visual C++ (link.exe), que o Rust usa no Windows. A toolchain GNU
# (MinGW) não serve: o ld dela não acha os arquivos quando o caminho do perfil
# tem acento (C:\Users\João...), nem com os nomes curtos 8.3.
function Test-MsvcLinker {
  $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path $vswhere)) { return $false }
  $found = & $vswhere -latest -products * -requires $vcComponent -property installationPath 2>$null
  return [bool]$found
}

# Deixa a máquina pronta para compilar: Build Tools do Visual C++ + Rust
# (rustup, toolchain MSVC). Roda uma vez só, e só quando precisa compilar.
$script:rustReady = $null
$script:cargoArgs = @()
function Ensure-Rust {
  if ($null -ne $script:rustReady) { return $script:rustReady }
  $script:rustReady = $false
  $prevEap = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    if (-not (Test-MsvcLinker)) {
      Info 'Instalando o Visual Studio Build Tools (linker C++ do Rust; ~2 GB, o Windows pede permissão de administrador)...'
      winget install --id Microsoft.VisualStudio.BuildTools -e --silent `
        --accept-package-agreements --accept-source-agreements --disable-interactivity `
        --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --add $vcComponent --includeRecommended" | Out-Null
      if (-not (Test-MsvcLinker)) {
        Warn 'O Build Tools do Visual C++ não ficou instalado (permissão negada?). Rode de novo como administrador.'
        return $false
      }
      Info 'Build Tools do Visual C++ instalado'
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
      Info 'Instalando o Rust (rustup, toolchain estável)...'
      $init = Join-Path $env:TEMP 'rustup-init.exe'
      Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/$rustHost/rustup-init.exe" -OutFile $init -UseBasicParsing
      & $init -y --profile minimal --default-toolchain stable --default-host $rustHost 2>&1 | Out-Null
      $code = $LASTEXITCODE
      Remove-Item $init -ErrorAction SilentlyContinue
      $env:Path = "$cargoBin;$env:Path"
      if ($code -ne 0 -or -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Warn "Não consegui instalar o Rust (rustup-init saiu com $code)."
        return $false
      }
      Info 'Rust instalado'
    } elseif (Get-Command rustup -ErrorAction SilentlyContinue) {
      # Rust que já existia com outra toolchain padrão (ex.: GNU): compila com a MSVC
      $active = (rustup show active-toolchain 2>$null | Out-String)
      if ($active -notmatch 'msvc') {
        Info 'Rust sem a toolchain MSVC como padrão: instalando a stable-msvc só para compilar o Astrograph...'
        rustup toolchain install "stable-$rustHost" --profile minimal 2>&1 | Out-Null
        $script:cargoArgs = @("+stable-$rustHost")
      }
    }
    $script:rustReady = $true
    return $true
  } finally {
    $ErrorActionPreference = $prevEap
  }
}

$release = $null
try {
  $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/Just-Gabe/Astrograph/releases/latest' `
    -Headers @{ 'User-Agent' = 'astrograph-installer' } -TimeoutSec 15
} catch { $release = $null }
if (-not $release) {
  # sem release com tag v*: usa os executáveis que o CI publica a cada push na main (release móvel main-latest)
  try {
    $mainBase = 'https://github.com/Just-Gabe/Astrograph/releases/download/main-latest'
    $mc = (Invoke-WebRequest -Uri "$mainBase/COMMIT.txt" -UseBasicParsing -TimeoutSec 15).Content
    if ($mc -is [byte[]]) { $mc = [Text.Encoding]::ASCII.GetString($mc) }
    $mc = "$mc".Trim()
    if ($mc -match '^[0-9a-f]{40}$') {
      $names = @('SHA256SUMS.txt', 'astrograph-windows-x64.exe', 'astrograph-agent-windows-x64.exe', 'astrograph-servicos-windows-x64.exe')
      $release = [pscustomobject]@{
        tag_name = "main-$($mc.Substring(0, 7))"
        assets   = @($names | ForEach-Object { [pscustomobject]@{ name = $_; browser_download_url = "$mainBase/$_" } })
      }
    }
  } catch { }
}
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
  } else {
    if ((Test-Path $exe) -and $installed -eq "src-$commit") { Info "$label já compilado desta versão"; return }
    if ($env:ASTROGRAPH_SEM_COMPILAR -eq '1') { Warn "$label sem versão publicada e ASTROGRAPH_SEM_COMPILAR=1: pulei."; return }
    if (-not (Ensure-Rust)) { Warn "$label não instalado: falta o Rust para compilar."; return }
    Info "Compilando o $label (Rust; ~2 a 5 min na primeira vez)..."
    $log = Join-Path $env:TEMP "astrograph-build-$crate.log"
    $built = Join-Path $Root "prototype\$crate\target\release\$builtName"
    Remove-Item $built -ErrorAction SilentlyContinue
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $cargoCmd = @($script:cargoArgs) + @('build', '--release', '--manifest-path', (Join-Path $Root "prototype\$crate\Cargo.toml"))
    & cargo @cargoCmd 2>&1 | ForEach-Object { "$_" } | Set-Content $log -Encoding UTF8
    $buildCode = $LASTEXITCODE
    $ErrorActionPreference = $prevEap
    if ($buildCode -eq 0 -and (Test-Path $built)) {
      if (& $place $built) { Set-Content $stamp "src-$commit"; Info "$label compilado e instalado" }
    } else {
      Warn "Não consegui compilar o $label (cargo saiu com $buildCode). Final do log ($log):"
      Get-Content $log -Tail 15 | ForEach-Object { Write-Host "    $_" }
    }
  }
}

Install-Binary 'app Astrograph' 'astrograph.exe' 'astrograph-windows-x64.exe' 'astrograph-ui' 'astrograph-ui.exe'
# ponte com os agentes (Claude Code, opencode...): usada por <Leader>Ae no editor
Install-Binary 'agente nativo' 'astrograph-agent.exe' 'astrograph-agent-windows-x64.exe' 'astrograph-agent' 'astrograph-agent.exe'
# projeto, git e atualização (o :Astrograph atualizar usa este)
Install-Binary 'serviços' 'astrograph-servicos.exe' 'astrograph-servicos-windows-x64.exe' 'astrograph-servicos' 'astrograph-servicos.exe'
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
if ($semNvim) {
  # o app abre sempre no motor nativo (ASTROGRAPH_ENGINE=nvim ainda força o Neovim)
  $toml = Join-Path $BinDir 'astrograph-ui.toml'
  if (-not (Test-Path $toml)) { Set-Content -Path $toml -Value "[engine]`nnative = true" -Encoding ASCII }
  Info "Motor nativo ligado em $toml; pulei plugins do Neovim."
} elseif ($configMode -eq 'manter') {
  Info 'Config do Neovim mantida: pulei os plugins do Astrograph (rodariam na sua config).'
} elseif (Get-Command nvim -ErrorAction SilentlyContinue) {
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
  if ($themeOut -match "module '([^']+)' not found") {
    Warn "O nvim abriu com erro: módulo '$($Matches[1])' não encontrado. A config ligada em $Config está incompleta."
    Warn "Confira com: git -C `"$Root`" status   (ou rode o instalador de novo com ASTROGRAPH_CONFIG_EXISTENTE=limpar)"
  } elseif ($themeName -match 'astrograph') {
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

# Checklist TUI: (cima)/(baixo) navega · espaço marca · enter confirma · a=todos · q/esc cancela
function Read-AgentMenu {
  param([object[]]$Items)
  $n = $Items.Count
  $checked = New-Object 'bool[]' $n
  $cursor = 0
  Write-Host ''
  Write-Host '  Agentes a instalar  ((cima)/(baixo) navega · espaço marca · enter confirma · a=todos)' -ForegroundColor Cyan
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
    Info 'Sem terminal interativo - pulando agentes (use ASTROGRAPH_AGENTS para automatizar).'
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

if ($semNvim) { Info 'Pronto! Abra um terminal novo e rode: astrograph' } else { Info 'Pronto! Abra um terminal novo e rode: nvim' }
