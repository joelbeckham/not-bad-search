param([string]$Exe="$PSScriptRoot\..\target\clipboard-fix\release\good-search.exe")
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public class DesignProbe {
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
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
 public static IntPtr Find(uint id){IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==id){var s=new System.Text.StringBuilder(256);GetWindowText(h,s,256);if(s.ToString()=="Good Search")found=h;}return true;},IntPtr.Zero);return found;}
 public static void Click(IntPtr h,int x,int y){Rect r;GetWindowRect(h,out r);double scale=GetDpiForWindow(h)/96.0;SetCursorPos(r.L+(int)(x*scale),r.T+(int)(y*scale));mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
}
'@
[void][DesignProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=[IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\design-$([guid]::NewGuid().ToString('N'))")
$data=Join-Path $base 'data';$files=Join-Path $base 'files'
New-Item -ItemType Directory -Force $data,$files | Out-Null
foreach($name in @('2026 Annual Plan.md','2026 Product Roadmap.pdf','2026 Research Notes.txt','2026 Budget.xlsx','2026 Team Photo.png')){[IO.File]::WriteAllText((Join-Path $files $name),'')}
@"
folders = ['$files']
exclusions = ['*.tmp', 'node_modules']
reconcile_seconds = 3600
"@ | Set-Content "$data\config.toml"
$old=$env:GOOD_SEARCH_DATA;$env:GOOD_SEARCH_DATA=$data
$oldTrace=$env:GOOD_SEARCH_TRACE;$env:GOOD_SEARCH_TRACE='1'
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
 [Windows.Forms.SendKeys]::SendWait('^a');Start-Sleep -Milliseconds 150;[Windows.Forms.SendKeys]::SendWait('2026')
 $deadline=[DateTime]::UtcNow.AddSeconds(3)
 do {Start-Sleep -Milliseconds 50;$trace=Get-Content "$data\ui-trace.log" -ErrorAction SilentlyContinue} until(($trace -match 'query="2026" hits=5') -or [DateTime]::UtcNow -gt $deadline)
 if(-not ($trace -match 'query="2026" hits=5')){throw 'Expected search results were not rendered'}
 Capture 'search'
 [Windows.Forms.SendKeys]::SendWait('{DOWN 4}');Start-Sleep -Milliseconds 200;Capture 'selection'
 [DesignProbe]::Click($script:h,60,475);Start-Sleep -Milliseconds 300;Capture 'settings'
 $before=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$before)
 $scale=[DesignProbe]::GetDpiForWindow($script:h)/96.0
 $x=$before.L+[int](34*$scale);$y=$before.T+[int](38*$scale)
 [void][DesignProbe]::SetCursorPos($x,$y);[DesignProbe]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 100
 [void][DesignProbe]::SetCursorPos(($x+30),($y+20));Start-Sleep -Milliseconds 150
 [void][DesignProbe]::SetCursorPos(($x+130),($y+80));Start-Sleep -Milliseconds 150
 [DesignProbe]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 200
 $after=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$after)
 if($after.L -eq $before.L -and $after.T -eq $before.T){throw 'Search icon did not drag the window'}
 Write-Output 'Search icon window dragging: PASS'
 Write-Output "Window captures for visual inspection: $base"
} finally {if(-not $proc.HasExited){Stop-Process -Id $proc.Id};$env:GOOD_SEARCH_DATA=$old;$env:GOOD_SEARCH_TRACE=$oldTrace}
