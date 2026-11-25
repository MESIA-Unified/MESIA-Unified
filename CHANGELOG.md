# Changelog

## Unreleased

### Fixed
- **Translate/device parity** — `emit::host::Host::new` now queries
  `D3D12_FEATURE_D3D12_OPTIONS1` like the game does: `translate
  --pipelines` reproduces production exactly (wave64-stuck refusals are
  now diagnosable offline). Compute root signatures no longer carry the
  input-assembler flag, illegal there.
- **Unit-test contamination** — `Host::new` leaves `HOST_MAX_WAVE` alone
  under `cfg(test)`: the global setting made later emit tests lower for
  32 lanes (a serial suite failure introduced by the parity fix).
- **Write-watch journal** — pages freed and re-mapped at the same
  address still reported writes from their previous life
  (over-copying, and the journal grew without bound); now pruned on
  `title_memory_remapped`. This was the real cause of the
  "flaky" `written_pages_are_reported_to_every_asker` — deterministic.
- **`merge_groups`** — O(n²) → O(n log n) sorted sweep (0.177 ms on 2k
  spans vs ~30 ms of compares), proven equivalent to the pairwise union
  on 336 generated sets; an empty-span edge case it exposed is fixed.
- **OOM paths** — `pooled()` is fallible (`try_reserve`), three `fail()`
  sites degrade (dirty ranges re-queued, compare skipped → whole fetch,
  fetch buffer kept on failed grow + chunked fetches) instead of
  aborting under memory pressure.
- **Stale rendered data** — `MEM_WRITE_WATCH` now applies to reservations
  at the title's own address, not only at addresses we chose; shadow
  copies track title writes per page. `MESIA_UNWATCHED_FRAMES` back to 1.
- **O1 merge race** — merge grouping used `shadow.start`-shifted spans
  while work items are already title addresses; two shadows covering the
  same title bytes could merge concurrently. Fixed via `merge_groups`
  (union-find on title spans).
- **`card_back` chunk race** — card-side merge chunks had the same
  overlap blind spot; now grouped by `merge_groups` too.
- **`host_transition` TOCTOU** — present path read/tracked state via
  swap+restore outside the submission lock; replaced with a plain
  `host_state` read (the present copy restores the state).
- **State drift** — `card_before` hash pass left buffers in
  NON_PIXEL_SHADER_RESOURCE; now restored to COPY_DEST.
- **DLL search order** — `dxil.dll` loaded via `LoadLibraryW` by bare
  name; now `LoadLibraryExW` + `LOAD_LIBRARY_SEARCH_SYSTEM32`.
- **Dump path policy** — `MESIA_CAPTURE`, `MESIA_DUMP_MATCH`,
  `MESIA_FAIL_DUMP`, `MESIA_EMIT_DUMP`, `MESIA_SHADER_CENSUS` could write
  game-derived files inside the source tree; now gated by
  `ensure_outside_repo`.
- Attribution line corrupted by the rename (`VIctory`/`victory-fork`).

### Added
- `MESIA_WIDE_MERGE=1` — parallel copy-back merges (O1), off by default;
  synthetic benchmark shows 1.7× with byte-identical output
  (`heavy_frames_of_merges_run_wide_and_merge_the_same`). Now covers the
  `relay_card_back` path — the dominant merge path under RELAY=2, which
  the flag previously never reached.
- **Pipeline dedupe** — identical (shaders, state) pipelines share one
  `Pending` via a `Weak` map keyed on the blake3 pipeline key; the
  library save runs only once the make queue drains (no mid-storm
  serialize stalls).
- `tests/synth_scene.rs` — native D3D12 reference renderer with golden
  images and an inter-frame freshness check (bit-identical frame 1 on
  RTX 3060 and WARP; WARP second-frame crash is an open issue).
- `pool_tests`, `merge_groups` equivalence tests (336 sets).
- Test: fixed-base reservation write-watch tracking (xmem).
- Docs: `test-inventory.md`, `memory-coherency.md`, `d3d12-validation.md`,
  `concurrency-validation.md`, `testing.md`, `benchmarks.md`,
  `security-audit.md`, `SECURITY.md`, `THIRD_PARTY_NOTICES.md`,
  `CONTRIBUTING.md`, `ROADMAP.md`, `CHANGELOG.md`.

### Known issues
- `written_pages_are_reported_to_every_asker` — fixed (was journal staleness, not flaky)
- GPU TDR under parallel test load (`v_cvt_norm` once) — transient, retried
- >1M view-heap: wrong/black textures in heavy scenes
- `E_INVALIDARG` on wave64-stuck compute pipelines — root cause confirmed:
  cross-half-wave code (LDS shared across a >32 thread group, lane masks
  crossing lane 32, `v_mbcnt`) needs lowering machinery that does not exist
  yet; `MESIA_WAVE_WIDE=1` recovers some, at documented GPU-hang risk
- WARP faults inside its software rasterizer on synth_scene frame 2
- GPL-2.0-only/GPL-3.0 module conflict (see THIRD_PARTY_NOTICES.md)
