# Kimi Build installer for Windows PowerShell.

param(
    [Parameter(Position = 0)]
    [string]$Version
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol =
    [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

if ($PSVersionTable.Platform -and $PSVersionTable.Platform -ne 'Win32NT') {
    throw 'This installer only supports Windows PowerShell.'
}

$Repository = if ($env:KAMI_REPOSITORY) { $env:KAMI_REPOSITORY } else { 'Ayor1337/kimi-build' }
$ReleasesUrl = "https://github.com/$Repository/releases"
$KamiHome = if ($env:KAMI_HOME) { $env:KAMI_HOME } else { Join-Path $env:USERPROFILE '.kami' }
$DownloadDir = Join-Path $KamiHome 'downloads'
$BinDir = if ($env:KAMI_BIN_DIR) { $env:KAMI_BIN_DIR } else { Join-Path $KamiHome 'bin' }

if (-not $Version -and $env:KAMI_VERSION) {
    $Version = $env:KAMI_VERSION
}
if (-not $Version) {
    Write-Host 'Fetching latest Kimi Build version...' -ForegroundColor DarkGray
    $Version = (Invoke-WebRequest -UseBasicParsing "$ReleasesUrl/latest/download/version.txt").Content.Trim()
}
if ($Version -notmatch '^\d+\.\d+\.\d+(-[A-Za-z0-9._]+)?$') {
    throw "Invalid version '$Version' (expected X.Y.Z or X.Y.Z-suffix)."
}

$Architecture = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'x86' { 'x86_64' }
    'ARM64' { throw 'Windows ARM64 releases are not available yet.' }
    default { throw "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
}

$Asset = "kami-$Version-windows-$Architecture.exe"
$Url = "$ReleasesUrl/download/v$Version/$Asset"
New-Item -ItemType Directory -Force $DownloadDir, $BinDir | Out-Null
$Binary = Join-Path $DownloadDir $Asset
$Temporary = "$Binary.tmp.$PID"

try {
    Write-Host "Installing Kimi Build $Version (windows-$Architecture)..." -ForegroundColor Cyan
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Temporary
    & $Temporary --version *> $null
    if ($LASTEXITCODE -ne 0) {
        throw 'Downloaded binary failed its version check; the current install is unchanged.'
    }
    Move-Item -Force $Temporary $Binary
} finally {
    if (Test-Path $Temporary) { Remove-Item -Force $Temporary }
}

foreach ($Name in @('kami.exe', 'grok.exe', 'agent.exe')) {
    Copy-Item -Force $Binary (Join-Path $BinDir $Name)
}

$ConfigFile = Join-Path $KamiHome 'config.toml'
if (-not (Test-Path $ConfigFile)) {
    [IO.File]::WriteAllText($ConfigFile, "[cli]`r`ninstaller = `"gh-release`"`r`n")
} else {
    $Content = Get-Content -Raw $ConfigFile
    if ($Content -match '(?m)^\[cli\]\s*$') {
        $Lines = Get-Content $ConfigFile
        $Output = [Collections.Generic.List[string]]::new()
        $InCli = $false
        foreach ($Line in $Lines) {
            if ($Line -match '^\[cli\]\s*$') {
                $Output.Add($Line)
                $Output.Add('installer = "gh-release"')
                $InCli = $true
                continue
            }
            if ($Line -match '^\[') { $InCli = $false }
            if ($InCli -and $Line -match '^\s*installer\s*=') { continue }
            $Output.Add($Line)
        }
        [IO.File]::WriteAllLines($ConfigFile, $Output)
    } else {
        Add-Content $ConfigFile "`r`n[cli]`r`ninstaller = `"gh-release`""
    }
}

$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$PathEntries = if ($UserPath) { $UserPath -split ';' } else { @() }
if ($PathEntries -notcontains $BinDir) {
    [Environment]::SetEnvironmentVariable('Path', (@($BinDir) + $PathEntries) -join ';', 'User')
}
if (($env:Path -split ';') -notcontains $BinDir) {
    $env:Path = "$BinDir;$env:Path"
}

Write-Host "Kimi Build $Version installed to $BinDir\kami.exe." -ForegroundColor Green
Write-Host "Run 'kami' to get started." -ForegroundColor Cyan
