# mesia launcher: runs the game for normal play (no scripted presses, no capture).
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [string]$Cache = "$env:LOCALAPPDATA\mesia\cache",
    [string]$Data = "$env:LOCALAPPDATA\mesia\data",
    [string]$Exe = "$PSScriptRoot\..\target\release\mesia.exe"
)
foreach ($v in "MESIA_KEYS", "MESIA_CAPTURE", "MESIA_CAPTURE_FRAMES", "MESIA_DUMP_TARGETS") {
    Remove-Item "Env:$v" -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Force $Cache, $Data | Out-Null
$env:MESIA_CACHE_DIR = $Cache
$env:MESIA_DATA_DIR = $Data
$env:MESIA_RELAY = "2"
$env:MESIA_LATE_WAIT = "3"
$env:MESIA_WIDE_BACK = "1"
$env:MESIA_PIPELINE_WAIT_MS = "24"
$env:MESIA_CARD_BUFFERS_MIN_MB = "1"
$env:MESIA_JOIN_COPIES = "1"
$env:MESIA_CARD_REHASH_RUNS = "1"
& $Exe run $Game --execute 2>&1 | Out-File "$env:TEMP\mesia.log" -Encoding utf8
Write-Host "Exited with $LASTEXITCODE; log: $env:TEMP\mesia.log"
