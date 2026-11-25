# Roadmap

Status: research codebase. Retail boots, prologue runs; heavy scenes ~1–2 fps.

## Now (correctness)

- [x] Write-watch on fixed-base reservations (stale-image fix, `6e22607`)
- [x] Parallel copy-back merges — `MESIA_WIDE_MERGE` (O1, gated, `a06cfaa`)
- [ ] Second descriptor heap for >1M title views (wrong/black textures)
- [ ] `card_back` merge grouping — done (`merge_groups`), pending bench
- [ ] `E_INVALIDARG` compute pipelines: dump refusing DXIL, Load-vs-Create log

## Next (performance, each measured independently)

- O2: batch small buffer uploads into one staging buffer
- O3: pipeline card-side compares across frames
- O4: synthetic D3D12 reference scenes + pixel-diff harness (does not exist yet)
- O5: O1 heavy benchmark on synthetic workload; enable by default only if proven

## Later

- Ray-traced lighting emulation (BVH path exists, untested on HW)
- `dxil.dll` validation in CI
- GPL-2.0-only files: resolve license conflict (reimplement or upstream grant)

## Explicitly out of scope

- DRM/copyright circumvention — titles must be pre-decrypted
- Network play (loopback-only by design)
