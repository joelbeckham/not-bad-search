param([ValidateSet('x64','ARM64')][string]$Architecture = 'x64', [switch]$Register)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$dotnet = Join-Path $repo '.tools\dotnet\dotnet.exe'
if (-not (Test-Path $dotnet)) { $dotnet = (Get-Command dotnet -ErrorAction Stop).Source }
$destination = Join-Path $PSScriptRoot "AppPackages\$Architecture"
& $dotnet publish (Join-Path $PSScriptRoot 'App\GoodSearch.CommandPalette.csproj') -c Release -r "win-$($Architecture.ToLowerInvariant())" -p:Platform=$Architecture --self-contained true -o $destination
if ($LASTEXITCODE -ne 0) { throw 'Extension publish failed' }
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'App\Package.appxmanifest') -Destination (Join-Path $destination 'AppxManifest.xml')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'THIRD-PARTY-NOTICES.md') -Destination $destination
New-Item -ItemType Directory -Force (Join-Path $destination 'Public'), (Join-Path $destination 'Assets') | Out-Null
Add-Type -AssemblyName System.Drawing
foreach ($asset in @(@('StoreLogo',50), @('Square44x44Logo',44), @('Square150x150Logo',150))) {
    $size = [int]$asset[1]
    $bitmap = New-Object System.Drawing.Bitmap($size,$size)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $pen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, [single]($size / 12))
    try {
        $graphics.Clear([System.Drawing.Color]::FromArgb(35,85,120))
        $graphics.SmoothingMode = 'AntiAlias'
        $graphics.DrawEllipse($pen,[single]($size*.22),[single]($size*.18),[single]($size*.44),[single]($size*.44))
        $graphics.DrawLine($pen,[single]($size*.60),[single]($size*.58),[single]($size*.82),[single]($size*.80))
        $bitmap.Save((Join-Path $destination "Assets\$($asset[0]).png"),[System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $pen.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
}
Write-Output "Extension package layout: $destination"
if ($Register) {
    Add-AppxPackage -Register (Join-Path $destination 'AppxManifest.xml')
    Write-Output 'Registered for the current user. Run Reload in Command Palette.'
}
