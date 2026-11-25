# Changelog

All notable research milestones of the MESIA Unified project are documented
in this file, in the style of [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

**Dating.** The lab reports progress at month granularity; every milestone
below is dated `2026-11`.

**Verification.** MESIA Unified is developed in a private lab; this
repository publishes documentation only. Every entry below describes work
that was implemented and tested inside the lab against **synthetic
fixtures** — hand-built PE images, hand-assembled guest shader containers,
and a lab-defined guest command-stream format. No retail title, game file,
dump, or cryptographic key is used at any stage of development or testing.
Compatibility with demanding retail titles is a stated long-term ambition;
**no real-title progress is claimed anywhere in this changelog.**

Hardware-verified results were produced on a desktop NVIDIA RTX 3060 unless
noted otherwise. For the narrative behind each milestone, see
[docs/MILESTONES.md](docs/MILESTONES.md).

## [Milestone 6 — Command-stream translation engine] - 2026-11

The guest command layer: a recorded guest command stream is decoded into a
typed intermediate representation, validated, guarded, and executed onto
host D3D12 command lists through the shared device and pipeline-library
surfaces — no second graphics engine was created.

### Added

- Guest command-stream decoder (`MESC` lab record format) and a typed
  12-operation `GuestCommand` IR covering pipeline, render-target,
  viewport, topology, root-constant, descriptor-heap, transition, draw,
  dispatch, buffer-copy, and indirect-execution commands. Anything outside
  the implemented subset is rejected with a named error at decode time.
- Recording and validation layer with the U-16 indirect-execution guard:
  a producer that had to be skipped (e.g. because the host could not
  translate it) poisons the recording, and any `ExecuteIndirect` that
  could observe its missing writes is neutralized to a driver-safe no-op
  — `ExecuteIndirect(sig, 0, args, 0, NULL, 0)` — rather than executing on
  garbage. Guard state resets per recording; `MESIA_INDIRECT_MAX = 65536`
  bounds the indirect path.
- Executor that emits the decoded IR onto caller-owned D3D12 command
  lists, including GPU-generated argument buffers consumed by
  `ExecuteIndirect` (a compute dispatch wrote the draw arguments that the
  indirect call then consumed on-device).
- Verified end-to-end on GPU: a decoded `Draw` produced a real,
  pixel-verified triangle; a decoded `Dispatch` ran a translated guest
  compute shader; the guard test showed a skipped producer's indirect call
  writing zero texels.
- Built by parallel lab agents in isolated worktrees against a frozen
  interface contract, integrated in a fixed order — the same multi-agent
  methodology as Milestone 5.

### Verified

- **100/100 workspace tests, 0 warnings** — including 20 stream-decode
  edge cases (a hostile byte-flip sweep over the format), 7 guard
  truth-table tests, 4 executor unit tests, and 6 GPU end-to-end tests.
- All results are on **synthetic fixtures**. The stream format (`MESC`) is
  a lab record format that documents the implemented op subset; it is
  **not** console record-format compatibility.

## [Milestone 5 — Translated graphics pipeline] - 2026-11

The graphics half of shader translation: a translated guest vertex/pixel
shader pair now renders verified pixels.

### Added

- Guest container support for graphics metadata — signature parts
  (`ISG1`/`OSG1`/`PSG1`) and per-stage stage data — so hand-assembled
  vertex and pixel shader fixtures traverse the same container path as
  compute.
- Multi-stage root-signature binding: per-stage constant parameters,
  merged and deduplicated root views, per-stage static samplers; signature
  fingerprints stay stable with the single-stage layout.
- Translated-graphics PSO path: a guest VS+PS pair is bound into a
  deduplicated root signature and a graphics pipeline through the same
  dedup → persisted cached-blob → compile layering as compute; the journal
  persists both guest containers so offline replay re-runs the identical
  translation.
- Command-layer design study (audit only — nothing enforced yet): the
  U-16 indirect-execution taint specification recovered from the technical audit, a
  guest-command IR sketch, state-invalidation policies, and a hang/TDR
  risk register that became the Milestone 6 plan.
- Built by four parallel lab agents in isolated worktrees against a
  frozen interface contract — zero file conflicts at integration.

### Verified

- A translated guest VS+PS triangle rendered to a 64×64 target and
  readback showed the expected coverage — **2016/4096 texels, byte-exact
  inside/outside colors** — plus repeated-draw stability, persistent-cache
  reuse across library instances, offline journal replay, and named-error
  rejection of invalid guest code.
- **64/64 workspace tests, 0 warnings** — all synthetic fixtures; no title
  content.

## [Milestone 4 — Shader translation service] - 2026-11

A single guest→host shader translation architecture: guest container bytes
in, translated shader out, bound to D3D12 without the GPU layer seeing
translator internals.

### Added

- Translation service exposing neutral owned types only
  (`translate(guest_container_bytes) -> TranslatedShader`), running over a
  the lab's shader-translation codepath, so alternative translators could implement
  the same contract. Exactly one translation path exists in the engine.
- Fixture writer that produces **real guest containers** — synthetic
  fixtures take the same code path a title's bytes would. Fixtures are
  hand-assembled GFX10 machine code: a constant-store program, a
  half-precision add, and the console compiler's own wave64
  `WaveActiveBitOr` OR-reduce expansion.
- Shader Model 6.6 binding contract for translated shaders: user-data
  constants, root descriptors, heap-indexed flags, translated compute
  pipelines through the dedup/cache layers.
- Required host configuration discovered and encoded: the host's maximum
  wave-lane count (`WaveLaneCountMax`) is queried at engine init, and the
  wave limit is folded into cache keys. Without it, 64-lane guest code
  emitted a `[WaveSize(64)]` shader the driver rejected.
- Journal v2: entries persist the guest container, so offline replay
  re-runs the identical translate → bind → compile path.

### Verified

- The 64-lane OR-reduce produced **0xFFFF_FFFF on the 32-lane host** —
  wave-width emulation verified on real hardware — alongside exact-match
  constant stores, a documented f16 rounding mode, repeated-dispatch
  stability, persistent-cache reuse, and offline journal replay.
- **51/51 workspace tests, 0 warnings** — all synthetic fixtures; no title
  content.

## [Milestone 3 — Real GPU resources and first verified render] - 2026-11

From accounting model to real resources: D3D12 resources behind the
budget/fence adapter, descriptor heaps, pipeline building, and the first
readback-verified render.

### Added

- GPU allocator binding allocation IDs to real `ID3D12Resource`s with a
  budgeted `alloc` path (suspend → evict → deny escalation), real
  `Evict`/`MakeResident` residency, and **explicit reclaim** — allocations
  are never silently evicted; the caller picks victims.
- Descriptor heaps implementing an audited safety contract: a permanent
  null guard slot plus null-fill at creation, so a view that was never
  written can never fault the GPU; the practical per-heap descriptor cap
  is measured and documented.
- Pipeline library deduplicating by canonical description, a shared empty
  root signature, offscreen render targets, and a readback path.
- **First render**: a vertex-ID-driven green triangle over a red clear,
  verified byte-exact by CPU readback.
- Persistent PSO cache: per-pipeline `D3D12_CACHED_PIPELINE_STATE` blobs
  harvested after each build, hash-verified before reaching the driver,
  and supplied on the next run; a `MESIAJNL` journal of every requested
  description can be replayed offline into a warm cache. Measured on a
  trivial lab PSO: cold compile+persist 13.8 ms vs blob-assisted create
  5.2 ms (≈2.65×).
- Documented design reversal: `ID3D12PipelineLibrary` was tried first and
  rejected — a malformed blob escalated to process-wide device removal.
  The per-PSO cached-blob mechanism provides the same benefit with a safe
  surface.
- PSO build lanes (urgent/graphics/compute, at most four parallel builds —
  each driver compile costs roughly a gigabyte of memory on the test
  machine) designed from the audit findings.
- External design input: a technical audit of the open-source
  the audited project informed several mechanisms — the descriptor guard
  slot, the skipped-work/indirect-call taint rule, PSO build lanes, the
  journal→offline-build model, and VRAM-proportional budgets. Concepts
  were re-implemented; no code was copied.

### Verified

- **36/36 workspace tests, 0 warnings** — allocator tests on hardware
  (in-flight allocations provably never reclaimed, including a
  frozen-queue determinism test), the readback-verified first render, and
  the cache/journal suite. All synthetic fixtures.

## [Milestone 2 — Entry-point execution and a single GPU backend] - 2026-11

Mapped images execute, and the engine gets its single D3D12 device owner.

### Added

- `LoadedImage` lifecycle state machine (Cold → Attached → Detached)
  honoring Windows ordering — TLS callbacks before `DllMain` attach,
  `DllMain` before TLS detach — with exactly-once guards, rollback on
  refusal, and detach-on-drop.
- Entry-point execution: a mapped executable's entry point runs on a
  controlled thread; the fixture reports back through imports bound to
  test stubs, proving the HLE policy layer was live mid-execution.
- GPU device layer: the single `ID3D12Device` owner (high-performance
  adapter preference with fallback), a direct queue, fence epochs, and
  submit/wait.
- Deferred-retire lane generalizing prior-art practice to explicit epochs —
  nothing is freed while the GPU may still use it — plus device-removed
  detection that maps a `u64::MAX` fence failure to a
  `GetDeviceRemovedReason`-backed error and has waiters re-sample after
  waking.
- Fence-guarded memory pool adapter: budget + LRU eviction, but only
  among allocations whose last submitted use has completed; pinned
  ethe reference engineusion; clean `BudgetExhausted` denial when every candidate is in
  flight.

### Verified

- **21/21 workspace tests, 0 warnings** — lifecycle tests (attach/detach
  exactly once, end-to-end EXE run on a spawned thread), deterministic
  pool tests over a mock clock, and 5 device tests on hardware including
  a **byte-exact UPLOAD→DEFAULT→READBACK copy**. All synthetic fixtures.

## [Milestone 1 — Unified loader and the memory contract] - 2026-11

The first real merge: not two loaders side by side, but one intermediate
representation, one mapper, one binding contract.

### Added

- Shared `PreparedImage` intermediate representation: one prepare pipeline
  (one reference engine's `prepare()` — its strength is loadconfig/CFG/CHPE patching)
  feeds one lab-written manual mapper in the field's manual-mapper style (allocation, section
  copy, relocations, import table, TLS, protections), with import binding
  routed through pluggable resolver and missing-import policy traits so
  either reference engine's HLE tables can drive it.
- Cross-engine PE-facts harness: both reference parsers diffed
  field-by-field over a 49-image synthetic corpus, plus a byte-parity
  methodology — snapshots are normalized (un-rebased) so two mappings of
  the same file are comparable regardless of ASLR, and everything outside
  a file-derived allowed set must be byte-identical.
- Host memory arena contract with a lazy-commit implementation on real
  reserved/committed pages: a hard budget enforced by LRU page eviction,
  transparent re-commit on re-touch — the minimal model of the
  commit-on-demand memory fix the engine targets.
- Unified configuration: paths plus per-subsystem backend selection.
- Documented divergences found by the parity harness, including a
  normalization bug in our own test code (caught by the harness — exactly
  its purpose) and the discovery that one reference engine binds several kernel32 imports
  to its own instrumentation shims — now modeled as a policy choice
  rather than a discrepancy.

### Verified

- **10/10 workspace tests, 0 warnings** — parser agreement across the
  corpus, byte-parity of the unified mapper against independent
  reference pipelines on the same image, a plain host-DLL mapping sanity
  test, and the arena contract tests. All synthetic fixtures; no title
  content.
