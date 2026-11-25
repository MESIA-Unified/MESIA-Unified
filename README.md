# MESIA Unified

[![License: CC-BY-4.0](https://img.shields.io/badge/license-CC--BY--4.0-blue)](LICENSE)
[![Status: experimental research](https://img.shields.io/badge/status-experimental%20research-orange)](ROADMAP.md)
[![Tests: 100 automated checks (private lab)](https://img.shields.io/badge/tests-100%20automated%20checks%20%28private%20lab%29-green)](docs/DEVELOPMENT_STATUS.md)

**An experimental Xbox Series compatibility engine for Windows**, written in Rust on
top of DirectX 12, with GFX10→DXIL shader translation. MESIA Unified is a
community research project studying how Xbox Series binaries could be loaded,
their GPU command streams interpreted, and their shaders recompiled for PC
hardware — as a single, unified engine.

> **This repository ships documentation only.** The engine source is developed in
> a private lab and is not yet published — a license-compatibility review across
> its upstream components is in progress. See the [FAQ](docs/FAQ.md) and the
> [source status](#source-status) section below.

**Français :** voir [README_FR.md](README_FR.md).

---

## What this is — and what it is not

MESIA Unified is **research infrastructure**, not a product and not an emulator
you can download and play games on. Everything verified so far was verified on
**hand-assembled synthetic fixtures built inside the lab — not on real titles**.

- **Long-term ambition:** run *GTA VI* (Xbox Series build) on PC. This is a
  stated ambition only — nothing here runs, boots, or renders that title or any
  other real game.
- **Today:** a unified engine skeleton with a PE loader, a D3D12 backend with
  budgeted GPU memory, a persistent pipeline cache, a GFX10→DXIL shader
  translation service, and a guest-command decode/guard/execute layer — each
  stage individually verified on the lab's own fixtures.
- **Not:** a crack, a port, or a redistribution. There are no game files, no
  proprietary content, and no DRM circumvention anywhere in this project.
  See [Legal status & disclaimer](#legal-status--disclaimer).

## Engine at a glance

Six crates, one engine — details in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md):

| Crate | Role |
|---|---|
| `mesia-loader` | PE loading: shared `PreparedImage` IR, manual mapper (VirtualAlloc + relocs + IAT + TLS), pluggable import resolvers, `LoadedImage` lifecycle |
| `mesia-mem` | Host memory arenas, `FencePool` — budget + LRU residency arbitrated by GPU fence epochs |
| `mesia-core` | Unified configuration, cross-engine `PeFacts` PE-comparison harness |
| `mesia-gpu` | The single D3D12 backend: device/queue/fence epochs, `GpuAllocator` (real `Evict`/`MakeResident`), descriptor heaps with a null guard slot, dedup'd `PipelineLibrary` with a persistent `D3D12_CACHED_PIPELINE_STATE` cache + offline journal |
| `mesia-shader` | GFX10→DXIL translator (built on a `victory-shader` derivative), guest container parsing (DXBC/DXIL/XDAT/XBCH/FSSM + `ISG1`/`OSG1` signature parts), wave64→host-width lowering, neutral `TranslatedShader` output types |
| `mesia-command` | Guest command layer: `MESC` lab stream decoder → typed `GuestCommand` IR → `Recording` + `IndirectGuard` → executor emitting onto the shared D3D12 surfaces |

## Where it stands

Six integration phases completed in the private lab, **100 automated tests green,
0 warnings** on the lab hardware (NVIDIA RTX 3060 12 GB, Windows 11, 16 GB RAM).
Full milestone text in [ROADMAP.md](ROADMAP.md) and
[docs/DEVELOPMENT_STATUS.md](docs/DEVELOPMENT_STATUS.md). Highlights:

- **PE loader** — one unified image pipeline proven byte-parity against both
  upstream engines' loaders; TLS callbacks and `DllMain`/EXE entry points run
  end-to-end on lab fixtures.
- **D3D12 backend** — single device on an RTX 3060, fence-epoch deferred frees,
  real `Evict`/`MakeResident` residency under a hard budget, descriptor heaps
  hardened with a permanent null guard slot + null-fill.
- **Persistent PSO cache** — per-PSO `D3D12_CACHED_PIPELINE_STATE` blobs,
  hash-verified before the driver ever sees them, plus an offline journal that
  rebuilds a warm cache without rerunning the workload.
- **GFX10→DXIL translation** — compute dispatch verified by readback, including
  a wave64 cross-lane OR-reduce lowered correctly onto a 32-lane host; a
  translated VS+PS pair rendered a pixel-exact triangle (2016/4096 texels).
- **Command translation** — `MESC` lab stream → decode → IR → indirect guard →
  execute: decoded draws, dispatches, and GPU-produced indirect arguments all
  verified; the U-16 indirect neutralization rule is GPU-verified.

*All results above were verified on synthetic fixtures, not real titles.*

## How it's built: multi-agent development

Phases 5 and 6 were built by **teams of isolated sub-agents**: each agent worked
in its own git worktree against a **frozen interface contract** published before
work began, and an orchestrator integrated the results in a fixed order —
touching the shared files itself. Phase 5: four worktrees, zero file conflicts.
Phase 6: same model, GPU tests serialized through the orchestrator. The
interface contracts are treated as lab artifacts (they survive in the reports)
— a methodology worth noting for anyone coordinating parallel work on a tightly
coupled Rust engine.

## Upstream watch

MESIA Unified stands on prior public work and tracks it for fixes:

- [`xcl-dev/xcl`](https://github.com/xcl-dev/xcl)
- [`fxceless/victory-fork`](https://github.com/fxceless/victory-fork)
- [`ZigZagBoi1/ScarlettPort`](https://github.com/ZigZagBoi1/ScarlettPort)
- [`DeIete13/MESIA`](https://github.com/DeIete13/MESIA)

Engineering lessons from these codebases (deferred-free epochs, descriptor guard
slots, indirect-call tainting, PSO build budgets, wave64 lowering) are cited in
the architecture and status documents.

## Source status

The engine source remains **private** pending a license-compatibility review
across the upstream components it consumes — call it "license review in
progress"; the lab will not distribute builds or code until it is resolved. This
repository publishes the project's documentation, status reports, and roadmap
under [CC-BY-4.0](LICENSE) in the meantime.

## Join the MESIA Unified Development Team

The project is small, research-grade, and run by collaborators. We are looking
for contributors with experience — or serious interest — in:

- **Rust** systems programming (unsafe code discipline, workspace design)
- **DirectX 12** (command lists, root signatures, residency, `ExecuteIndirect`)
- **GFX10 / DXIL shader translation** (ISA decoding, wave-size lowering, SM 6.x)
- **GPU memory management** (budgets, eviction, fence-based lifetime tracking)
- **Compatibility-layer systems** (PE loading, HLE/import binding, command IRs)
- **Performance measurement** and **test engineering** on real hardware
- **Documentation** (the repo you're reading now)

To apply, read [CONTRIBUTING.md](CONTRIBUTING.md) and open an issue using the
**collaborator application** template (`.github/ISSUE_TEMPLATE/`), or start from
the [issue tracker](https://github.com/MESIA-Project/MESIA-Unified/issues).

## Documentation

- [ROADMAP.md](ROADMAP.md) — what's done, what's exploring, what's planned
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crate diagram, command & shader pipelines, safety rules
- [docs/DEVELOPMENT_STATUS.md](docs/DEVELOPMENT_STATUS.md) — per-phase ledger: goals, results, limits
- [docs/FAQ.md](docs/FAQ.md) — the honest answers first

## Legal status & disclaimer

MESIA Unified is an **unofficial community research project**. It is not
affiliated with, sponsored by, or endorsed by Rockstar Games, Take-Two
Interactive, Microsoft, or any other company. "Xbox", "Xbox Series", "GTA", and
"GTA VI" are trademarks of their respective owners; references are for
identification of research goals only.

The project is **not a crack, not a port, and not a bypass**: it contains no
game code, no game assets, no console firmware or SDK material, and implements
no DRM circumvention. All development and verification uses clean-room,
hand-assembled synthetic fixtures. Running a real title would require the
user's own legally obtained files — and no real title is currently supported.

Documentation in this repository is licensed under
[Creative Commons Attribution 4.0 International (CC-BY-4.0)](LICENSE).
