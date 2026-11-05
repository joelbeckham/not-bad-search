param([Parameter(Mandatory=$true)][string]$DataFolder,[string]$Exe="")
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
$data=(Resolve-Path -LiteralPath $DataFolder).Path
$testRoot=[IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\")
if(-not $data.StartsWith($testRoot,[StringComparison]::OrdinalIgnoreCase)){throw 'Use an isolated test-data profile'}
$base=Split-Path $data -Parent
$files=Join-Path $base 'files'
@"
folders = ['$files']
image_folders = ['$files']
image_model = "gpt-5.6-terra"
image_daily_requests = 0
reconcile_seconds = 30
"@ | Set-Content "$data\config.toml"
$old=$env:GOOD_SEARCH_DATA;$env:GOOD_SEARCH_DATA=$data
$oldTrace=$env:GOOD_SEARCH_TRACE;$env:GOOD_SEARCH_TRACE='1'
Remove-Item -LiteralPath "$data\ui-trace.log" -ErrorAction SilentlyContinue
$proc=Start-Process -FilePath $Exe -PassThru
function Capture([string]$name){
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Test window lost focus; refusing to capture another app'}
 $r=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$r)
 $bitmap=New-Object Drawing.Bitmap ($r.R-$r.L),($r.B-$r.T)
 $graphics=[Drawing.Graphics]::FromImage($bitmap)
 $graphics.CopyFromScreen($r.L,$r.T,0,0,$bitmap.Size)
 $bitmap.Save("$base\$name.png");$graphics.Dispose();$bitmap.Dispose()
}
try {
 $limit=[DateTime]::UtcNow.AddSeconds(20)
 do{Start-Sleep -Milliseconds 100;$script:h=[DesignProbe]::Find($proc.Id)}until($script:h -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $limit)
 if($script:h -eq [IntPtr]::Zero){throw 'Window missing'}
 Write-Output ('Native style: 0x{0:X}' -f [DesignProbe]::GetWindowLongPtrW($script:h,-16).ToInt64())
 [void][DesignProbe]::SetWindowPos($script:h,[IntPtr](-1),0,0,0,0,0x43)
 Start-Sleep -Seconds 1
 [DesignProbe]::Click($script:h,130,36);Start-Sleep -Milliseconds 200
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Could not focus test window'}
 [Windows.Forms.SendKeys]::SendWait('^a');Start-Sleep -Milliseconds 150;[Windows.Forms.SendKeys]::SendWait('marmalade')
 $deadline=[DateTime]::UtcNow.AddSeconds(3)
 do {Start-Sleep -Milliseconds 50;$trace=Get-Content "$data\ui-trace.log" -ErrorAction SilentlyContinue} until(($trace -match 'query="marmalade" hits=1') -or [DateTime]::UtcNow -gt $deadline)
 if(-not ($trace -match 'query="marmalade" hits=1')){throw 'Expected search results were not rendered'}
 Capture 'search'
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Focus lost before context menu'}
 $r=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$r)
 $scale=[DesignProbe]::GetDpiForWindow($script:h)/96.0
 [void][DesignProbe]::SetCursorPos(($r.L+[int](30*$scale)),($r.T+[int](140*$scale)))
 Start-Sleep -Milliseconds 150
 [DesignProbe]::mouse_event(8,0,0,0,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 80
 [DesignProbe]::mouse_event(16,0,0,0,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 300
 Capture 'context-menu'
 [DesignProbe]::Click($script:h,90,158)
 Start-Sleep -Seconds 1
 Capture 'inspector'
 [DesignProbe]::Click($script:h,60,475)
 Start-Sleep -Milliseconds 300
 Capture 'back-to-search'
 [void][DesignProbe]::SetCursorPos(($r.L+[int](30*$scale)),($r.T+[int](140*$scale)))
 Start-Sleep -Milliseconds 150
 [DesignProbe]::mouse_event(8,0,0,0,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 80
 [DesignProbe]::mouse_event(16,0,0,0,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 300
 [DesignProbe]::Click($script:h,90,158)
 Start-Sleep -Milliseconds 500
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Focus lost before inspector Escape'}
 [Windows.Forms.SendKeys]::SendWait('{ESC}')
 Start-Sleep -Milliseconds 300
 if(-not [DesignProbe]::IsWindowVisible($script:h)){throw 'Escape hid the app instead of closing the inspector'}
 [DesignProbe]::Click($script:h,60,475);Start-Sleep -Milliseconds 300
 $r=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$r)
 $scale=[DesignProbe]::GetDpiForWindow($script:h)/96.0
 [void][DesignProbe]::SetCursorPos(($r.L+[int](500*$scale)),($r.T+[int](300*$scale)))
 [DesignProbe]::mouse_event(0x800,0,0,4294966576,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 500
 Capture 'image-settings'
 if([DesignProbe]::GetForegroundWindow() -ne $script:h){throw 'Focus lost before Escape'}
 [Windows.Forms.SendKeys]::SendWait('{ESC}')
 Start-Sleep -Milliseconds 300
 if(-not [DesignProbe]::IsWindowVisible($script:h)){throw 'Escape hid the app instead of leaving Settings'}
 Capture 'escape-back-from-settings'
 [Windows.Forms.SendKeys]::SendWait('{ESC}')
 Start-Sleep -Milliseconds 300
 if(-not [DesignProbe]::IsWindowVisible($script:h)){throw 'Escape hid the app instead of clearing the search'}
 $trace=Get-Content "$data\ui-trace.log"
 if(-not ($trace -match 'query="" hits=0')){throw 'Escape did not clear the query and results'}
 Capture 'escape-cleared-search'
 [Windows.Forms.SendKeys]::SendWait('{ESC}')
 Start-Sleep -Milliseconds 300
 if([DesignProbe]::IsWindowVisible($script:h)){throw 'Escape did not hide the empty search'}
 $proc.Refresh();$cpu=$proc.TotalProcessorTime.TotalSeconds
 $timer=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 10;$proc.Refresh()
 $idle=100*($proc.TotalProcessorTime.TotalSeconds-$cpu)/$timer.Elapsed.TotalSeconds
 if($idle -gt 2){throw "Hidden idle CPU exceeds budget: $idle"}
 [pscustomobject]@{ImageResult='PASS';Escape='PASS';HiddenCpuOneCorePercent=$idle;WorkingSetMiB=$proc.WorkingSet64/1MB;Captures=$base}|ConvertTo-Json
} finally {if(-not $proc.HasExited){Stop-Process -Id $proc.Id};$env:GOOD_SEARCH_DATA=$old;$env:GOOD_SEARCH_TRACE=$oldTrace}
