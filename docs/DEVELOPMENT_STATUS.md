# DEVELOPMENT STATUS — MESIA Unified

Private-lab ledger, phases 0–6. Every result in this document was
**verified on hand-assembled synthetic fixtures — not real titles**. No real
game runs on this engine.

Lab hardware: **NVIDIA RTX 3060 12 GB · Windows 11 · 16 GB RAM**.
Current totals: **137 automated tests green, 0 warnings**.

## Per-phase ledger

| Phase | Goal | Status | Demonstrated result | Test coverage | Known limits at that stage | Last verified milestone |
|---|---|---|---|---|---|---|
| **0 — Foundation** | One workspace, independent engine trees coexisting, unified config, cross-engine PE harness | ✅ done | Both PE parsers agree on every `PeFacts` field over a 49-image corpus; config resolves per-subsystem backends | 3 `pe_compare` tests | Prepare-time facts only; no loader merge yet | `pe-compare` green on System32 + the reference engine corpora |
| **1 — Unified PE loader** | Single `PreparedImage` IR + one manual mapper + pluggable import binding | ✅ done | **Byte-parity both ways**: normalized snapshots of the same image mapped by two independent reference pipelines and the unified mapper differ only inside a computed allowed set; probes callable through both pipelines; TLS callbacks + `DllMain` run | 10/10 | No entry-point execution; resolver-dependent IAT slots documented; no GPU | `prepared_image_parity` — zero unexplained byte diffs |
| **2 — Execution + GPU device** | Run mapped images from their entry point; own a real D3D12 device; fence-arbitrated memory | ✅ done | Real `DllMain` + returning EXE entry run end-to-end; RTX 3060 device + queue + fence epochs; UPLOAD→DEFAULT→READBACK byte-exact; `FencePool` evicts only `last_use <= completed`; device-removed *detection* | 21/21 | No render path, no PSOs/descriptors; one direct queue; `FencePool` accounting-only | `gpu_device` suite 5/5 on real hardware |
| **3 — GPU resources + first render** | Real resources under budget, descriptor heaps, first pixels, persistent PSO cache | ✅ done | `GpuAllocator` with real `Evict`/`MakeResident`; heaps with null guard slot + null-fill; byte-exact triangle readback; `CachedPSO` warm create **5.2 ms vs 13.8 ms** cold (×2.65); journal → offline cache rebuild | 36/36 | Offscreen only (no swapchain); no command layer; PSO lanes designed, conservative | `gpu_render` + `gpu_alloc` + `gpu_cache` all green on RTX 3060 |
| **4 — Shader translation** | `mesia-shader`: guest GFX10 container → DXIL, bound per SM 6.6 contract, dispatched | ✅ done | Real guest DXBC containers (DXIL/XDAT/XBCH/FSSM) translated; const-store readback-exact; **wave64 OR-reduce → `0xFFFF_FFFF` on 32-lane host** after `WaveLaneCountMax` config; journal v2 replays translation offline | 51/51 | Compute only; one wave64 program; f16 rounding documented (host toward-zero vs console nearest) | `wave64_or_reduce_runs_on_the_host_gpu` — translate 833 µs |
| **5 — Translated graphics** | VS/PS through the same pipeline; multi-stage signatures; command-layer design | ✅ done | `ISG1`/`OSG1`/`PSG1` + stage-data writers; hand-assembled GFX10 VS+PS → graphics PSO → `DrawInstanced` → **2016/4096 green texels, pixel-exact**; repeat draws byte-identical, PSO built once; U-16 spec fully recovered (audit) | 64/64 | One fixture pair; no vertex fetch, varyings, VS resources, GS/HS/DS/MS, depth, MRT | `translated_triangle_renders` — byte-exact inside/outside |
| **6 — Command translation** | `mesia-command`: stream → IR → guard → execute on the shared device | ✅ done | `MESC` lab stream decoded to 12-op IR; decoded Draw → pixel-exact triangle; decoded Dispatch → translated compute; **compute-produced args → real `ExecuteIndirect`** (U-16 happy path); **Broken producer → neutralized, zero texels** (U-16 protection) | 100/100 | Lab stream format only (no console-format claim); no indexed draws/scissor op/multi-RT | `skipped_producer_neutralizes_indirect` + `compute_produces_indirect_draws_it` |
| **7 — Advanced commands + first varying** | count-buffer indirect, real CopyBuffer, interpolated attributes | ✅ done | `CopyBuffer` byte-exact incl. compute→copy→indirect chain; **count-buffer `ExecuteIndirect`**: GPU count 0/1/70000 vs host cap verified; U-16 neutralizes a tainted *count* buffer too; **first varying**: VS `exp param` + PS `v_interp` → a real interpolated gradient (and flat variant), pixel-verified | 137/137 | Indirect still single-signature-type Draw/Dispatch; interp verified on one channel pair (perspective+flat); no indexed draws/MRT/depth | `indirect_count_capped_by_max`, `translated_vinterp_gradient_renders` |

## Cumulative test growth

```text
phase 0   ▍  3      phase 3   ▍ 36
phase 1   ▍ 10      phase 4   ▍ 51      phase 7  ▍ 137  ✅ 0 warnings
phase 2   ▍ 21      phase 5   ▍ 63      phase 6  ▍ 100
```

(Milestone-5 reports previously read "64" — a reporting arithmetic slip;
the true count at both ends of phase 5 is 63. The ledger above is exact.)

## Standing constraints & documented design issues

| Item | State |
|---|---|
| License compatibility across third-party components unresolved — **source stays private; "license review in progress"** | open, blocks publication |
| No legal title content on the lab machine → no render/audio/gameplay measurement of real titles possible | open |
| 16 GB RAM (~3 GB free at rest) → PSO builds bounded to 4 threads; budgeted residency is the default posture | standing constraint |
| Optional kernel32 import interception behind a resolver policy flag | designed, not wired |
| One direct queue only; compute/copy queues and present path pending | partial |
| Device-removed detection yes, recovery no — but removal is now inducible on demand (see the pipeline-library incident below), so recovery is testable | accepted / testable |
| `ID3D12PipelineLibrary` escalates malformed blobs to adapter-wide `DEVICE_REMOVED` on this driver — mitigated by per-PSO `CachedPSO` + hash-verified envelope | mitigated by design |
| Translator `host_max_wave` is process-global — engine contract: `configure_translation(dev)` before translating; folded into cache key | mitigated by contract |
| f16 rounding: host toward-zero vs console nearest — `a console-exact f16 rounding flag` exists in the translator, not yet an engine option | documented |
| Translated graphics breadth — the honest list: no vertex fetch, VS resources, extra stages, depth/stencil, MRT; varyings now verified on one channel pair (perspective + flat) | roadmap |

## How to read the numbers

- "Verified" always means *verified by CPU readback on the real RTX 3060* —
  byte-exact or named-error, never "it didn't crash".
- Fixture shaders are tens of instructions; translation timings (~833 µs) will
  scale with real program size. The ×2.65 cache figure is on a trivial PSO;
  the mechanism — persisted blob, driver-validated — is what scales.
- Test count is the lab's gate: every phase closes only when the full
  workspace suite is green **with zero warnings**.
