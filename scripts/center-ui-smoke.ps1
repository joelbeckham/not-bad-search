param([string]$Exe = "$PSScriptRoot\..\target\release\good-search.exe")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class NativeProbe {
 public delegate bool EnumProc(IntPtr hwnd,IntPtr param);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb,IntPtr param);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd,System.Text.StringBuilder text,int count);
 public static IntPtr OwnedWindow(uint pid) {IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var s=new System.Text.StringBuilder(256);GetWindowText(h,s,256);if(s.ToString()=="Good Search" && IsWindowVisible(h))found=h;}return true;},IntPtr.Zero);return found;}
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string cls,string title);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern void keybd_event(byte vk,byte scan,uint flags,UIntPtr extra);
 [DllImport("user32.dll")] public static extern bool PostThreadMessage(uint thread,uint message,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd,uint msg,IntPtr w,IntPtr l);
 public static void Key(byte key) { keybd_event(key,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(100);keybd_event(key,0,2,UIntPtr.Zero); }
 public static void Hotkey() {keybd_event(0x11,0,0,UIntPtr.Zero);keybd_event(0x12,0,0,UIntPtr.Zero);System.Threading.Thread.Sleep(100);Key(0x20);System.Threading.Thread.Sleep(100);keybd_event(0x12,0,2,UIntPtr.Zero);keybd_event(0x11,0,2,UIntPtr.Zero);}
}
'@
Add-Type @"
using System;using System.Runtime.InteropServices;
public class CenterProbe {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int L,T,R,B;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct MonitorInfo {public int Size;public Rect Monitor,Work;public uint Flags;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point p);
 [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(Point p,uint f);
 [DllImport("user32.dll")] public static extern bool GetMonitorInfoW(IntPtr h,ref MonitorInfo m);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
}
"@
[void][CenterProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
$base=[IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\center-$([guid]::NewGuid().ToString('N'))")
$data=Join-Path $base 'data';$files=Join-Path $base 'files'
New-Item -ItemType Directory -Path $data,$files | Out-Null
@("folders = ['$files']",'reconcile_seconds = 300')|Set-Content "$data\config.toml"
$old=$env:GOOD_SEARCH_DATA;$env:GOOD_SEARCH_DATA=$data;$env:GOOD_SEARCH_TRACE='1';$env:GOOD_SEARCH_WORKER_TRACE='1'
$proc=Start-Process -FilePath $Exe -PassThru
function Check-Center([string]$stage){
 Start-Sleep -Milliseconds 1500
 $r=New-Object CenterProbe+Rect;[void][CenterProbe]::GetWindowRect($script:h,[ref]$r)
 $dx=[double]::MaxValue;$dy=[double]::MaxValue
 foreach($screen in [Windows.Forms.Screen]::AllScreens){
  $area=$screen.WorkingArea
  $x=[math]::Abs(($r.L+$r.R-$area.Left-$area.Right)/2.0)
  $y=[math]::Abs(($r.T+$r.B-$area.Top-$area.Bottom)/2.0)
  if(($x+$y) -lt ($dx+$dy)){$dx=$x;$dy=$y}
 }
 if($dx -gt 2 -or $dy -gt 2){throw "$stage off center: dx=$dx dy=$dy window=$r"}
 Write-Output "$stage centered: dx=$dx dy=$dy"
}
try {
 $limit=[DateTime]::UtcNow.AddSeconds(20)
 do{Start-Sleep -Milliseconds 100;$script:h=[NativeProbe]::OwnedWindow($proc.Id)}until($script:h -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $limit)
 if($script:h -eq [IntPtr]::Zero){throw 'Window missing'}
 Check-Center 'Startup'
 [void][NativeProbe]::SetForegroundWindow($script:h)
 Start-Sleep -Milliseconds 300
 [void][CenterProbe]::SetWindowPos($script:h,[IntPtr]::Zero,40,60,0,0,0x15)
 foreach($thread in $proc.Threads){[void][NativeProbe]::PostThreadMessage($thread.Id,0x312,[IntPtr]1,[IntPtr]::Zero)};Start-Sleep -Milliseconds 300
 Get-Content "$data\worker-trace.log" -ErrorAction SilentlyContinue
 Check-Center 'Hotkey message while visible'

 [void][NativeProbe]::ShowWindow($script:h,0);Start-Sleep -Milliseconds 200
 if([NativeProbe]::IsWindowVisible($script:h)){throw 'Escape did not hide empty search'}
 [void][CenterProbe]::SetWindowPos($script:h,[IntPtr]::Zero,90,70,0,0,0x15)
 foreach($thread in $proc.Threads){[void][NativeProbe]::PostThreadMessage($thread.Id,0x312,[IntPtr]1,[IntPtr]::Zero)};Check-Center 'Hotkey message from hidden'
 [void][CenterProbe]::SetWindowPos($script:h,[IntPtr]::Zero,30,80,0,0,0x15)
 $second=Start-Process -FilePath $Exe -PassThru
 if(-not $second.WaitForExit(5000)){Stop-Process -Id $second.Id;throw 'Second instance did not exit'}
 Check-Center 'Second launch'
 Write-Output 'CENTERING PASS'
}finally{if(-not $proc.HasExited){Stop-Process -Id $proc.Id};$env:GOOD_SEARCH_DATA=$old}