# Changelog

## Unreleased

### Fixed
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
- Attribution line corrupted by the rename (`VIctory`/`victory-fork`).

### Added
- `MESIA_WIDE_MERGE=1` — parallel copy-back merges (O1), off by default.
- Test: fixed-base reservation write-watch tracking (xmem).
- Docs: `test-inventory.md`, `memory-coherency.md`, `d3d12-validation.md`,
  `concurrency-validation.md`, `SECURITY.md`, `THIRD_PARTY_NOTICES.md`,
  `CONTRIBUTING.md`, `ROADMAP.md`, `CHANGELOG.md`.

### Known issues
- 2 flaky GPU tests under contention (`ds_min_max_f32`, `waits_end_on_ticks`)
- >1M view-heap: wrong/black textures in heavy scenes
- `E_INVALIDARG` on 2–3 translated compute pipelines
- GPL-2.0-only/GPL-3.0 module conflict (see THIRD_PARTY_NOTICES.md)
