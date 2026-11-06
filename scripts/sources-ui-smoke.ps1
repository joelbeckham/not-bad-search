param([string]$Exe="",[switch]$SkipShellOpen)
$ErrorActionPreference='Stop'
if(-not $Exe){$Exe=Join-Path $PSScriptRoot '..\target\content-index\release\good-search.exe'}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public class DesignProbe {
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 public delegate bool Callback(IntPtr h,IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int L,T,R,B;}
 [DllImport("user32.dll")] public static extern bool EnumWindows(Callback c,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint id);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,System.Text.StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h,int n);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 public static IntPtr Find(uint id){IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==id){var s=new System.Text.StringBuilder(256);GetWindowText(h,s,256);if(s.ToString()=="Good Search" && IsWindowVisible(h))found=h;}return true;},IntPtr.Zero);return found;}
 public static void Click(IntPtr h,int x,int y){Rect r;GetWindowRect(h,out r);double scale=(r.R-r.L)/1014.0;SetCursorPos(r.L+(int)(x*scale),r.T+(int)(y*scale));System.Threading.Thread.Sleep(100);mouse_event(2,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(80);mouse_event(4,0,0,0,UIntPtr.Zero);}
}
'@
[void][DesignProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=[IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\icons-ui-$([guid]::NewGuid().ToString('N'))")
$data=Join-Path $base 'data';$files=Join-Path $base 'files'
New-Item -ItemType Directory -Path $data,$files | Out-Null
foreach($name in @('sample.md','sample.png','sample.pdf','sample.rs','sample.zip')){
 [IO.File]::WriteAllText((Join-Path $files $name),'Synthetic filename fixture; content indexing disabled.')
}
@("folders = ['$files']","content_folders = ['$files']",'semantic_enabled = false','image_daily_requests = 0','reconcile_seconds = 300') | Set-Content "$data\config.toml"
$old=$env:GOOD_SEARCH_DATA;$oldTrace=$env:GOOD_SEARCH_TRACE;$oldWorker=$env:GOOD_SEARCH_WORKER_TRACE
$env:GOOD_SEARCH_DATA=$data;$env:GOOD_SEARCH_TRACE='1';$env:GOOD_SEARCH_WORKER_TRACE='1'
$proc=Start-Process -FilePath $Exe -WindowStyle Hidden -PassThru
function Keys([string]$keys){
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Test lost focus; refusing input'}
 [Windows.Forms.SendKeys]::SendWait($keys)
}
function Capture([string]$name){
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Test lost focus; refusing capture'}
 $r=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$r)
 $bitmap=New-Object Drawing.Bitmap ($r.R-$r.L),($r.B-$r.T)
 $graphics=[Drawing.Graphics]::FromImage($bitmap);$graphics.CopyFromScreen($r.L,$r.T,0,0,$bitmap.Size)
 $bitmap.Save("$base\$name.png");$graphics.Dispose();$bitmap.Dispose()
}
function Wait-Trace([string]$pattern){
 $timer=[Diagnostics.Stopwatch]::StartNew()
 do{
  $trace=Get-Content "$data\worker-trace.log" -ErrorAction SilentlyContinue
  if($trace -match $pattern){return $timer.Elapsed.TotalMilliseconds}
  Start-Sleep -Milliseconds 20
 }while($timer.Elapsed.TotalSeconds -lt 5)
 throw "Missing trace: $pattern"
}
function Clear-Trace {Clear-Content "$data\worker-trace.log"}
try {
 $limit=[DateTime]::UtcNow.AddSeconds(20)
 do{Start-Sleep -Milliseconds 100;$script:h=[DesignProbe]::Find($proc.Id)}until($script:h -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $limit)
 if($script:h -eq [IntPtr]::Zero){throw 'Window missing'}
 [void][DesignProbe]::SetWindowPos($script:h,[IntPtr](-1),0,0,0,0,0x43)
 Start-Sleep -Milliseconds 500
 [DesignProbe]::Click($script:h,130,36)
 Keys('^,');Start-Sleep -Seconds 2;Capture 'sources'
 [DesignProbe]::Click($script:h,42,260);Start-Sleep -Milliseconds 250;Capture 'source-details'
 Keys('{ESC}');Start-Sleep -Milliseconds 150;Capture 'sources-after-back'
 [DesignProbe]::Click($script:h,198,105);Start-Sleep -Milliseconds 150;Capture 'search-settings'
 [DesignProbe]::Click($script:h,272,105);Start-Sleep -Milliseconds 150;Capture 'processing-settings'
 [DesignProbe]::Click($script:h,48,143);Start-Sleep -Milliseconds 250
 $saved=Get-Content "$data\config.toml" -Raw
 if($saved -notmatch '\[\[sources\]\]' -or $saved -notmatch 'kind = "folder"'){throw 'Source migration was not persisted'}
 Keys('{ESC}');Start-Sleep -Milliseconds 100;Keys('sample')
 $null=Wait-Trace 'accepted id=.*hits=5'
 Write-Output "PASS: tabs, source details, Escape navigation, saving migrated sources, search after settings. Captures: $base"
} finally {
 if(-not $proc.HasExited){Stop-Process -Id $proc.Id}
 $env:GOOD_SEARCH_DATA=$old;$env:GOOD_SEARCH_TRACE=$oldTrace;$env:GOOD_SEARCH_WORKER_TRACE=$oldWorker
}