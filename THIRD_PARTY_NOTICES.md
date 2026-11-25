# Third-Party Notices

## Upstream projects

| Project | Author | License | Used as |
|---|---|---|---|
| **VIctory** | Marvisak | GPL-3.0-or-later | Base of this fork (whole codebase) |
| **victory-fork** | fxceless | GPL-3.0-or-later | Intermediate fork this descends from |
| **XCL** | ivall | **GPL-2.0-only** | `crates/mesia-loader/src/console_insn.rs` + `console_insn_decode.rs` (ported files, notices preserved in-file) |

⚠️ **License note (inherited from upstream):** the two files ported from
XCL are GPL-2.0-only inside a GPL-3.0-or-later work. The licenses are not
compatible for relicensing; the files keep their original GPL-2.0-only
notices and are treated as a separable module. Downstream redistributors
should review this before shipping binaries. Tracked as a known legal
debt — see ROADMAP.md.

## Rust dependencies (crates.io, pinned by `Cargo.lock`)

anyhow (MIT/Apache-2.0) · blake3 (CC0/Apache-2.0) · clap (MIT/Apache-2.0) ·
memchr (MIT/Unlicense) · miniz_oxide (MIT/Zlib/Apache-2.0) · object
(Apache-2.0/MIT) · **opus-pure** (BSD-3-Clause) · serde (MIT/Apache-2.0) ·
tempfile (MIT/Apache-2.0) · thiserror (MIT/Apache-2.0) · toml
(MIT/Apache-2.0) · tracing (MIT) · tracing-subscriber (MIT) · walkdir
(MIT/Unlicense) · windows / windows-sys / windows-core (MIT/Apache-2.0,
Microsoft) · and their transitive dependencies (see `Cargo.lock`).

## Data files

| File | Status |
|---|---|
| `crates/mesia-formats/src/bcpack-tables.bin` | Constant tables for the proprietary Microsoft BCPack format. **Not covered by the repository license** (see `docs/asset-policy.md`); retained for format compatibility research. |

## No title content

No game data, keys, or copyrighted title content is committed. The
`xtask` asset policy (`cargo xtask policy`) and `ensure_outside_repo`
enforce this mechanically.
