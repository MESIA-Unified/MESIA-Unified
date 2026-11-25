# Milestones

MESIA Unified is an experimental Xbox Series compatibility engine for
Windows, developed in a private lab and documented publicly here. This page
is the detailed record of what each research phase set out to do, what
shipped, and how it was verified.

**Read this first.** Every result below was produced against **synthetic
fixtures**: hand-built PE images, hand-assembled guest shader containers,
and a lab-defined guest command-stream format. No retail game, game file,
dump, or key has ever been used — there is no legal title content on the
test machine, and the project will not accept any. Retail-title
compatibility (the project names GTA VI-class titles as a long-term
ambition) is an aspiration, not a claim; no real title has been run.

Hardware-verified results were produced on an NVIDIA RTX 3060. Test counts
are cumulative workspace totals for the whole private lab at that phase.

---

## Milestone 1 — Unified loader and the memory contract

**Goal.** Merge the two reference engines' loading paths into a single
intermediate representation and prove — at the byte level — that the merged
loader produces images equivalent to both reference pipelines.

**What shipped.**

- A shared `PreparedImage` IR: one prepare pipeline — the reference engine's
  `prepare()`, whose strength is loadconfig/CFG/CHPE patching — feeds one
  lab-written manual mapper in a manual-mapper style (allocation, section copy,
  relocations, import table, TLS, memory protections).
- Pluggable `ImportResolver` / `MissingImportPolicy` traits, so either
  reference engine's HLE tables can drive import binding as a policy
  choice.
- A cross-engine PE harness: both parsers report a common `PeFacts`
  contract, diffed field-by-field over a 49-image synthetic corpus.
- `ImageSnapshot::normalize()` — byte-parity comparison across ASLR:
  captured images are un-rebased so two mappings of the same file can be
  compared, with an allowed-difference set computed from the file itself.
- `mesia-mem`: a `HostMemoryArena` contract plus a lazy-commit
  implementation on real reserved/committed pages — a hard budget
  enforced by LRU page eviction with transparent re-commit on re-touch.
- Unified configuration: paths plus per-subsystem backend selection.

**How it was verified (10/10 tests, 0 warnings).**

- `pe_compare` (3 tests): both reference parsers agree on every fact across
  the 49-image corpus.
- Two reference-loader parity tests (`*_and_unified_*`): the unified
  mapper's output compared byte-for-byte against independent reference
  mappings of the same image — normalized bytes identical outside the
  documented allowed set, exported probe functions returning identical
  values through both pipelines, TLS callbacks and `DllMain` attach/detach
  running correctly.
- `unified_maps_a_plain_host_dll`: sanity — the manual mapper loads a real
  system DLL with correct protections.
- `mesia-mem` (3 tests): reservation commits nothing, the budget holds via
  eviction, and re-touch transparently re-commits.

The harness already paid for itself: it caught a normalization bug in our
own test code (bound import slots must be skipped when un-relocating) and
isolated a real behavioral difference — one reference engine binds several kernel32 imports
to its own instrumentation shims rather than raw host pointers — which is
now modeled as a policy flag rather than a bug.

**Honest limits.** No GPU device code, no shader path, no audio/video, no
entry-point execution (probe calls were direct host calls into mapped
code).

**What's next.** Run a mapped image's entry point under a controlled
thread, and stand up a single-owner GPU device with the memory
substrate's adapter to a deferred-free timeline.

---

## Milestone 2 — Entry-point execution and a single GPU backend

**Goal.** Execute mapped images for real, and give the engine exactly one
D3D12 device owner with a fence-guarded memory model.

**What shipped.**

- `LoadedImage` lifecycle: a Cold → Attached → Detached state machine
  honoring Windows ordering (TLS callbacks before `DllMain` attach;
  `DllMain` before TLS detach), exactly-once guards, rollback on refusal,
  detach-on-drop.
- Entry-point execution on a controlled thread. The executable fixture
  reports results back through imports bound to test stubs — proof that
  the HLE policy layer is live mid-execution.
- `mesia-gpu`: the single `ID3D12Device` owner (high-performance adapter
  selection with fallback), one direct queue, fence epochs, submit/wait.
- A deferred-retire lane: objects are retired against explicit epochs and
  only freed after the fence passes — the "never free while the GPU may
  use it" rule.
- Device-removed detection: a `u64::MAX` fence value is mapped to a
  `GetDeviceRemovedReason`-backed error, and waiters re-sample after
  waking.
- `FencePool`: the memory-pool adapter where budget + LRU eviction is
  epoch-guarded — a page is only evictable if its last submitted use has
  completed. Pinned allocations are excluded; denial is a clean
  `BudgetExhausted`.

**How it was verified (21/21 tests, 0 warnings).**

- `entry_point` (3 tests): DLL attach/detach exactly once, EXE end-to-end
  on a spawned thread (entry → exit 0 → result through bound imports),
  bind census.
- `gpupool` (3 tests, deterministic mock clock): deferred frees wait for
  the fence; in-flight allocations are never evicted; pinned exclusion.
- `gpu_device` (5 tests, on hardware): single device/queue, submit + fence
  wait, an **UPLOAD→DEFAULT→READBACK copy verified byte-exact**, registry
  tracking, deferred retire following the fence.

**Honest limits.** No render pipeline (no PSOs, descriptors, swapchain, or
draws); one queue only; device-removed is detected but not recovered; the
fence pool was still an accounting model — not yet wired to real
resources.

**What's next.** Real `ID3D12Resource`s behind the pool, descriptor heaps,
pipeline building, and a first readback-verified render.

---

## Milestone 3 — Real GPU resources and the first verified render

**Goal.** Turn the accounting model into real resource management and draw
verified pixels.

**What shipped.**

- `GpuAllocator`: allocation IDs bound to real `ID3D12Resource`s, a
  budgeted allocation path (suspend → evict → deny), real
  `Evict`/`MakeResident` residency management, and **explicit reclaim** —
  allocations are never silently evicted; the caller selects victims.
- Descriptor heaps implementing an audited safety contract: a permanent
  null guard slot and null-fill of every slot at creation, so a view that
  was never written can never fault the GPU. The practical per-heap
  descriptor cap is measured and documented.
- `PipelineLibrary`: deduplication by canonical pipeline description, a
  shared root signature, offscreen render targets, and a CPU readback
  path.
- A persistent PSO cache: per-pipeline `D3D12_CACHED_PIPELINE_STATE` blobs
  harvested after each build, **hash-verified before they reach the
  driver**, and supplied on the next run; a `MESIAJNL` journal of every
  requested pipeline description can be replayed offline to pre-warm the
  cache.
- A documented design reversal: `ID3D12PipelineLibrary` was tried first
  and rejected after a malformed blob escalated to process-wide device
  removal. The per-PSO cached-blob mechanism achieves the same effect
  through a safe surface.

Design input came from a technical audit of the open-source prior art
project — the descriptor guard slot, the skipped-work/indirect-call taint
rule, PSO build lanes, the journal→offline-build model, and
VRAM-proportional budgets were adopted as concepts and re-implemented; no
code was copied.

**How it was verified (36/36 tests, 0 warnings).**

- Allocator tests on hardware (7): deterministic in-flight protection —
  including a frozen-queue test proving resources cannot be reclaimed
  mid-submission — a real evict/make-resident round-trip under budget
  pressure, and clean denial.
- `gpu_render` (3): clear + vertex-ID triangle on a 64×64 target with
  **pixels verified byte-exact by CPU readback**; a second render is
  pixel-identical with the pipeline built once.
- Cache suite (3): cold→persist→warm reuse with a real draw, corrupt-file
  recovery, and journal → offline replay → warm hits. Measured on a
  trivial lab PSO: cold compile+persist 13.8 ms vs blob-assisted create
  5.2 ms (≈2.65× faster; scales with shader size).

**Honest limits.** Offscreen render targets only — no swapchain/present
path; no compute pipeline lane or async PSO workers; no device-removed
recovery; the indirect-taint rule was designed but had no producer to
enforce on yet.

**What's next.** The shader translation service: guest container bytes in,
D3D12-bound shaders out — and the design of the command layer that will
feed it.

---

## Milestone 4 — The shader translation service

**Goal.** One translation architecture for guest shaders — compute first —
verified end-to-end on hardware.

**What shipped.**

- `mesia-shader`: a single `translate(guest_container_bytes) ->
  TranslatedShader` path over a the lab's shader-translation codepath, exposing
  neutral owned types only — the GPU layer binds results without seeing
  translator internals, so alternative translators could implement the
  same contract.
- A fixture writer producing **real guest containers**: synthetic fixtures
  carry genuine container parts (`DXIL`/`XDAT`/`XBCH`/`FSSM`) so they
  traverse the same code path a title's bytes would. Fixtures are
  hand-assembled GFX10 machine code — a constant-store program, a
  half-precision add, and the console compiler's own wave64
  `WaveActiveBitOr` OR-reduce expansion.
- The Shader Model 6.6 binding contract: user-data constants, root
  descriptors sorted by (slot, kind), heap-directly-indexed flags, and a
  `compute_translated` pipeline path through the dedup → cached-blob →
  compile layers.
- A required-config discovery: the host's maximum wave-lane count
  (`WaveLaneCountMax`, 32 on the RTX 3060) must be queried at engine init —
  without it, 64-lane guest code emits a `[WaveSize(64)]` shader the
  driver rejects. The limit is now folded into cache keys.
- Journal v2: entries persist the guest container itself, so offline
  replay re-runs the identical translate → bind → compile path.

**How it was verified (51/51 tests, 0 warnings).**

- `wave64_or_reduce_runs_on_the_host_gpu`: the 64-lane OR-reduce produced
  **0xFFFF_FFFF on the 32-lane host** — cross-lane emulation correct on
  real hardware (translate 833 µs; build+dispatch+readback 4.2 ms).
- `translated_const_store_lands_in_the_uav` and
  `translated_f16_rounds_in_a_documented_mode`: exact-match output dwords;
  the f16 rounding difference vs the console is a documented option.
- `wave32_variant_runs`, `repeated_dispatches_are_stable` (5 dispatches,
  1 build), `translated_compute_uses_the_persistent_cache`,
  `journal_replays_translated_compute_offline`.
- 8 CPU tests: stable/distinct cache keys, malformed-container rejection,
  and named `Unsupported`/`Isa` errors for invalid guest code — the engine
  never emits a wrong shader silently.

**Honest limits.** One translation program per coverage class (wave64
breadth is a single verified program); the f16 rounding option is an
environment flag rather than an engine option; sampler/resource-heap paths
are bound by contract but untested on GPU; no DXIL validator wired — the
driver's own validation is the verifier for now.

**What's next.** The graphics half: vertex/pixel shader fixtures with
signature parts and per-stage stage data, multi-stage binding, and a
translated VS+PS pair rendering pixels — plus the design audit for the
command layer.

---

## Milestone 5 — Translated graphics pipeline

**Goal.** Extend the translation path to graphics stages and render a real
triangle from translated guest shaders.

**What shipped.**

- Guest container support for graphics metadata: signature parts
  (`ISG1`/`OSG1`/`PSG1`) and per-stage stage-data tables (VS/PS semantics,
  `SPI_PS_INPUT_ADDR`, `DB_SHADER_CONTROL`, the MRT export-patch table),
  so VS/PS fixtures travel the same container path as compute.
- Hand-assembled graphics fixtures: a vertex-ID-driven position shader
  (`v_and`/`v_lshlrev`/`v_cvt` position math — every instruction word
  verified against the translator's own GFX10 decode tables) and a
  constant-color pixel shader, both with correct signature rows and
  hardware `exp` export encodings.
- `root_signature_for`: multi-stage binding — per-stage constant
  parameters, root views merged/sorted/deduplicated, per-stage samplers —
  with fingerprints that fold back to the single-stage layout.
- `graphics_translated`: a VS+PS pair becomes a deduplicated root
  signature plus graphics PSO through the same dedup → cached-blob →
  compile layering; the journal records both guest containers and
  render-target formats for identical offline replay.
- A command-layer design audit (read-only): the U-16 indirect-execution
  taint specification fully recovered from the technical audit, a guest-command IR
  and deferred-compile contract, state-invalidation policies, a sync
  model, and a hang/TDR risk register — this became the Milestone 6 plan.

This phase also piloted the lab's multi-agent build method: four isolated
agents in separate worktrees against a frozen interface contract,
integrated in a fixed order — zero file conflicts.

**How it was verified (64/64 tests, 0 warnings).**

- `translated_triangle_renders`: clear red → draw 3 → readback showed the
  expected bottom-left coverage — **2016/4096 texels, byte-exact**
  inside/outside colors.
- `translated_draw_repeated_stable` (3 draws byte-identical, built once),
  `translated_graphics_persistent_cache`,
  `translated_graphics_journal_offline`,
  `translated_vs_rejects_invalid_isa` (named error, never a wrong shader).
- 6 CPU tests: VS/PS translate to DXIL, host signature rows carry
  `SV_Position`/`SV_Target`, and exports to undeclared rows are named
  errors.

**Honest limits.** Graphics coverage is one fixture pair: no vertex fetch
or fetch shaders, no interpolated varyings, no VS resource reads, no
geometry/hull/domain/mesh stages, no depth/stencil, single render target.
Still no swapchain, no command layer enforcing the taint spec, no
device-removed recovery.

**What's next.** Build the command-translation engine against the audited
spec: a stream decoder, the typed IR, the indirect guard, and an executor
onto real command lists.

---

## Milestone 6 — The command-stream translation engine

**Goal.** Accept a recorded guest command stream, decode it into a typed
IR, validate and guard it, and execute it on the shared GPU surfaces —
without creating a second graphics engine.

**What shipped.**

- `mesia-command`: a lab stream format (`MESC`) → `decode` → a typed
  `GuestCommand` IR of 12 operations (`SetPipeline`, `SetRenderTarget`,
  `ClearRenderTarget`, `SetViewport`, `SetTopology`, `SetRootConstants`,
  `SetDescriptorHeap`, `Transition`, `Draw`, `Dispatch`, `CopyBuffer`,
  `ExecuteIndirect`) → `Recording` with tables, `validate()`, and the
  indirect guard → `execute_into` onto caller-owned D3D12 command lists.
- The U-16 indirect guard in its strictest audited form: one skipped
  producer poisons the whole recording until reset — a rewrite cannot
  prove it repaired every bound output — and `ExecuteIndirect` is
  neutralized to a driver-safe no-op (`ExecuteIndirect(sig, 0, args, 0,
  NULL, 0)`, never an infinite wait). `MESIA_INDIRECT_MAX = 65536` bounds
  the indirect path.
- A `Broken` pipeline state modeling a producer the host could not
  translate: dispatching on it skips the work and raises the taint.
- Executor conventions found by test and fixed at integration: scissor
  defaults to viewport (an unset D3D12 scissor clips everything), and the
  signature+PSO pair is emitted lazily and indivisibly at the first
  pipeline-dependent command.
- Same multi-agent build method as Milestone 5 — four worktrees, frozen
  contract, ordered integration, GPU runs serialized.

**How it was verified (100/100 tests, 0 warnings).**

- `command_stream_draws_the_triangle`: a decoded `Draw` → real triangle,
  pixel-exact.
- `command_stream_dispatches_compute`: translated guest machine code runs
  from the stream; readback dword exact.
- `compute_produces_indirect_draws_it`: compute writes the draw args
  on-GPU → `ExecuteIndirect` draws them — the happy path.
- `skipped_producer_neutralizes_indirect`: a `Broken` producer → the
  indirect call is neutralized; **zero texels written** — the protection.
- `invalid_stream_is_rejected`: bad magic, bad version, truncation,
  unsupported ops, and bad keys are each named at decode.
- `repeated_execute_is_stable`: 3 executions, byte-identical pixels, one
  pipeline build.
- Plus 20 decode edge-cases (including a hostile byte-flip sweep over the
  format), 7 guard truth-table tests, and 4 executor unit tests.

**Honest limits.** `MESC` is our own lab record format — it documents the
op subset we implemented and is **not** console record-format
compatibility. Missing: a scissor op in the IR (the convention covers it),
the count-buffer indirect path on GPU (parsed and emitted, untested),
multi-RT, indexed draws, richer state objects — and everything still open
from Milestone 5.

**What's next.** Breadth: vertex fetch and varying interpolation,
additional shader stages, depth/stencil, multi-RT, indexed and
count-buffered indirect draws, a swapchain/present path, and
device-removed recovery — all still verified on synthetic fixtures until
legal title content exists.
