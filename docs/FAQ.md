# FAQ — MESIA Unified

The honest answers first.

## Can it run GTA VI?

**No.** Running *GTA VI* (Xbox Series build) on PC is the project's stated
**long-term ambition** — a direction, not a capability. Nothing in the engine
runs, boots, or renders that title, and no claim to the contrary should be
believed. Every verified result so far was produced by hand-assembled
synthetic fixtures inside the private lab.

## Can it run any game at all?

**No.** The engine is verified exclusively on **synthetic fixtures** — lab-built
PE binaries, hand-assembled GFX10 shader programs, and lab-format command
streams. There is no legal title content on the development machine, no game
has ever been loaded by this engine, and no game compatibility exists to
report.

## Where's the code?

**Private, pending license review.** The engine source lives in a closed lab
repository. Publication is blocked by an unresolved license-compatibility
question across the upstream components the engine consumes — call it
**"license review in progress"**; we won't distribute code or builds until it
is resolved. This public repository ships **documentation only** (status,
architecture, roadmap, reports) under CC-BY-4.0.

## Is it affiliated with Rockstar, Take-Two, or Microsoft?

**No.** MESIA Unified is an unofficial community research project. It has no
affiliation with, sponsorship from, or endorsement by Rockstar Games, Take-Two
Interactive, Microsoft, or anyone else. Trademarks are referenced only to
identify research targets.

## Is this a crack / does it bypass DRM / does it include game files?

**No, on all three.** The project contains no game code, no game assets, no
console firmware or SDK material, and implements no DRM circumvention.
Development and verification are clean-room: fixtures are assembled by hand
inside the lab. If real-title work ever becomes possible, it would require the
user's own legally obtained files.

## What is `MESC`?

The lab's own **guest command stream record format** — a documented,
versioned byte format used to exercise the command pipeline (decode → IR →
guard → execute) end-to-end. It is **not** console command-format
compatibility and makes no provenance claim; the decoder tests state this in
their header. Supporting real guest record formats is roadmap work.

## What actually works today?

Six integration phases are complete: a unified PE loader (byte-parity-proven),
a single-device D3D12 backend with fence-arbitrated budgeted residency, a
persistent PSO cache + offline journal, GFX10→DXIL shader translation
(compute dispatch and a translated VS+PS draw, both readback-verified), and a
command layer with a GPU-verified indirect-call guard. **100 automated tests,
0 warnings** — all on synthetic fixtures, all in the private lab.

## On what hardware was it verified?

A single lab machine: **NVIDIA RTX 3060 12 GB, Windows 11, 16 GB RAM**. The
tight memory budget is a deliberate design pressure — PSO builds are bounded
(driver compiles cost ~1 GB each) and GPU residency is budget-enforced rather
than assumed.

## How can I follow progress?

Watch this repository — dev notes, status documents, and the roadmap are
published here as milestones land. Releases (when they exist) will announce
through the repo's release feed. There is no build to download yet; see
"Where's the code?" above.

## How can I contribute?

See the *Join the MESIA Unified Development Team* section of the
[README](../README.md), read [CONTRIBUTING.md](../CONTRIBUTING.md), and open
an issue with the **collaborator application** template. Active needs: Rust,
DirectX 12, GFX10/DXIL shader translation, GPU memory management,
compatibility systems, performance measurement, testing, and documentation.

## What's the legal status of the research itself?

The published material is **documentation under CC-BY-4.0**, describing
clean-room engineering on synthetic fixtures — no proprietary content is
reproduced. The engine consumes upstream open-source components; the
publication of its source awaits the license-compatibility resolution noted
above. Nothing here is a tool for infringing anyone's rights: no game files,
no circumvention, no redistribution.

## Why is it called "Unified"?

Because it merges what three separate upstream codebases do — PE loading and
HLE binding, GPU command submission, and GFX10 shader translation — into one
engine with one device, one memory policy, one translation path, and one test
gate. The "unified" refers to the architecture, not to any product claim.
