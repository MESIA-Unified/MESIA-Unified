# ARCHITECTURE — MESIA Unified

One engine, six crates, one D3D12 device. This document describes the
private-lab codebase as of the phase-6 milestone (100 automated tests green on
an RTX 3060 / Windows 11). All verified behavior is exercised through
**synthetic lab fixtures — not real titles**.

## Crate map

```text
            guest PE image        guest shader          guest command
            (synthetic .exe/.dll) container (GFX10)     stream (MESC, lab)
                  │                     │                     │
                  ▼                     ▼                     ▼
        ┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐
        │   mesia-loader    │ │   mesia-shader    │ │  mesia-command    │
        │ PreparedImage IR  │ │ translate() →     │ │ decode → IR →     │
        │ + manual mapper   │ │ TranslatedShader  │ │ Recording → guard │
        │ + ImportResolver  │ │ (translator core  │ │ → execute_into()  │
        │ + LoadedImage     │ │  derivative;      │ │ (IndirectGuard =  │
        │   lifecycle       │ │  neutral types)   │ │  U-16 rule)       │
        └─────────┬─────────┘ └─────────┬─────────┘ └─────────┬─────────┘
                  │                     │                     │
                  │             TranslatedShader              │ emits onto
                  │             {dxil, user_data,             │ caller's
                  │              views, samplers,             │ command list
                  │              key}                         ▼
                  │                     │           ┌───────────────────────┐
                  │                     └──────────►│      mesia-gpu        │
                  │                                 │  THE ID3D12Device —   │
                  │      ┌──────────────┐           │  single owner         │
                  │      │              │           │                       │
                  └─────►│  mesia-mem   │◄───────── │ GpuDevice · queue +   │
                         │ HostMemory   │  GpuClock │ fence epochs          │
                         │ Arena trait  │  seam     │ GpuAllocator (Evict/  │
                         │ LazyCommit   │  (only    │   MakeResident)       │
                         │ Arena        │  GPU fact │ DescHeap (guard slot  │
                         │ FencePool —  │  memory   │   + null-fill)        │
                         │ budget+LRU   │  policy   │ PipelineLibrary       │
                         │ arbitrated   │  consults)│  (dedup · CachedPSO   │
                         │ by fence     │           │   · MESIAJNL journal) │
                         │ epochs       │           │ OffscreenTarget       │
                         └──────────────┘           └───────────────────────┘

     ┌───────────────────────── shared foundation ─────────────────────────┐
     │                            mesia-core                                │
     │  unified config (paths, per-subsystem backends) + PeFacts            │
     │  cross-engine PE-comparison harness — used by every crate above      │
     └──────────────────────────────────────────────────────────────────────┘
```

### Crate roles

| Crate | Owns | Explicitly does not own |
|---|---|---|
| `mesia-loader` | `PreparedImage` IR, manual mapping (VirtualAlloc + sections + relocs + IAT + TLS + VirtualProtect), pluggable `ImportResolver`/`MissingImportPolicy`, `LoadedImage` Cold→Attached→Detached lifecycle | GPU, shaders, any console-format claim |
| `mesia-mem` | `HostMemoryArena` trait, `LazyCommitArena`, `FencePool` residency policy, the `GpuClock`/`QueueClock` seam | Resource objects — it never creates `ID3D12Resource` |
| `mesia-core` | Config resolution, `PeFacts` harness | Runtime behavior |
| `mesia-gpu` | **The single `ID3D12Device`**, one direct queue, fence epochs, `GpuAllocator`, `DescHeap`, `PipelineLibrary` + persistent cache + journal, `OffscreenTarget`, the SM 6.6 translated-binding contract, device-removed detection | Command semantics (that's `mesia-command`), translation internals |
| `mesia-shader` | `translate()` + `TranslatedShader`, guest DXBC container read/write, wave64→host lowering config, stable cache keys | GPU objects — output is neutral owned types |
| `mesia-command` | `MESC` decoder, `GuestCommand` IR (12 ops), `Recording` + `validate()`, `IndirectGuard`, `execute_into` | The D3D12 device — it borrows `mesia-gpu` surfaces |

## The single-D3D12-device rule

**`mesia-gpu` owns THE `ID3D12Device`; nothing else creates one.** Phase 6
explicitly did *not* build a second D3D12 engine for command execution — the
executor emits onto caller-provided command lists through the shared device,
`PipelineLibrary`, `DescHeap`, and `OffscreenTarget` surfaces. Likewise, the
only GPU fact memory policy may consult is the `GpuClock`/`QueueClock` seam
(fence completed-value); `FencePool` never touches the device, and the wave
limit the translator lowers to comes from `WaveLaneCountMax` read once at
engine init. One device, one queue, one clock — every subsystem borrows them.

## The command path (phase 6)

```text
MESC bytes ──► decode() ──► Vec<GuestCommand>           12 ops implemented;
                                │                       anything else =
                                ▼                       named Unsupported
                        Recording::new()
                        (pipeline / RT / heap tables
                         + validate())
                                │
                                ▼
                        IndirectGuard  ── U-16: skipped producer ⇒
                        taint_any / lost_writes / written   tainted recording
                                │
                                ▼
                        execute_into(cmd_list)
                        ── mesia-gpu emission conventions:
                        sig+PSO emitted indivisibly at first
                        pipeline-dependent command · RTV bound on
                        change · heaps re-bound defensively ·
                        scissor := viewport
                                │
                                ▼
                ID3D12GraphicsCommandList → queue → fence → readback
```

`GuestCommand` IR op subset: `SetPipeline` · `SetRenderTarget` ·
`ClearRenderTarget` · `SetViewport` · `SetTopology` · `SetRootConstants` ·
`SetDescriptorHeap` · `Transition` · `Draw` · `Dispatch` · `CopyBuffer` ·
`ExecuteIndirect`. `PipelineDef` carries `TranslatedGraphics` /
`TranslatedCompute` / `Broken` — `Broken` models a producer the host could not
translate; dispatching on it skips the work *and taints the recording*.

> **`MESC` is the lab's own record format.** It documents the implemented op
> subset and makes **no claim of console record-format compatibility** — the
> decoder tests say so in their header. Real guest formats are roadmap work.

## The indirect guard (U-16), in one line

**If any producer dispatch in a recording was skipped, every `ExecuteIndirect`
in that recording is rewritten to `MaxCommandCount = 0` — a driver-safe no-op —
because its argument/count buffers may hold garbage that would TDR the GPU.**

Semantics, strictly:

- Taint is per-recording (`taint_any`), cleared only by `reset()`; a
  mid-recording split inherits it. One skip poisons the whole recording — the
  strictest audited semantics — because a rewrite cannot prove it
  repaired every bound output.
- `lost_writes` tracks precisely which buffers a skipped producer was bound to
  write (a future per-buffer mode exists on paper); `written` is bookkeeping
  for conservative overwrite detection.
- Neutralization emits `ExecuteIndirect(sig, 0, args, 0, NULL, 0)` — the driver
  sees a no-op, never an infinite wait.
- Untainted calls are capped at `MESIA_INDIRECT_MAX = 65536` commands.
- GPU-verified both ways: a compute-produced `(3,1,0,0)` args buffer drives a
  real `ExecuteIndirect` draw; a `Broken` producer produces zero written
  texels.

## The shader translate path (phases 4–5)

```text
guest DXBC container (console-precompiled GFX10 machine code)
   DXBC hdr → part DXIL → XDAT (kind 0x0d native) → XBCH v3 zlib
   → FSSM v0x101 (8 stage slots: code ptr/len, wave32 flag, layout
     + used tables, regs; then stage_data, 20-byte entries, code)
   graphics adds: ISG1 / OSG1 / PSG1 signature parts + per-stage
   stage_data (semantics tables, SPI_PS_INPUT_ADDR, DB_SHADER_CONTROL,
   8-MRT export-patch table)
        │  mesia-shader::translate          ← the lab's shader-translation codepath
        ▼
TranslatedShader { stage, dxil, user_data[], user_data_dwords,
                   samplers, static_samplers, root_cbvs/srvs/uavs,
                   root_indexed_views, key, guest }
        │  mesia-gpu::translated            ← SM 6.6 binding contract
        ▼
ID3D12RootSignature
   param k = stage k's user-data constants (b0, space 0, per-stage
   visibility) · root CBV/SRV/UAV descriptors sorted by (slot, kind)
   at space 1 (visibility ALL) · immediate samplers as static samplers
   · flags: ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT |
     CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED | SAMPLER_HEAP_DIRECTLY_INDEXED
        │  PipelineLibrary::{compute,graphics}_translated
        ▼
ID3D12PipelineState
   dedup → CachedPSO blob (hash-verified, driver-validated) → compile;
   MESIAPSO file + MESIAJNL journal (persists the *guest container* —
   offline replay re-runs the identical translate → sig → compile)
        │
        ▼
DrawInstanced / Dispatch → fence → readback → verified bytes
```

Two design rules worth stating plainly:

1. **Neutral types across the seam.** `mesia-shader` emits owned data only —
   nothing from the translator's internals reaches `mesia-gpu`, so an
   alternative translator could implement the same `TranslatedShader`
   contract.
2. **The wave limit is host config, not guest data.** The translator's
   `host_max_wave` is process-global; the engine sets it once at init from
   `WaveLaneCountMax` and folds it into `cache_key` — the same guest shader is
   a different pipeline under a different host wave limit. A wave64 guest
   translated with the limit unset emits `[WaveSize(64)]` DXIL the driver
   rejects; with it set, the wave64→wave32 lowering produces correct results
   on a 32-lane host (verified: 64-lane OR-reduce → `0xFFFF_FFFF`).

## Safety rules baked into the design

| Rule | Origin | Enforcement |
|---|---|---|
| Never free while the GPU may still use it | both reference engines | Fence-epoch deferred retire; `last_use <= completed` required to free or evict |
| An unwritten descriptor must never fault the GPU | technical audit of published work — out-of-range descriptor reads hang | Permanent null guard slot + null-fill at heap creation |
| A skipped producer's indirect args are poison | documented crash 0x887a0006 (the U-16 rule) | `IndirectGuard` neutralization, GPU-verified |
| Root signature and PSO bind as one indivisible pair | structural TDR vector | Emitted lazily together at first pipeline-dependent command |
| Driver sees only bytes we serialized | `ID3D12PipelineLibrary` `DEVICE_REMOVED` incident | Hash-verified `MESIAPSO` envelope; per-PSO `CachedPSO` instead of a library object |
| PSO builds are bounded | driver compile ≈ 1 GB each; 16 GB machine | ≤4 build threads, urgent/gfx/compute lanes, bounded wait + stall counter |
| Unparsable/untranslatable input fails *named* | correctness rule | `NotDxbc`/`Malformed`/`Isa`/`Unsupported` errors — never a wrong shader silently emitted |
