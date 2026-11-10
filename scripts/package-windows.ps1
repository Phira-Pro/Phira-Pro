[CmdletBinding()]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$taskArguments = @((Join-Path $PSScriptRoot 'build.py'), '--platform', 'windows', '--package')
if ($SkipBuild) { $taskArguments += '--skip-build' }
& python @taskArguments
if ($LASTEXITCODE -ne 0) { throw 'Windows build/package failed' }
$workspace = Split-Path -Parent $PSScriptRoot
$Version = & python (Join-Path $PSScriptRoot 'version.py') show --field pro_version
if ($LASTEXITCODE -ne 0) { throw 'Cannot read the package version' }
. (Join-Path $PSScriptRoot 'package-cleanup.ps1')
Clear-OldPackages -DeliveryRoot (Join-Path (Split-Path -Parent $workspace) 'dist\windows') -Version $Version
