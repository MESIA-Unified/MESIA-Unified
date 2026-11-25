# MESIA — Status & Roadmap

**English** | [Français](STATUS_FR.md)

_Last updated: 2026-11-25. Test machine: Windows 11, NVIDIA GeForce RTX 3060 12 GB (driver current), 16 GB RAM._

MESIA runs the Xbox Series build of the game natively on Windows: the
title's own code executes unmodified inside a re-hosted Win32 process,
with its kernel, D3D12X (Xbox variant), DirectStorage, XAudio/Opus and
GameInput calls translated to their PC equivalents. Shaders are
translated to host DXIL and cached on disk between runs.

## What works today

| Area | State |
|---|---|
| Boot & first-boot screens | Working — reaches the title's own flow without patches |
| Intro sequence / opening credits | Working, presented at ~30 fps |
| Prologue | Reaches playable sections (cell → cafeteria → robbery → police chase → boat) |
| Input | GameInput → host GameInput; gamepad + keyboard enumerated |
| Audio | Opus decode path verified stable over multi-minute runs (no underruns/pops observed at 48 kHz stereo) |
| Saves / virtual drives | `G:` (game dir) and `T:` (title temp) mapped; per-title storage at `<data>/<title id>/` |
| Shader pipeline cache | Persistent library (`cache/pipelines/host-pipelines.bin`, auto-saved every 20 s) — currently ~92 MB after a few runs; each run compiles fewer shaders on first sight |
| Prepared images | Title binaries re-hosted once into `cache/images/mesia-image-*.dll` (imports bound, TLS redirected) |

## Measured performance

| Phase | Frame rate | Notes |
|---|---|---|
| Loading / 2D screens | ~30 fps | Light scenes; presents are real frames, not a splash |
| World (early, cold pipelines) | 0.2 – 1.7 fps | Dominated by first-encounter pipeline compiles and texture uploads |
| World (warmer cache) | 1 – 3 fps | Improves run-over-run as `host-pipelines.bin` fills |

Where a heavy frame (~1–5 s) goes, per the engine's own counters:

- **Texture refreshes: up to ~3–4 s/frame** — reading, hashing, de-tiling and
  uploading title texture memory (thousands of views picked by index each
  frame during world streaming; >20,000 queued uploads observed).
- **Buffer copies: ~100–500 ms/frame** — title memory → host copies.
- **Copy-back (relay): ~400–900 ms/frame** — returning results into title
  memory from a side thread (`MESIA_RELAY`).
- **Heap-buffer passes: ~300–1,300 ms/frame** — card-side compare passes.

The submitting thread itself waits on the card for only tens of ms —
the bottleneck is host-side CPU work (refresh/copy/hash), not raw GPU.

## Current blockers

1. **NVIDIA descriptor-heap cap (1,000,000 views).**
   The title builds a view heap of ~1.24 M slots. D3D12 on NVIDIA refuses
   heaps past 1 M, so MESIA caps the host heap and translates every shader
   with the clamp (`set_view_heap_limit`): reads past the cap return a
   null slot instead of hanging the GPU. Consequence: some textures bound
   through high indices show as missing/null. A proper fix needs a slot
   remapping layer, not a bigger heap.

2. **First-encounter pipeline storm.**
   The world binds >1,100 pipelines that don't exist yet. Draws whose
   pipeline is still compiling are dropped for that frame
   (`MESIA_PIPELINE_WAIT_MS` bounds the wait), so first-run scenes show
   missing/flickering elements — the buggy-looking loading screens.
   This disappears as the on-disk library fills across runs.

3. **Refused compute pipelines (`0x80070057`, E_INVALIDARG).**
   A handful of translated compute DXIL blobs are rejected by the driver's
   `CreateComputePipelineState`. Rare (2–3 per session) but each refusal
   drops its draws permanently for the run. Under investigation.

4. **Ray-traced lighting not running.**
   Shadows are fully black and bright scenes overexpose: the game's
   ray-traced GI/lighting path isn't translated yet. Blocky artifacts in
   places; boats sit low in the water (a physics/sim issue, not graphics).

5. **Throughput ceiling.**
   Even with all perf switches on, world frames are bound by CPU-side
   texture/buffer refresh + copy-back. The two biggest remaining levers
   are batching texture uploads further and widening the card-side
   buffers window (`MESIA_CARD_BUFFERS_MB`, `MESIA_CARD_WINDOWS_MB`).

## Perf switches used by `scripts/play.ps1`

```
MESIA_RELAY=2                # copy-back runs beside the submitting thread
MESIA_LATE_WAIT=3            # latest-possible settle point
MESIA_WIDE_BACK=1            # widened copy-back batches
MESIA_PIPELINE_WAIT_MS=24    # cap per-frame pipeline wait (default 500)
MESIA_CARD_BUFFERS_MIN_MB=1  # card-side compares for buffers >=1 MiB
MESIA_JOIN_COPIES=1          # merge consecutive buffer copies
MESIA_CARD_REHASH_RUNS=1     # single hash pass over card copies
```

## Next milestones

- Slot remapping for view heaps past 1 M (fixes null-slot textures).
- Fix the `0x80070057` compute pipeline refusals.
- Offline/early pipeline prewarm from `early-pipelines.txt`.
- Batched texture-upload path (cut the 3–4 s refresh spikes).
- Ray-traced lighting path (unblock shadows/exposure).

_Not affiliated with or endorsed by the game's developer or publisher.
This repository contains no game files or assets._
