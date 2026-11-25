# mesia launcher: runs the game for normal play (no scripted presses, no capture).
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [string]$Cache = "$env:LOCALAPPDATA\mesia\cache",
    [string]$Data = "$env:LOCALAPPDATA\mesia\data",
    [string]$Exe = "$PSScriptRoot\..\target\release\mesia.exe",
    # -Fast: fluidity over fidelity (fewer texture refreshes, plainer
    # copies, no guard holes). Textures may go stale.
    [switch]$Fast
)
foreach ($v in "MESIA_KEYS", "MESIA_CAPTURE", "MESIA_CAPTURE_FRAMES", "MESIA_DUMP_TARGETS") {
    Remove-Item "Env:$v" -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Force $Cache, $Data | Out-Null
$env:MESIA_CACHE_DIR = $Cache
$env:MESIA_DATA_DIR = $Data
# Relay disabled pending V4 crash bisect: three in-world device-removed with
# MESIA_RELAY=2 + LATE_WAIT=3; the flights machinery stays on via LATE_WAIT=1.
$env:MESIA_LATE_WAIT = "2"                # V5: flights fix test — was the fastest DEVICE_HUNG repro (~10s)
$env:MESIA_WIDE_BACK = "1"
$env:MESIA_PIPELINE_WAIT_MS = "24"
$env:MESIA_CARD_BUFFERS_MIN_MB = "1"
$env:MESIA_CARD_BUFFERS_MB = "3072"
$env:MESIA_CARD_WINDOWS_MB = "2048"
$env:MESIA_UNWATCHED_FRAMES = "1"
$env:MESIA_INPLACE = "1"                  # windows in place; stage 2 (title buffers) banned with late wait
$env:MESIA_WINDOW_KB = "1024"             # 1MiB windows for what cannot be placed (halves grid waste)
$env:MESIA_TEXTURE_NO_WAIT = "1"
$env:MESIA_JOIN_COPIES = "1"
$env:MESIA_CARD_REHASH_RUNS = "1"
$env:MESIA_WAVE_WIDE = "0"
$env:MESIA_PIPELINE_WORKERS = "12"
if ($Fast) {
    $env:MESIA_COPY_MODE = "none"         # no copy-backs at all; textures go stale
    $env:MESIA_GUARD_HOLES = "0"          # skip VirtualQuery hole scans
    $env:MESIA_UNWATCHED_FRAMES = "8"     # unwatched whole-copies every 8th frame
    # WIDE_REFRESH/WIDE_MERGE held off: two in-world device-removed with them on
    # under relay (V4); re-enable one at a time after the crash cause is known.
}
& $Exe run $Game --execute 2>&1 | Out-File "$env:TEMP\mesia.log" -Encoding utf8
Write-Host "Exited with $LASTEXITCODE; log: $env:TEMP\mesia.log"
