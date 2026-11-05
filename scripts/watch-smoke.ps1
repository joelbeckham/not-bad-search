param([string]$Distro='Ubuntu-24.04')
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath("$PSScriptRoot\..")
$id=[guid]::NewGuid().ToString('N')
$linux="/tmp/good-search-probe-$id"
& wsl -d $Distro -- mkdir -p $linux
if ($LASTEXITCODE -ne 0) {throw 'Could not create WSL fixture'}
$root="\\wsl.localhost\$Distro\tmp\good-search-probe-$id"
$log="$repo\test-data\wsl-watch-$id.log"
$proc=Start-Process -FilePath "$repo\target\release\diagnostics.exe" -ArgumentList @('watch',$root) -RedirectStandardOutput $log -RedirectStandardError "$log.err" -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2
if (-not ((Get-Content $log -Raw) -match 'WATCH_READY')) {throw "Watcher failed: $(Get-Content "$log.err" -Raw)"}
& wsl -d $Distro -- touch "$linux/created.txt"
Start-Sleep -Seconds 1
& wsl -d $Distro -- mv "$linux/created.txt" "$linux/renamed.txt"
Start-Sleep -Seconds 1
& wsl -d $Distro -- rm "$linux/renamed.txt"
$proc.WaitForExit()
Get-Content $log
# Leave a filename for the Windows SQLite traversal/reconciliation check.
& wsl -d $Distro -- touch "$linux/wsl-search-proof.md"
& "$repo\target\release\diagnostics.exe" index "$repo\test-data\wsl-$id.sqlite" $root
& wsl -d $Distro -- rm "$linux/wsl-search-proof.md"
& "$repo\target\release\diagnostics.exe" index "$repo\test-data\wsl-$id.sqlite" $root
& wsl -d $Distro -- rmdir $linux
