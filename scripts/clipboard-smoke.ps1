param([string]$Exe = "$PSScriptRoot\..\target\clipboard-fix\release\good-search.exe")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class ClipboardProbe {
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 public delegate bool EnumProc(IntPtr h,IntPtr p);
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int L,T,R,B;}
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb,IntPtr p);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,System.Text.StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point p);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point p);
 public static IntPtr Window(uint pid) {IntPtr found=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var s=new System.Text.StringBuilder(256);GetWindowText(h,s,256);if(s.ToString()=="Good Search")found=h;}return true;},IntPtr.Zero);return found;}
 public static void Click(IntPtr h,int x,int y) {double scale=GetDpiForWindow(h)/96.0;Rect r;GetWindowRect(h,out r);SetCursorPos(r.L+(int)(x*scale),r.T+(int)(y*scale));mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
}
'@
[void][ClipboardProbe]::SetThreadDpiAwarenessContext([IntPtr](-4))
# Preserve available clipboard formats without printing their contents.
$saved = New-Object Windows.Forms.DataObject
$original = [Windows.Forms.Clipboard]::GetDataObject()
if ($null -ne $original) {
 foreach ($format in $original.GetFormats($false)) {
  $value = $original.GetData($format,$false)
  if ($null -ne $value) {$saved.SetData($format,$false,$value)}
 }
}
$cursor = New-Object ClipboardProbe+Point
[void][ClipboardProbe]::GetCursorPos([ref]$cursor)
$data = [IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\clipboard-$([guid]::NewGuid().ToString('N'))")
New-Item -ItemType Directory -Path $data | Out-Null
$previousData=$env:GOOD_SEARCH_DATA
$env:GOOD_SEARCH_DATA=$data
$proc=$null
function Keys([string]$keys) {
 [Windows.Forms.SendKeys]::SendWait($keys)
 Start-Sleep -Milliseconds 150
}
function RoundTrip([string]$label,[int]$y,[string]$value) {
 if ($y -ge 0) {[ClipboardProbe]::Click($script:hwnd,120,$y)} elseif ($y -eq -2) {Keys '+{TAB}';Keys '+{TAB}'} else {Keys '{TAB}'}
 Start-Sleep -Milliseconds 250
 if ([ClipboardProbe]::GetForegroundWindow() -ne $script:hwnd) {throw "Could not focus test window for $label"}
 Keys '^a'
 [Windows.Forms.Clipboard]::SetText($value)
 Keys '^v'
 # Replace the OS clipboard to prove Ctrl+C reads the widget, not our seed.
 [Windows.Forms.Clipboard]::SetText('copy-sentinel')
 Keys '^a'
 Keys '^c'
 $actual=[Windows.Forms.Clipboard]::GetText().Replace("`r`n","`n")
 if ($actual -ne $value.Replace("`r`n","`n")) {throw "$label copy/paste round trip failed"}
 [Windows.Forms.Clipboard]::SetText('cut-sentinel')
 Keys '^x'
 if ([Windows.Forms.Clipboard]::GetText().Replace("`r`n","`n") -ne $value.Replace("`r`n","`n")) {throw "$label cut failed"}
 Keys '^v'
 Keys '^a'
 [Windows.Forms.Clipboard]::SetText('restore-sentinel')
 Keys '^c'
 if ([Windows.Forms.Clipboard]::GetText().Replace("`r`n","`n") -ne $value.Replace("`r`n","`n")) {throw "$label paste after cut failed"}
 Write-Output "$label clipboard copy/cut/paste: PASS"
}
try {
 $proc=Start-Process -FilePath $Exe -PassThru
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 do {Start-Sleep -Milliseconds 100;$script:hwnd=[ClipboardProbe]::Window($proc.Id)} until ($script:hwnd -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $deadline)
 if ($script:hwnd -eq [IntPtr]::Zero) {throw 'Test window did not appear'}
 [void][ClipboardProbe]::SetWindowPos($script:hwnd,[IntPtr](-1),0,0,0,0,0x43)
 Start-Sleep -Seconds 1
 RoundTrip 'Selected folders' 180 "C:\Clipboard Test\notes`n\\wsl.localhost\Ubuntu-24.04\home\test\notes"
 RoundTrip 'Exclusions' -1 "*.tmp`nprivate/**"
 RoundTrip 'Search' 36 'clipboard-search-proof'
} finally {
 if ($null -ne $proc -and -not $proc.HasExited) {Stop-Process -Id $proc.Id}
 if ($null -ne $original) {[Windows.Forms.Clipboard]::SetDataObject($saved,$true)} else {[Windows.Forms.Clipboard]::Clear()}
 [void][ClipboardProbe]::SetCursorPos($cursor.X,$cursor.Y)
 $env:GOOD_SEARCH_DATA=$previousData
}
