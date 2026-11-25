# Contributing

## Ground rules

- **No game data, keys, or title binaries** — `cargo xtask policy` enforces
  this; the pre-commit hook runs it.
- **Rust stable**, `cargo fmt`, `cargo clippy`-clean code, `unsafe` blocks
  documented with a `// SAFETY:` comment.
- Tests: `cargo test --release --workspace` (≈500 tests; a few need a real
  D3D12 device — they skip or use WARP).
- Performance experiments behind `MESIA_*` env flags, off by default, with
  measurements in `docs/perf-study.md`.
- Commits in English; describe *why*, not *what*.

## Layout

`apps/mesia` (binary) · `crates/mesia-{core,formats,game,gpu,hle,loader,pe,pipeline,shader}` ·
`docs/` (architecture, perf study, audits) · `tests/` · `xtask/` (repo policy).

## Reporting bugs

Attach the `mesia.log` (INFO line per second has per-frame counters), GPU
model/driver, and the `MESIA_*` env configuration used.
