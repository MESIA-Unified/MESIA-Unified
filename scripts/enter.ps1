# Click the mesia window center to grant real focus, then press Enter.
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class MesiaEnter {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$proc = Get-Process mesia -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Error "no mesia window"; exit 1 }
$r = New-Object MesiaEnter+RECT
[void][MesiaEnter]::GetWindowRect($proc.MainWindowHandle, [ref]$r)
$cx = [int](($r.Left + $r.Right) / 2); $cy = [int](($r.Top + $r.Bottom) / 2)
[void][MesiaEnter]::SetCursorPos($cx, $cy)
Start-Sleep -Milliseconds 150
# real click on the window body: grants focus like a user does
[MesiaEnter]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 60
[MesiaEnter]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 300
[void][MesiaEnter]::SetForegroundWindow($proc.MainWindowHandle)
Start-Sleep -Milliseconds 300
[MesiaEnter]::keybd_event(0x0D, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 120
[MesiaEnter]::keybd_event(0x0D, 0, 2, [UIntPtr]::Zero)
Write-Output "done: click($cx,$cy) + Enter"
