param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot ".." )).Path

if (-not $SkipBuild) {
    Push-Location $projectRoot
    try { cargo build --release --locked }
    finally { Pop-Location }
}

$compilerCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
)
$compiler = $compilerCandidates | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $compiler) {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) { $compiler = $command.Source }
}
if (-not $compiler) {
    throw "Inno Setup 6 is required. Install it with: winget install JRSoftware.InnoSetup"
}

& $compiler (Join-Path $projectRoot "installer\Neko.iss")
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }

$installer = Join-Path $projectRoot "dist\Neko-Setup-x64.exe"
if (-not (Test-Path -LiteralPath $installer)) { throw "Installer was not created" }
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$installer.sha256", "$hash  Neko-Setup-x64.exe`n")
Write-Host "Created $installer"
