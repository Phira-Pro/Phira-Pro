[CmdletBinding()]
param([switch]$SkipBuild, [string]$Version = '0.8.2-pro.9')
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$deliveryRoot = Join-Path (Split-Path -Parent $workspace) 'dist\windows'
$name = "PhiraPro-v$Version-win64"
$package = Join-Path $deliveryRoot $name
if (-not $SkipBuild) {
    Push-Location $workspace
    try {
        & cargo build --locked --release -p phira-main
        if ($LASTEXITCODE -ne 0) { throw 'Windows build failed' }
    } finally { Pop-Location }
}
$exe = Join-Path $workspace 'target\release\phira-main.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw 'phira-main.exe missing' }
# Use a fresh staging directory: existing user data is never removed or bundled.
if (Test-Path -LiteralPath $package) { throw "Delivery directory already exists: $package" }
New-Item -ItemType Directory -Path $package -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination $package
Copy-Item -LiteralPath (Join-Path $workspace 'assets') -Destination $package -Recurse
Copy-Item -LiteralPath (Join-Path $workspace 'LICENSE') -Destination $package
Copy-Item -LiteralPath (Join-Path $workspace 'docs\development\pro-8.md') -Destination (Join-Path $package '更新说明.md')
Get-ChildItem -LiteralPath (Join-Path $workspace 'target\release') -Filter '*.dll' -File | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $package
}
$zip = Join-Path $deliveryRoot "$name.zip"
Compress-Archive -LiteralPath $package -DestinationPath $zip -CompressionLevel Optimal
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText("$zip.sha256", "$hash  $name.zip`n", [System.Text.UTF8Encoding]::new($false))
. (Join-Path $PSScriptRoot 'package-cleanup.ps1')
Clear-OldPackages -DeliveryRoot $deliveryRoot -Version $Version
Write-Output $zip
Write-Output "SHA256: $hash"
