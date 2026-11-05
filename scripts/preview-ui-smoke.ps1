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
 public static void Click(IntPtr h,int x,int y){Rect r;GetWindowRect(h,out r);double scale=GetDpiForWindow(h)/96.0;SetCursorPos(r.L+(int)(x*scale),r.T+(int)(y*scale));System.Threading.Thread.Sleep(100);mouse_event(2,0,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(80);mouse_event(4,0,0,0,UIntPtr.Zero);}
}
'@
[void][DesignProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=Join-Path (Join-Path $PSScriptRoot '..\test-data') ('preview-'+[guid]::NewGuid().ToString('N'))
$base=[IO.Path]::GetFullPath($base)
$data=Join-Path $base 'data';$files=Join-Path $base 'files'
New-Item -ItemType Directory -Path $data,$files | Out-Null
$bitmap=New-Object Drawing.Bitmap 800,400
$graphics=[Drawing.Graphics]::FromImage($bitmap)
$graphics.Clear([Drawing.Color]::FromArgb(30,55,65))
$font=New-Object Drawing.Font 'Segoe UI',30
$graphics.DrawString('Preview fixture image',$font,[Drawing.Brushes]::White,35,145)
$bitmap.Save((Join-Path $files 'previewfixture-a.png'))
$font.Dispose();$graphics.Dispose();$bitmap.Dispose()
$text=(1..100 | ForEach-Object {if($_ -eq 40){'previewfixture: highlighted matching line'}else{"Line $_ of this local text preview"}}) -join "`n"
[IO.File]::WriteAllText((Join-Path $files 'previewfixture-b.txt'),$text)
[IO.File]::WriteAllText((Join-Path $files 'previewfixture-c.txt'),'previewfixture: another file')
@("folders = ['$files']",'image_daily_requests = 0','semantic_enabled = false','reconcile_seconds = 300') | Set-Content "$data\config.toml"
$old=$env:GOOD_SEARCH_DATA;$oldTrace=$env:GOOD_SEARCH_TRACE;$oldWorker=$env:GOOD_SEARCH_WORKER_TRACE
$env:GOOD_SEARCH_DATA=$data;$env:GOOD_SEARCH_TRACE='1';$env:GOOD_SEARCH_WORKER_TRACE='1'
$proc=Start-Process -FilePath $Exe -PassThru
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
 Start-Sleep -Seconds 1
 [DesignProbe]::Click($script:h,130,36);Keys('^a');Keys('previewfixture')
 $null=Wait-Trace 'accepted .*hits=3'
 Start-Sleep -Milliseconds 100
 Capture 'results'
 $timer=[Diagnostics.Stopwatch]::StartNew();Keys('{ENTER}')
 $null=Wait-Trace 'preview ready .*previewfixture-a.png.*success=true'
 $imageMs=$timer.Elapsed.TotalMilliseconds
 Start-Sleep -Milliseconds 100;Capture 'image-preview'
 Clear-Trace;Keys('{DOWN}')
 $null=Wait-Trace 'preview ready .*previewfixture-b.txt.*success=true'
 Start-Sleep -Milliseconds 100;Capture 'text-preview'
 $r=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$r)
 $scale=[DesignProbe]::GetDpiForWindow($script:h)/96.0
 [void][DesignProbe]::SetCursorPos(($r.L+[int](500*$scale)),($r.T+[int](300*$scale)))
 [DesignProbe]::mouse_event(0x800,0,0,4294966936,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 300;Capture 'text-scrolled'
 Clear-Trace;Keys('{DOWN}')
 $null=Wait-Trace 'preview ready .*previewfixture-c.txt.*success=true'
 Clear-Trace;Keys('{UP}')
 $null=Wait-Trace 'preview ready .*previewfixture-b.txt.*success=true'
 Keys('^,');Start-Sleep -Milliseconds 200;Keys('{ESC}');Start-Sleep -Milliseconds 200
 Capture 'preview-after-settings'
 Keys('{ESC}');Start-Sleep -Milliseconds 200;Capture 'returned-list'
 # Click the filename itself, not only blank row space.
 Clear-Trace;[DesignProbe]::Click($script:h,140,190)
 $null=Wait-Trace 'preview ready .*success=true'
 Start-Sleep -Milliseconds 100;Capture 'clicked-preview'
 Keys('{ESC}');Start-Sleep -Milliseconds 200
 # Return to first result, then rapidly browse, checking that the final request wins.
 Keys('{UP}');Keys('{UP}');Keys('{ENTER}');Start-Sleep -Milliseconds 100
 Clear-Trace;Keys('{DOWN}');Keys('{DOWN}');Keys('{UP}')
 $null=Wait-Trace 'preview ready .*previewfixture-b.txt.*success=true'
 Start-Sleep -Milliseconds 100;Capture 'rapid-navigation'
 if(-not $SkipShellOpen){
 # Exercise real shell activation using a synthetic text file.
 Clear-Trace;Keys('{ENTER}')
 $null=Wait-Trace 'preview open .*success=true'
 Start-Sleep -Milliseconds 500
 [void][DesignProbe]::SetWindowPos($script:h,[IntPtr](-1),0,0,0,0,0x43)
 [DesignProbe]::Click($script:h,50,35);Keys('{ESC}');Start-Sleep -Milliseconds 200
 Clear-Trace;Keys('^{ENTER}')
 $null=Wait-Trace 'direct open .*success=true'
 [void][DesignProbe]::SetWindowPos($script:h,[IntPtr](-1),0,0,0,0,0x43)
 [DesignProbe]::Click($script:h,130,36)
 }else{Keys('{ESC}');Start-Sleep -Milliseconds 200}
 Keys('{ESC}');Start-Sleep -Milliseconds 150;Keys('{ESC}')
 Start-Sleep -Milliseconds 200
 if([DesignProbe]::IsWindowVisible($script:h)){throw 'Empty search did not hide'}
 $proc.Refresh();$cpu=$proc.TotalProcessorTime.TotalSeconds
 $timer=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 5;$proc.Refresh()
 [pscustomobject]@{Preview='PASS';Navigation='PASS';DefaultApp=$(if($SkipShellOpen){'Skipped'}else{'PASS'});ImageReadyMs=$imageMs;HiddenCpuOneCorePercent=100*($proc.TotalProcessorTime.TotalSeconds-$cpu)/$timer.Elapsed.TotalSeconds;WorkingSetMiB=$proc.WorkingSet64/1MB;Captures=$base}|ConvertTo-Json
}finally{
 if(-not $proc.HasExited){Stop-Process -Id $proc.Id}
 $env:GOOD_SEARCH_DATA=$old;$env:GOOD_SEARCH_TRACE=$oldTrace;$env:GOOD_SEARCH_WORKER_TRACE=$oldWorker
}