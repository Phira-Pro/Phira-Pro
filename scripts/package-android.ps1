[CmdletBinding()]
param([switch]$SkipBuild, [string]$Version = '0.8.2-pro.7')
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$androidProject = Join-Path $workspace 'phira-android'
if (-not $SkipBuild) {
    Push-Location $androidProject
    try {
        & .\gradlew.bat assembleRelease --no-daemon --console=plain
        if ($LASTEXITCODE -ne 0) { throw 'Android build failed' }
    } finally { Pop-Location }
}
$apk = Join-Path $androidProject 'app\build\outputs\apk\release\app-release.apk'
if (-not (Test-Path -LiteralPath $apk)) { throw 'Signed app-release.apk missing; configure the signing environment or keystore.properties' }
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { $env:ANDROID_SDK_ROOT }
if (-not $sdk) { throw 'Set ANDROID_HOME or ANDROID_SDK_ROOT to verify the APK signature' }
$signer = Get-ChildItem -LiteralPath (Join-Path $sdk 'build-tools') -Directory |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'apksigner.bat') } |
    Sort-Object { [version]($_.Name -replace '-.*$', '') } -Descending |
    Select-Object -First 1
if (-not $signer) { throw 'Android build-tools / apksigner missing' }
& (Join-Path $signer.FullName 'apksigner.bat') verify $apk
if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed' }
$deliveryRoot = Join-Path (Split-Path -Parent $workspace) 'dist\android'
New-Item -ItemType Directory -Path $deliveryRoot -Force | Out-Null
$name = "PhiraPro-v$Version-android-arm64-v8a.apk"
$destination = Join-Path $deliveryRoot $name
Copy-Item -LiteralPath $apk -Destination $destination
$hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText("$destination.sha256", "$hash  $name`n", [System.Text.UTF8Encoding]::new($false))
. (Join-Path $PSScriptRoot 'package-cleanup.ps1')
Clear-OldPackages -DeliveryRoot $deliveryRoot -Version $Version
Write-Output $destination
Write-Output "SHA256: $hash"
