# ROADMAP — MESIA Unified

Scope honesty: every "done" line below was **verified on hand-assembled
synthetic fixtures in the private lab — not on real titles**. Long-term
ambition: *GTA VI* (Xbox Series build) on PC — an ambition, not a capability.
No real title runs on this engine today.

Current state: **6 integration phases complete · 100 automated tests green,
0 warnings** (NVIDIA RTX 3060 12 GB, Windows 11, 16 GB RAM).

---

## DONE — integration phases

### Phase 0 — Workspace foundation ✅

- One Cargo workspace where both upstream engine trees coexist as path
  dependencies, a unified config layer (`mesia-core`), and a cross-engine PE
  harness (`PeFacts` contract, per-field diff report).
- Both PE parsers proven to agree on every fact over a 49-image corpus.

### Phase 1 — Unified PE loader (`mesia-loader`) ✅

- Single `PreparedImage` IR produced by victory's `prepare()` pipeline
  (loadconfig/CFG/CHPE patching), mapped/bound by one lab-written manual mapper
  (xcl-style: VirtualAlloc + section copy + relocs + IAT + TLS +
  VirtualProtect).
- Pluggable `ImportResolver` / `MissingImportPolicy` traits so HLE tables from
  either engine can drive binding.
- **Byte-parity proven both ways**: normalized snapshots of the same title
  mapped by victory, xcl, and the unified mapper differ only inside a computed
  "allowed set" (loadconfig patch fields, ImageBase, security cookie, TLS index,
  missing-import stubs). Every exported probe callable through both pipelines;
  TLS callbacks ran.
- 10/10 tests green.

### Phase 2 — Entry-point execution + single GPU backend ✅

- `LoadedImage` lifecycle: Cold→Attached→Detached state machine in Windows
  order (TLS before `DllMain` attach, `DllMain` before TLS detach),
  exactly-once guards, rollback on refusal, Drop detaches. A real `DllMain`
  and a returning EXE entry point ran end-to-end.
- `mesia-gpu`: THE `ID3D12Device` (adapter preference → RTX 3060), one direct
  queue, fence epochs, `submit`/`wait`, registry ledger — and device-removed
  *detection* (`u64::MAX` → `GetDeviceRemovedReason`).
- `mesia-mem::gpupool::FencePool`: budget + LRU eviction arbitrated by the
  fence (`last_use <= completed` required to free anything), pinned exclusion,
  deferred-free bytes committed until the epoch passes.
- Hardware-verified: UPLOAD→DEFAULT→READBACK copy byte-exact; deferred retire
  follows the fence on the live queue. 21/21 tests green.

### Phase 3 — Real GPU resources + first render ✅

- `GpuAllocator`: `AllocId`↔`ID3D12Resource`, budgeted alloc with
  suspend→evict→deny escalation, real `ID3D12Device::Evict`/`MakeResident`.
- `DescHeap`: shader-visible heaps carry a **permanent null guard slot** +
  null-fill at creation — an unwritten view can never fault the GPU.
- `PipelineLibrary`: dedup by canonical desc key; shared empty root signature;
  PSO build lanes (urgent/gfx/compute, ≤4 threads — driver compiles cost ~1 GB
  each on this 16 GB machine).
- **First render verified byte-exact** by CPU readback (clear + `SV_VertexID`
  triangle on a 64×64 R8G8B8A8 target).
- **Persistent PSO cache**: per-PSO `D3D12_CACHED_PIPELINE_STATE` blobs,
  hash-verified before the driver sees them (×2.65 warm-create speedup on a
  trivial PSO); `MESIAJNL` journal → `replay_journal()` rebuilds a warm cache
  offline.
- Design reversal documented: `ID3D12PipelineLibrary` rejected after a malformed
  blob escalated to adapter-wide `DEVICE_REMOVED` on this driver.
- 36/36 tests green.

### Phase 4 — Shader translation service (`mesia-shader`) ✅

- One translation path: `translate(guest_container_bytes) -> TranslatedShader`
  over a `victory-shader` derivative; **neutral owned types only** — `mesia-gpu`
  binds the result without seeing translator internals.
- `guest_container` writes *real* guest DXBC containers (DXIL/XDAT/XBCH-zlib/
  FSSM) so fixtures take the same code path a title's bytes would.
- Hand-assembled GFX10 machine-code fixtures (const-store, `v_add_f16`, the
  console compiler's own wave64 `WaveActiveBitOr` expansion).
- SM 6.6 host binding contract: user-data constants at `b0` space 0, root
  descriptors space 1, heap-directly-indexed flags.
- **wave64→wave32 lowering verified on hardware**: `configure_translation`
  reads `WaveLaneCountMax` at engine init; 64-lane OR-reduce produced
  `0xFFFF_FFFF` on the 32-lane RTX 3060. Wave limit folded into the cache key.
- Translated-compute dispatch verified by readback; journal v2 persists the
  guest container for offline re-translation. 51/51 tests green.

### Phase 5 — Translated graphics render ✅ *(multi-agent: 4 worktrees, frozen interface contract)*

- Signature parts `ISG1`/`OSG1`/`PSG1` + per-stage stage-data (VS in/out
  semantics, PS inputs, `SPI_PS_INPUT_ADDR`, `DB_SHADER_CONTROL`, MRT
  export-patch table) so graphics containers are real, not synthetic-lite.
- Hand-assembled `vs_triangle` (VertexId→position via `v_and`/`v_lshl`/`v_cvt`)
  and `ps_solid` (`v_mov_b32` literals + `exp mrt0`) fixtures — every word
  verified against the translator's own GFX10 decode tables.
- `root_signature_for` generalized to multi-stage; `graphics_translated` runs
  the same dedup → `CachedPSO` → compile layering; journal kind-2 persists both
  guest containers.
- **Measured**: translated VS+PS → `DrawInstanced(3,1,0,0)` → readback
  **2016/4096 green texels**, byte-exact inside/outside colors; repeat draws
  byte-identical with the PSO built once.
- Command-layer design audit delivered: `GuestCommand` IR + deferred-compile
  contract, U-16 indirect-taint spec fully recovered from upstream, 12-entry
  hang/TDR risk register.
- 64/64 tests green.

### Phase 6 — Command translation engine (`mesia-command`) ✅ *(multi-agent)*

- `MESC` **lab** stream format → `decode` → typed `GuestCommand` IR (12 ops)
  → `Recording` (tables + `validate()`) → `IndirectGuard` (U-16) →
  `execute_into` → `mesia-gpu`. **No second D3D12 engine was created** — the
  executor emits onto caller lists through the shared device/PSO surfaces.
- Verified end-to-end on the RTX 3060: decoded `Draw` → pixel-exact triangle;
  decoded `Dispatch` → translated compute, readback-exact; compute-produced
  `(3,1,0,0)` args → `ExecuteIndirect` draws it (U-16 happy path); a `Broken`
  producer → same call neutralized, zero texels written (U-16 protection).
- Guard semantics: strictest upstream parity — one skipped producer poisons the
  recording until `reset()`; neutralization emits
  `ExecuteIndirect(sig, 0, args, 0, NULL, 0)`; cap `MESIA_INDIRECT_MAX=65536`.
- **100/100 tests green, 0 warnings.**

---

## EXPLORING — active design work, not committed code

- **Real guest command formats.** `MESC` is the lab's own record format — it
  documents the implemented op subset and makes *no* provenance claim. Mapping
  real console record formats (the `console_list` model) onto the same
  `GuestCommand` IR is the design question on the table.
- **Device-removed recovery.** Detection is implemented; recovery/recreate is
  not. Since a malformed `ID3D12PipelineLibrary` blob reliably induces
  `DEVICE_REMOVED` on this driver, a recovery path is now testable on demand —
  an unusual and useful test asset.
- **Per-buffer indirect taint.** The guard currently poisons the whole
  recording on any skip (strictest semantics). The `lost_writes` set already
  tracks which buffers a skipped producer would have written — a precision mode
  that neutralizes only truly-affected indirect calls is designed but not
  implemented.

## PLANNED — ordered by dependency, no dates promised

| Work item | Why it matters | Blocks on |
|---|---|---|
| **Graphics breadth** — vertex fetch (`tbuffer` attribute loads / fetch shaders), interpolated varyings (`v_interp` + `PS_INPUT_ADDR` routing), VS resource reads, depth/stencil, MRT, GS/HS/DS/MS stages | One VS+PS fixture pair is proven; real shaders need all of the above | phase 5 base |
| **Wave64 breadth** | One compute program is verified on GPU; divergent-ballot and lane-mask patterns untested through our harness | phase 4 base |
| **Command coverage** — indexed draws, scissor op in the IR, count-buffer `ExecuteIndirect` on GPU (parsed + emitted, untested), multi-RT, wider state objects | `MESC` exercises today's IR only | phase 6 base |
| **Real guest command formats** | Lab stream → real record formats; requires a legal-content strategy first | exploring item above + the standing constraint that no legal title content exists on the lab machine |
| **Texture/resource streaming** | Titles stream constantly; write-tracking costs are a known upstream trap (~600 ms/frame before caching) | resource model maturity |
| **Swapchain / present path** | Everything renders offscreen today, verified by readback | render path stability |
| **Device-removed recovery** | Detection-only today; now testable on demand | exploring item above |
| **HLE import policy** — optional xcl-style kernel32 interception behind a policy flag | Designed since phase 1 (`ImportResolver` seam), not wired | loader consumers |
| **License strategy for source publication** | The lab combines GPL-2.0-only and GPL-3.0-or-later components; publication is blocked until the license review resolves a compliant path | legal, not technical |
| **Engine options surface** — `VICTORY_F16_NEAREST` (console-exact f16 rounding), PSO thread budgets, guard modes as real config | Currently env/once flags | config plumbing |

## Explicitly out of scope

- Shipping builds, installers, or any form of game-run capability.
- DRM circumvention of any kind, console firmware/SDK material, game assets.
- Claiming real-title compatibility — verification language here stays
  synthetic until a real, legal content path exists.
