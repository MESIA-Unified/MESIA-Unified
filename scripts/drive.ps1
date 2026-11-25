# Drive the mesia.exe game window: focus, real key presses, real mouse moves.
# These go through SendInput/GetAsyncKeyState/GetCursorPos exactly like a user.
# Usage:
#   drive.ps1 focus                      -> bring the game window foreground
#   drive.ps1 key Z 300                  -> hold key Z for 300 ms (VK letter, or name)
#   drive.ps1 keys Z,S,Esc               -> tap several keys (120 ms each)
#   drive.ps1 move 40,-10 20             -> move cursor by dx,dy in N steps
#   drive.ps1 drag 500,0 1000            -> same over ~1 s
#   drive.ps1 click left|right|middle    -> click at current cursor pos
param(
    [Parameter(Mandatory = $true)][string]$Cmd,
    [string]$A = "",
    [int]$Ms = 120,
    [int]$Steps = 0
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class MesiaInput {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
    [DllImport("user32.dll")] public static extern short VkKeyScanA(char c);
    public struct POINT { public int X, Y; }
    public const uint KEYUP = 2, L_DOWN = 2, L_UP = 4, R_DOWN = 8, R_UP = 16, M_DOWN = 32, M_UP = 64;
}
"@
$proc = Get-Process mesia -ErrorAction SilentlyContinue |
        Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
function Focused {
    if (-not $proc) { Write-Error "no mesia window"; exit 1 }
    [void][MesiaInput]::ShowWindow($proc.MainWindowHandle, 9)
    [void][MesiaInput]::SetForegroundWindow($proc.MainWindowHandle)
    Start-Sleep -Milliseconds 250
}
function VkOf([string]$k) {
    $names = @{ "Esc" = 0x1B; "Enter" = 0x0D; "Back" = 0x08; "Tab" = 0x09;
                "Up" = 0x26; "Down" = 0x28; "Left" = 0x25; "Right" = 0x27;
                "Space" = 0x20; "Shift" = 0x10; "Ctrl" = 0x11 }
    if ($names.ContainsKey($k)) { return $names[$k] }
    if ($k.Length -eq 1) { return [MesiaInput]::VkKeyScanA($k.ToUpper()[0]) -band 0xFF }
    return [int]$k
}
switch ($Cmd) {
    "focus" { Focused; break }
    "key" {
        Focused
        $vk = VkOf $A
        [MesiaInput]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds $Ms
        [MesiaInput]::keybd_event([byte]$vk, 0, 2, [UIntPtr]::Zero)
        break
    }
    "keys" {
        Focused
        foreach ($k in $A.Split(',')) {
            $vk = VkOf $k.Trim()
            [MesiaInput]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 60
            [MesiaInput]::keybd_event([byte]$vk, 0, 2, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds ($Ms - 60)
        }
        break
    }
    { $_ -in "move", "drag" } {
        Focused
        $dx, $dy = $A.Split(',') | ForEach-Object { [int]$_ }
        $steps = if ($Steps -gt 0) { $Steps } else { 20 }
        $p = New-Object MesiaInput+POINT
        for ($i = 0; $i -lt $steps; $i++) {
            [void][MesiaInput]::GetCursorPos([ref]$p)
            [void][MesiaInput]::SetCursorPos($p.X + [int]($dx / $steps), $p.Y + [int]($dy / $steps))
            Start-Sleep -Milliseconds ([math]::Max(1, [int]($Ms / $steps)))
        }
        break
    }
    "click" {
        Focused
        $map = @{ "left" = @(2, 4); "right" = @(8, 16); "middle" = @(32, 64) }
        $d, $u = $map[$A]
        [MesiaInput]::mouse_event($d, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [MesiaInput]::mouse_event($u, 0, 0, 0, [UIntPtr]::Zero)
        break
    }
    default { Write-Error "unknown command $Cmd"; exit 1 }
}
Write-Output "done: $Cmd $A"
