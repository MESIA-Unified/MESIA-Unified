# Changelog

## Unreleased

### Fixed
- **>1M view-heap** — first attempt (dedicated null slot, `f59ee56`)
  made out-of-range views defined-null: correct but invisible, and the
  table covers enough real views that it showed as a mass black screen.
  REVERTED (`25a95f1`) — indices land on dynamic slots again (wrong but
  visible). True fix stays: second host heap + index-range select.
- **Black screen / silent run** — resolved live: the null-slot clamp,
  not the compile storm. Display + in-game audio confirmed restored by
  the user.
- **Permanent unmirrored heap** — `Heap::host()` cached `None` on a
  transient `HostHeap::new` failure; now built before publishing.
- **Pipeline create-path waste** — thundering herd (every `make` woke
  all waiters → per-Pending condvar), `describe()` re-formatted 3× per
  pipeline (OnceLock), QUADS/TRIANGLE dedupe miss, root signatures
  recreated per pipeline (now shared), re-hashed key inside the
  PENDINGS lock, per-pipeline container re-parse (memoized).
- **Sleep precision** — `std::thread::sleep` truncates to whole ms;
  frame waits now use a high-resolution waitable timer (overshoot −43%
  on 4 ms waits).
- **Copy-back scratch waste** — fresh zeroed 64 KiB per call → pooled
  grow-only; per-piece clear+resize → grow-only; unmapped merge is a
  plain copy (equivalence-tested).
- **Land-time AVs** — `Written::write`/`read_backs` wrote title memory
  at land time; unmapped pages now skipped via `watch::writing` +
  `write_mapped`. Corrupt `UserData.index` in the shader cache could
  panic a draw. Stale-address-keyed tables (`RANGES`, `m.heaps`) now
  keyed by live COM pointer; three unbounded title-controlled
  collections capped.
- **`synth_scene` WARP crash** — root cause was a use-after-free: the
  RTV descriptor heap was dropped at the end of `Scene::new` while the
  stored `rtv_a`/`rtv_b` handles pointed into it and were recorded into
  every command list. Now kept alive in `Scene`; the WARP skip is
  removed, both frames bit-identical on hardware and WARP.
- **Heap-buffer sweep spikes** — the full 83k-slot rescan ran every 16
  passes (0.4–1.9 s inside one frame); now a rotating 1/16 slice per
  pass, same staleness guarantee, ~1/16 of the cost per frame.
- **GPU test contention** — `diff_tests` ran real compute work without
  the `ONE_AT_A_TIME` lock every other GPU test holds; plausible source
  of the observed TDR under parallel load. Serialized.
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
  images and an inter-frame freshness check (bit-identical on RTX 3060
  and WARP, both frames).
- `tests/compute_correctness.rs` — six original-HLSL checks with exact
  CPU references on hardware and WARP: groupshared barrier reductions,
  wave intrinsics, divergent-branch writes, 256-SRV descriptor table,
  null-descriptor reads, compute→pixel interop.
- `tests/heavy_load.rs` — reproducible heavy synthetic load (thousands
  of draws, multi-pass, timestamp queries) printing p50/p95/p99, CPU vs
  GPU ms, RAM/VRAM; gated behind `MESIA_HEAVY=1`. Sustained run: 1500
  frames / ~5 min, p50 192 ms GPU, RAM flat at 96 MiB, hash stable.
- `tests/compute_advanced.rs` — second-wave checks: groupshared bank
  patterns, UAV atomics, lane exchange, mask bitcount, UAV RAW hazards,
  bitwise determinism, f16/unorm edges (7/7 HW+WARP).
- `pool_tests`, `merge_groups` equivalence tests (336 sets).
- Test: fixed-base reservation write-watch tracking (xmem).
- Docs: `test-inventory.md`, `memory-coherency.md`, `d3d12-validation.md`,
  `concurrency-validation.md`, `testing.md`, `benchmarks.md`,
  `security-audit.md`, `SECURITY.md`, `THIRD_PARTY_NOTICES.md`,
  `CONTRIBUTING.md`, `ROADMAP.md`, `CHANGELOG.md`.

### Known issues
- `written_pages_are_reported_to_every_asker` — fixed (was journal staleness, not flaky)
- >1M view-heap: out-of-range views now read null (defined), not wrong;
  true fix = second host heap + index-range select (P1 backlog)
- Cross-process GPU contention (heavy_load + test suite at once) can
  TDR a long dispatch — run GPU tests while the GPU is free
- `E_INVALIDARG` on wave64-stuck compute pipelines — root cause confirmed:
  cross-half-wave code (LDS shared across a >32 thread group, lane masks
  crossing lane 32, `v_mbcnt`) needs lowering machinery that does not exist
  yet; `MESIA_WAVE_WIDE=1` recovers some, at documented GPU-hang risk

- GPL-2.0-only/GPL-3.0 module conflict (see THIRD_PARTY_NOTICES.md)
