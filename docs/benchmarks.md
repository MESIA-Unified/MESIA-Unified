# Benchmarks

Two kinds: ignored in-tree benchmark tests (synthetic, reproducible
anywhere) and real-title frame counters (the ground truth).

## In-tree benchmarks

Run with `cargo test -p <crate> --release -- --ignored <name> --nocapture`:

| Benchmark | Measures |
|---|---|
| `host::batch::texture_checks_per_submission` (mesia-gpu) | per-submission texture check cost |
| `shadow::wide_tests::heavy_frames_of_merges_run_wide_and_merge_the_same` (mesia-gpu) | O1 copy-back merge: sequential vs `MESIA_WIDE_MERGE` threads — asserts byte-identical output (484 MiB/frame × 6 frames) |
| `emit::bitcode::tests::bench_write` (mesia-shader) | DXIL bitcode emission throughput |

Each prints wall-times; they are synthetic — no title data involved.

## Real-title counters

`mesia.log` emits a per-second INFO line with the engine's own counters
(copy-back ms, upload counts, pipeline compiles, present times). Measure
a change as:

1. same title section, same `MESIA_*` config, 3 runs each side;
2. compare the counter that the change targets, and frame time
   p50/p95 from the log's present timings;
3. report in `docs/perf-study.md` under the optimization's entry.

## Rules

- Never present a projected gain as a measurement.
- An optimization stays behind its `MESIA_*` flag (off by default) until
  a benchmark shows the gain *and* the same workload produces
  byte-identical output.
- Record hardware (GPU/driver/RAM) alongside numbers — absolute ms are
  machine-bound.
