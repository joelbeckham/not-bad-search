$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$toolsDir = Join-Path $repo '.tools\dotnet'
New-Item -ItemType Directory -Force $toolsDir | Out-Null
$installer = Join-Path $repo '.tools\dotnet-install.ps1'
Invoke-WebRequest -UseBasicParsing 'https://dot.net/v1/dotnet-install.ps1' -OutFile $installer
& $installer -Channel 10.0 -InstallDir $toolsDir -NoPath
if (-not (Test-Path (Join-Path $toolsDir 'dotnet.exe'))) { throw 'SDK installation failed' }
