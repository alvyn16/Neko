param(
    [Parameter(Mandatory = $true)]
    [string[]]$File,
    [Parameter(Mandatory = $true)]
    [string]$CertificatePath,
    [Parameter(Mandatory = $true)]
    [string]$CertificatePassword
)

$ErrorActionPreference = "Stop"
if (-not (Test-Path -LiteralPath $CertificatePath)) {
    throw "The signing certificate was not found"
}

$signTool = (Get-Command signtool.exe -ErrorAction SilentlyContinue).Source
if (-not $signTool) {
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $signTool = Get-ChildItem -LiteralPath $kits -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\signtool.exe" } |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1
}
if (-not $signTool) {
    throw "SignTool was not found. Install the Windows SDK signing tools."
}

foreach ($target in $File) {
    $resolved = (Resolve-Path -LiteralPath $target).Path
    & $signTool sign /fd SHA256 /td SHA256 /tr "http://timestamp.digicert.com" /f $CertificatePath /p $CertificatePassword $resolved
    if ($LASTEXITCODE -ne 0) { throw "SignTool failed for $resolved" }
    & $signTool verify /pa $resolved
    if ($LASTEXITCODE -ne 0) { throw "Signature verification failed for $resolved" }
}
