param([string]$Exe = "$PSScriptRoot\..\target\release\not-bad-search.exe")
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class NativeProbe {
 public delegate bool EnumProc(IntPtr hwnd,IntPtr param);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb,IntPtr param);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,System.Text.StringBuilder text,int count);
 public static IntPtr OwnedWindow(uint pid) {IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var s=new System.Text.StringBuilder(256);GetWindowText(h,s,256);if(s.ToString()=="Not Bad Search" && IsWindowVisible(h))found=h;}return true;},IntPtr.Zero);return found;}
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls,string title);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd,uint msg,IntPtr w,IntPtr l);
 public static void Key(byte key) { keybd_event(key,0,0,UIntPtr.Zero);keybd_event(key,0,2,UIntPtr.Zero); }
 public static void Hotkey() {keybd_event(0x11,0,0,UIntPtr.Zero);keybd_event(0x12,0,0,UIntPtr.Zero);Key(0x20);keybd_event(0x12,0,2,UIntPtr.Zero);keybd_event(0x11,0,2,UIntPtr.Zero);}
}
'@
$base = [IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\ui-$([guid]::NewGuid().ToString('N'))")
$data = Join-Path $base 'data'
$files = Join-Path $base 'files'
New-Item -ItemType Directory -Force $data,$files | Out-Null
Set-Content -LiteralPath "$files\unique-smoke-report.md" -Value 'fixture'
@"
folders = ['$files']
exclusions = ['*.tmp']
semantic_enabled = false
reconcile_seconds = 30
"@ | Set-Content -LiteralPath "$data\config.toml"
$env:NOT_BAD_SEARCH_DATA=$data
$env:NOT_BAD_SEARCH_TRACE='1'
$proc=Start-Process -FilePath $Exe -PassThru
try {
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 do {Start-Sleep -Milliseconds 100; $hwnd=[NativeProbe]::OwnedWindow($proc.Id)} until ($hwnd -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $deadline)
 if ($hwnd -eq [IntPtr]::Zero) {throw 'Window did not appear'}
 $caption=New-Object System.Text.StringBuilder 256
 [void][NativeProbe]::GetWindowText($hwnd,$caption,256)
 Write-Output "Window title: $caption; handle: $hwnd"
 Start-Sleep -Seconds 2
 [NativeProbe]::Hotkey()
 Start-Sleep -Milliseconds 300
 [NativeProbe]::Key(0x1B)
 Start-Sleep -Milliseconds 300
 if ([NativeProbe]::IsWindowVisible($hwnd)) {
  Write-Output "Foreground: $([NativeProbe]::GetForegroundWindow()); trace: $(Get-Content "$data\ui-trace.log" -Raw)"
  throw 'Escape did not hide the window'
 }
 $times=@()
 for ($i=0;$i -lt 10;$i++) {
  $timer=[Diagnostics.Stopwatch]::StartNew()
  [NativeProbe]::Hotkey()
  while ((-not [NativeProbe]::IsWindowVisible($hwnd) -or [NativeProbe]::GetForegroundWindow() -ne $hwnd) -and $timer.ElapsedMilliseconds -lt 3000) {Start-Sleep -Milliseconds 1}
  if ($timer.ElapsedMilliseconds -ge 3000) {throw 'Global hotkey did not show and focus the window'}
  $times += $timer.Elapsed.TotalMilliseconds
  Start-Sleep -Milliseconds 100
  if ($i -lt 9) {[NativeProbe]::Key(0x1B);Start-Sleep -Milliseconds 200}
 }
 Add-Type -AssemblyName System.Windows.Forms
 $timer=[Diagnostics.Stopwatch]::StartNew()
 [Windows.Forms.SendKeys]::SendWait('unique-smoke')
 do {Start-Sleep -Milliseconds 10; $trace=Get-Content "$data\ui-trace.log" -ErrorAction SilentlyContinue} until (($trace -match 'query="unique-smoke" hits=1') -or $timer.ElapsedMilliseconds -gt 5000)
 if (-not ($trace -match 'query="unique-smoke" hits=1')) {throw 'Live filename result was not rendered'}
 $searchMs=$timer.Elapsed.TotalMilliseconds
 # Confirm native change notifications refresh the currently displayed query.
 Clear-Content "$data\ui-trace.log"
 Rename-Item -LiteralPath "$files\unique-smoke-report.md" -NewName 'renamed-report.md'
 $timer.Restart()
 do {Start-Sleep -Milliseconds 25; $trace=Get-Content "$data\ui-trace.log"} until (($trace -match 'query="unique-smoke" hits=0') -or $timer.ElapsedMilliseconds -gt 5000)
 if (-not ($trace -match 'query="unique-smoke" hits=0')) {throw 'Native rename did not refresh live results within 5 seconds'}
 $changeMs=$timer.Elapsed.TotalMilliseconds
 $proc.Refresh();$visibleCpu=$proc.TotalProcessorTime.TotalSeconds
 $timer.Restart();Start-Sleep -Seconds 10;$proc.Refresh()
 $visiblePercent=100*($proc.TotalProcessorTime.TotalSeconds-$visibleCpu)/$timer.Elapsed.TotalSeconds
 [void][NativeProbe]::PostMessage($hwnd,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
 Start-Sleep -Milliseconds 300
 if ([NativeProbe]::IsWindowVisible($hwnd)) {throw 'Close did not hide the window'}
 $proc.Refresh();$cpu=$proc.TotalProcessorTime.TotalSeconds
 $timer.Restart();Start-Sleep -Seconds 35
 $proc.Refresh()
 [pscustomobject]@{HotkeySamplesMs=$times;TypingToResultMs=$searchMs;NativeRenameToResultMs=$changeMs;WorkingSetMiB=$proc.WorkingSet64/1MB;PrivateMiB=$proc.PrivateMemorySize64/1MB;VisibleIdleCpuOneCorePercent=$visiblePercent;HiddenIdleCpuOneCorePercent=100*($proc.TotalProcessorTime.TotalSeconds-$cpu)/$timer.Elapsed.TotalSeconds;Fixture=$base} | ConvertTo-Json
} finally {
 if (-not $proc.HasExited) {Stop-Process -Id $proc.Id}
 Remove-Item Env:\NOT_BAD_SEARCH_DATA,Env:\NOT_BAD_SEARCH_TRACE -ErrorAction SilentlyContinue
}
