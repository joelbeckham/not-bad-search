param([string]$Exe = "$PSScriptRoot\..\target\release\not-bad-search.exe",[int]$Rounds=20,[string]$Folder='',[switch]$Content)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class SearchProbe {
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public delegate bool Callback(IntPtr h,IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool EnumWindows(Callback c,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,System.Text.StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 public static IntPtr Find(uint id) {IntPtr result=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==id){var s=new System.Text.StringBuilder(256);GetWindowText(h,s,256);if(s.ToString()=="Not Bad Search" && IsWindowVisible(h))result=h;}return true;},IntPtr.Zero);return result;}
 public static void Activate(IntPtr h) {Rect r;GetWindowRect(h,out r);double scale=GetDpiForWindow(h)/96.0;SetCursorPos(r.Left+(int)(120*scale),r.Top+(int)(36*scale));mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
}
'@
[void][SearchProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=[IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\repeat-$([guid]::NewGuid().ToString('N'))")
$data=Join-Path $base 'data';$files=Join-Path $base 'files'
New-Item -ItemType Directory -Force $data | Out-Null
if($Folder) {$files=(Resolve-Path -LiteralPath $Folder).Path} else {
 New-Item -ItemType Directory -Force $files | Out-Null
 for($i=0;$i -lt 2279;$i++){
  $name=if($Content){"note-{0:X4}.md" -f $i}else{"report-2026-{0:D5}.md" -f $i}
  $body=if($Content){"# Planning`nBudget for 2026 includes the marmalade telescope."}else{''}
  [IO.File]::WriteAllText((Join-Path $files $name),$body)
 }
}
$contentRoots=if($Content){"['$files']"}else{'[]'}
@"
folders = ['$files']
exclusions = []
content_folders = $contentRoots
semantic_enabled = false
reconcile_seconds = 30
"@ | Set-Content "$data\config.toml"
$oldData=$env:NOT_BAD_SEARCH_DATA;$oldTrace=$env:NOT_BAD_SEARCH_TRACE
$env:NOT_BAD_SEARCH_DATA=$data;$env:NOT_BAD_SEARCH_TRACE='1'
$proc=Start-Process -FilePath $Exe -PassThru
function Send-TestKeys([string]$keys) {
 if([SearchProbe]::GetForegroundWindow() -ne $hwnd){throw 'Test window lost focus; refusing to type into another app'}
 [Windows.Forms.SendKeys]::SendWait($keys)
}
function Wait-Result([string]$query,[int]$hits,[long]$after,[int]$timeoutMs=1500) {
 $timer=[Diagnostics.Stopwatch]::StartNew()
 $pattern='^query="'+[regex]::Escape($query)+'" hits='+$hits+' .*generation=(\d+)$'
 do {
  foreach($line in (Get-Content "$data\ui-trace.log" -ErrorAction SilentlyContinue)) {
   if($line -match $pattern -and [long]$Matches[1] -gt $after){return [long]$Matches[1]}
  }
  Start-Sleep -Milliseconds 10
 } while($timer.ElapsedMilliseconds -lt $timeoutMs)
 throw "No fresh result for '$query' within $timeoutMs ms. Trace: $data\ui-trace.log"
}
try {
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 do {Start-Sleep -Milliseconds 100;$hwnd=[SearchProbe]::Find($proc.Id)} until($hwnd -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $deadline)
 if($hwnd -eq [IntPtr]::Zero){throw 'No search window'}
 [void][SearchProbe]::SetWindowPos($hwnd,[IntPtr](-1),0,0,0,0,0x43)
 Start-Sleep -Seconds 2
 $shell=New-Object -ComObject WScript.Shell
 [void]$shell.AppActivate($proc.Id)
 [void][SearchProbe]::SetForegroundWindow($hwnd)
 [SearchProbe]::Activate($hwnd)
 Start-Sleep -Milliseconds 200
 $times=@()
 $generation=0
 for($round=0;$round -lt $Rounds;$round++) {
  if([SearchProbe]::GetForegroundWindow() -ne $hwnd){throw 'Test window lost focus; refusing to type into another app'}
  if($round -gt 0) {
   Send-TestKeys('{BACKSPACE 4}')
   $generation=Wait-Result '' 0 $generation
  }
  $timer=[Diagnostics.Stopwatch]::StartNew()
  Send-TestKeys('2026')
  $generation=Wait-Result '2026' 100 $generation
  $times+=$timer.Elapsed.TotalMilliseconds
 }
 if($Content) {
  Add-Type -AssemblyName System.Drawing
  $rect=New-Object SearchProbe+Rect
  [void][SearchProbe]::GetWindowRect($hwnd,[ref]$rect)
  $bitmap=New-Object Drawing.Bitmap ($rect.Right-$rect.Left),($rect.Bottom-$rect.Top)
  $graphics=[Drawing.Graphics]::FromImage($bitmap)
  try {
   $graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size)
   $bitmap.Save((Join-Path $base 'content-results.png'))
  } finally {$graphics.Dispose();$bitmap.Dispose()}
  if(-not $Folder) {
   Send-TestKeys('^a');Send-TestKeys('watcheruniquenectar')
   $generation=Wait-Result 'watcheruniquenectar' 0 $generation
   [IO.File]::WriteAllText((Join-Path $files 'note-0000.md'),'watcheruniquenectar')
   $generation=Wait-Result 'watcheruniquenectar' 1 $generation 10000
   Write-Output 'Live text edit notification: PASS'
  }
 }
 Send-TestKeys('^a');Send-TestKeys('unmatched-fixture-name')
 $generation=Wait-Result 'unmatched-fixture-name' 0 $generation
 $proc.Refresh();$cpu=$proc.TotalProcessorTime.TotalSeconds
 $idle=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 10;$proc.Refresh()
 $idlePercent=100*($proc.TotalProcessorTime.TotalSeconds-$cpu)/$idle.Elapsed.TotalSeconds
 # Visible windows can repaint due to pointer/caret/compositor activity. Report that
 # sample, then assert the hidden idle budget to detect a persistent polling loop.
 if([SearchProbe]::GetForegroundWindow() -ne $hwnd){throw 'Test window lost focus before Escape'}
 Send-TestKeys('{ESC}');Start-Sleep -Milliseconds 300
 if(-not [SearchProbe]::IsWindowVisible($hwnd)){throw 'Escape hid a nonempty search'}
 $generation=Wait-Result '' 0 $generation
 Send-TestKeys('{ESC}');Start-Sleep -Milliseconds 300
 if([SearchProbe]::IsWindowVisible($hwnd)){throw 'Escape did not hide the search window'}
 $proc.Refresh();$cpu=$proc.TotalProcessorTime.TotalSeconds;$idle.Restart()
 Start-Sleep -Seconds 10;$proc.Refresh()
 $hiddenPercent=100*($proc.TotalProcessorTime.TotalSeconds-$cpu)/$idle.Elapsed.TotalSeconds
 if($hiddenPercent -gt 2){throw "Hidden search is using excessive idle CPU: $hiddenPercent percent of one core"}
 $second=Start-Process -FilePath $Exe -PassThru
 if(-not $second.WaitForExit(5000)){Stop-Process -Id $second.Id;throw 'Second launch did not reuse the existing instance'}
 Start-Sleep -Milliseconds 200
 if(-not [SearchProbe]::IsWindowVisible($hwnd)){throw 'Second launch did not reopen the hidden instance'}
 Write-Output 'Escape and reopening the same instance: PASS'
 [pscustomobject]@{Rounds=$Rounds;MaxInputToResultMs=($times | Measure-Object -Maximum).Maximum;MeanInputToResultMs=($times | Measure-Object -Average).Average;SettledIdleCpuOneCorePercent=$idlePercent;HiddenIdleCpuOneCorePercent=$hiddenPercent;WorkingSetMiB=[math]::Round($proc.WorkingSet64/1MB,2);Content=[bool]$Content;Fixture=$base} | ConvertTo-Json
} finally {
 if(-not $proc.HasExited){Stop-Process -Id $proc.Id}
 $env:NOT_BAD_SEARCH_DATA=$oldData;$env:NOT_BAD_SEARCH_TRACE=$oldTrace
}
