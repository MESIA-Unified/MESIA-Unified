# Capture the mesia.exe game window (or full screen fallback) to a PNG.
# Usage: shot.ps1 [-Out path.png] [-Full]
param(
    [string]$Out = "$env:TEMP\mesia-shot.png",
    [switch]$Full,
    [switch]$PrintWindow
)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class MesiaWin {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$proc = @(Get-Process mesia -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1)
if ($proc.Count -gt 0 -and -not $Full) {
    $proc = $proc[0]
    $r = [MesiaWin+RECT]::new()
    [void][MesiaWin]::GetWindowRect([IntPtr]$proc.MainWindowHandle, [ref]$r)
    Write-Output "debug: proc=$($proc.Id) rtype=$($r.GetType().FullName) L=$($r.Left) R=$($r.Right)"
    $x = $r.Left
    $y = $r.Top
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
} else {
    Add-Type -AssemblyName System.Windows.Forms
    $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $x = $b.Left
    $y = $b.Top
    $w = $b.Width
    $h = $b.Height
}
if ($w -le 0 -or $h -le 0) { Write-Error "bad rect $w x $h"; exit 1 }
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
if ($proc.Count -gt 0 -and -not $Full -and $PrintWindow) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
public class MesiaPw {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
}
"@
    $hdc = $g.GetHdc()
    [void][MesiaPw]::PrintWindow([IntPtr]$proc.MainWindowHandle, $hdc, 2)
    $g.ReleaseHdc($hdc)
} else {
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "saved $Out ($w x $h) proc=$($proc.Id)"
