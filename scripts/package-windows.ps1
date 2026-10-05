[CmdletBinding()]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$taskArguments = @((Join-Path $PSScriptRoot 'build.py'), '--platform', 'windows', '--package')
if ($SkipBuild) { $taskArguments += '--skip-build' }
& python @taskArguments
if ($LASTEXITCODE -ne 0) { throw 'Windows build/package failed' }
$workspace = Split-Path -Parent $PSScriptRoot
$versions = Get-Content -Raw -LiteralPath (Join-Path $workspace 'version.json') | ConvertFrom-Json
$Version = "$($versions.base_version)-pro.$($versions.pro_revision)"
. (Join-Path $PSScriptRoot 'package-cleanup.ps1')
Clear-OldPackages -DeliveryRoot (Join-Path (Split-Path -Parent $workspace) 'dist\windows') -Version $Version
