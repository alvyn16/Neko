$ErrorActionPreference = "Stop"
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot ".." )).Path
$distRoot = Join-Path $projectRoot "dist"
$packageRoot = Join-Path $distRoot "Neko"
$zipPath = Join-Path $distRoot "Neko-windows-x64.zip"

New-Item -ItemType Directory -Force -Path $distRoot | Out-Null
if (Test-Path -LiteralPath $packageRoot) { Remove-Item -LiteralPath $packageRoot -Recurse -Force }
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }

Push-Location $projectRoot
try { cargo build --release --locked }
finally { Pop-Location }

New-Item -ItemType Directory -Force -Path $packageRoot | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot "target\release\neko.exe") -Destination (Join-Path $packageRoot "neko.exe")
Copy-Item -LiteralPath (Join-Path $projectRoot "README.md") -Destination (Join-Path $packageRoot "README.md")
Copy-Item -LiteralPath (Join-Path $projectRoot "LICENSE") -Destination (Join-Path $packageRoot "LICENSE")
New-Item -ItemType Directory -Force -Path (Join-Path $packageRoot "icons") | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\neko-icon.svg") -Destination (Join-Path $packageRoot "icons\neko-icon.svg")
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\neko-icon-symbolic.svg") -Destination (Join-Path $packageRoot "icons\neko-icon-symbolic.svg")
Copy-Item -LiteralPath (Join-Path $projectRoot "assets\neko-icon.ico") -Destination (Join-Path $packageRoot "icons\neko-icon.ico")
Compress-Archive -LiteralPath $packageRoot -DestinationPath $zipPath -CompressionLevel Optimal
Write-Host "Created $zipPath"
