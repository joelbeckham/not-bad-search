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
$base=[IO.Path]::GetFullPath("$PSScriptRoot\..\test-data\semantic-ui-$([guid]::NewGuid().ToString('N'))")
$data=Join-Path $base 'data';$files=Join-Path $base 'files'
$model=Join-Path $data 'models\potion-8M-bf8b056651a2c21b8d2565580b8569da283cab23'
New-Item -ItemType Directory -Path $data,$files,$model | Out-Null
Copy-Item "$PSScriptRoot\..\test-data\embedding-model\*" -Destination $model
[IO.File]::WriteAllText("$files\animals.md",'A photograph of a golden retriever playing in the park')
[IO.File]::WriteAllText("$files\finances.md",'An invoice for a laptop purchase with a total amount due')
[IO.File]::WriteAllText("$files\meeting.md",'Meeting notes about database performance and query optimization')
[IO.File]::WriteAllText("$files\dessert.md",'A recipe for chocolate cake and vanilla frosting')
@("folders = ['$files']","content_folders = ['$files']",'semantic_enabled = true','semantic_delay_ms = 200','image_daily_requests = 0','reconcile_seconds = 300') | Set-Content "$data\config.toml"
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
 $null=Wait-Trace 'Local semantic index: 4 / 4 chunks'
 [DesignProbe]::Click($script:h,130,36)
 Clear-Trace
 $timer=[Diagnostics.Stopwatch]::StartNew();Keys('computer receipt');$typingMs=$timer.Elapsed.TotalMilliseconds
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 $firstMs=$timer.Elapsed.TotalMilliseconds;$firstAfterTypingMs=$firstMs-$typingMs
 Start-Sleep -Milliseconds 100;Capture 'semantic-match'
 $filterY=if((Get-Content "$data\ui-trace.log" -Raw).Contains('Hot key is already registered')){118}else{95}
 Clear-Trace;[DesignProbe]::Click($script:h,165,$filterY)
 $null=Wait-Trace 'accepted id=.*hits=0'
 Start-Sleep -Milliseconds 250;Capture 'semantic-off'
 Clear-Trace;[DesignProbe]::Click($script:h,165,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 Clear-Trace;[DesignProbe]::Click($script:h,33,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=0'
 Start-Sleep -Milliseconds 100;Capture 'text-off'
 Clear-Trace;[DesignProbe]::Click($script:h,33,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 Clear-Trace;[DesignProbe]::Click($script:h,88,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 Start-Sleep -Milliseconds 100;Capture 'images-off'
 Clear-Trace;[DesignProbe]::Click($script:h,88,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 Clear-Trace;[DesignProbe]::Click($script:h,250,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 Start-Sleep -Milliseconds 100;Capture 'semantic-only'
 Clear-Trace;[DesignProbe]::Click($script:h,250,$filterY)
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 $trace=Get-Content "$data\worker-trace.log"
 if(-not ($trace -match 'accepted id=.*hits=0')){throw 'Immediate lexical pass missing'}
 # Repeat query, then replace an in-flight query before its debounce expires.
 Keys('{ESC}');Start-Sleep -Milliseconds 100
 Clear-Trace;$timer.Restart();Keys('computer receipt');$typingMs=$timer.Elapsed.TotalMilliseconds
 $null=Wait-Trace 'semantic accepted .*query="computer receipt" hits=1'
 $repeatMs=$timer.Elapsed.TotalMilliseconds;$repeatAfterTypingMs=$repeatMs-$typingMs
 Keys('^a');Keys('dog outdoors');Keys('^a');Clear-Trace;Keys('baking dessert')
 $null=Wait-Trace 'semantic accepted .*query="baking dessert" hits=1'
 Start-Sleep -Milliseconds 200;Capture 'latest-query'
 $trace=Get-Content "$data\worker-trace.log"
 if($trace -match 'semantic accepted .*query="dog outdoors"'){throw 'Stale semantic query was applied'}
 Keys('{ENTER}');Start-Sleep -Milliseconds 300;Capture 'semantic-preview';Keys('{ESC}')
 # Quoted searches must not broaden to semantic-only matches.
 Keys('^a');Clear-Trace;Keys('"computer receipt"')
 $null=Wait-Trace 'semantic accepted .*computer receipt'
 Start-Sleep -Milliseconds 200;Capture 'quoted-search'
 $uiTrace=Get-Content "$data\ui-trace.log"
 if(-not ($uiTrace -match 'hits=0')){throw 'Expected empty strict phrase search'}
 # Clearing an unsettled query must cancel semantic updates.
 Keys('^a');Keys('slow SQL');Keys('{ESC}');Clear-Trace
 Start-Sleep -Milliseconds 500
 if((Get-Content "$data\worker-trace.log") -match 'semantic accepted'){throw 'Cleared query received semantic results'}
 Capture 'cleared-search'
 Keys('^,');Start-Sleep -Milliseconds 150
 $r=New-Object DesignProbe+Rect;[void][DesignProbe]::GetWindowRect($script:h,[ref]$r)
 $scale=[DesignProbe]::GetDpiForWindow($script:h)/96.0
 [void][DesignProbe]::SetCursorPos(($r.L+[int](500*$scale)),($r.T+[int](300*$scale)))
 [DesignProbe]::mouse_event(0x800,0,0,4294966576,[UIntPtr]::Zero)
 Start-Sleep -Milliseconds 300;Capture 'semantic-settings'
 Keys('{ESC}');Keys('{ESC}');Start-Sleep -Milliseconds 200
 if([DesignProbe]::IsWindowVisible($script:h)){throw 'Empty search did not hide'}
 $proc.Refresh();$cpu=$proc.TotalProcessorTime.TotalSeconds;$timer.Restart()
 Start-Sleep -Seconds 10;$proc.Refresh()
 [pscustomobject]@{Semantic='PASS';Debounce='PASS';QueryReplacement='PASS';FirstInputToSemanticMs=$firstMs;RepeatInputToSemanticMs=$repeatMs;FirstAfterTypingMs=$firstAfterTypingMs;RepeatAfterTypingMs=$repeatAfterTypingMs;HiddenCpuOneCorePercent=100*($proc.TotalProcessorTime.TotalSeconds-$cpu)/$timer.Elapsed.TotalSeconds;WorkingSetMiB=$proc.WorkingSet64/1MB;Captures=$base}|ConvertTo-Json
}finally{
 if(-not $proc.HasExited){Stop-Process -Id $proc.Id}
 $env:GOOD_SEARCH_DATA=$old;$env:GOOD_SEARCH_TRACE=$oldTrace;$env:GOOD_SEARCH_WORKER_TRACE=$oldWorker
}