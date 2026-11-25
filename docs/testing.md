# Testing

## Running

```sh
cargo test --release --workspace        # whole suite (~500 tests, ~3 min)
cargo test -p mesia-gpu --release       # GPU crate only (111 tests, ~90 s)
cargo test -p mesia-gpu --release <name>  # one test
```

Release mode is the supported configuration — several tests time GPU work
and debug builds skew them.

## Inventory

Per-crate counts and the categorization (pure-CPU / WARP-needed / real-GPU)
live in `docs/test-inventory.md`. Reproduce it with
`cargo test --release --workspace -- --list`.

## Tiers

| Tier | Needs | Examples |
|---|---|---|
| CPU-only | nothing | shadow merges, DXIL parse, xmem policy |
| D3D12 device | any adapter | pipeline creation, heaps, fences |
| WARP | none (software) | `warp_*` tests — deterministic |
| Real GPU | hardware adapter | `ds_min_max_f32`, dispatch checks |

Tests that cannot get a device skip or use WARP rather than fail.

## Known flaky tests

Two tests are timing-sensitive under full-suite contention and have been
observed to pass in isolation while failing inside a parallel suite run:

- `emit::valu_tests::ds_min_max_f32_order_floats_and_return_the_old_value`
  (real-GPU dispatch; ~77 s alone)
- `frame::tests::waits_end_on_ticks_and_count_frames` (timing windows)

Both are documented in `docs/test-inventory.md`. They are scheduling
artifacts, not correctness failures; do not remove them — if one fails in
a suite run, re-run it alone before investigating.

## Benchmarks

`#[ignore]`d benchmark tests (run explicitly, not in CI):

```sh
cargo test -p mesia-gpu --release -- --ignored texture_checks_per_submission
cargo test -p mesia-shader --release -- --ignored bench_write
```

They print wall-times via `--nocapture`. See `docs/benchmarks.md`.

## Policy

`cargo xtask policy` enforces the no-game-data-in-repo rule; the
pre-commit hook runs it. `ensure_outside_repo` does the same at runtime
for every path MESIA writes to (see `docs/asset-policy.md`).
