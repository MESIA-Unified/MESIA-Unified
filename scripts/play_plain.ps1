# V5.6 A/B run B: MESIA_COPY_PLAIN=1 — same launcher as play.ps1 with the
# plain copy enabled. Remove after the campaign decides.
$env:MESIA_COPY_PLAIN = "1"
& "$PSScriptRoot\play.ps1" @args
