# Security audit

_Last run: 2026-11-25, commit `5c527dc`. Reproduce: `cargo audit`._

## Dependency vulnerabilities

`cargo audit` (RustSec advisory-db, 1567 advisories) over `Cargo.lock`
(101 dependencies): **0 vulnerabilities, 0 warnings**.

## Trust model

MESIA is a translation layer (Wine-style), not a sandbox — title code
runs natively in-process. See `SECURITY.md` for the full model and why
security tools may flag normal emulator mechanics (VEH, own-process
memory APIs, generated code pages, patched image on disk).

## Hardening applied

| Change | Commit |
|---|---|
| `dxil.dll` via `LoadLibraryExW` + `LOAD_LIBRARY_SEARCH_SYSTEM32` (was bare `LoadLibraryW` — app-dir DLL planting) | `46cb06a` |
| `ensure_outside_repo` gate on `MESIA_CAPTURE`, `MESIA_DUMP_MATCH`, `MESIA_FAIL_DUMP`, `MESIA_EMIT_DUMP`, `MESIA_SHADER_CENSUS` dump paths | `5c527dc` |

## Findings assessed, not applied

| Finding | Assessment |
|---|---|
| Cross-process injection patterns | None present (`CreateRemoteThread`, `VirtualAllocEx`, hooks, APC: zero uses). All memory APIs target `GetCurrentProcess()`. |
| Network code | None of our own; title syscalls restricted to loopback (`hle/net.rs`). Not a security boundary. |
| Secrets | None in tree; keys are user-provided at runtime, never logged or committed (`xtask policy` enforces). |
| Build-time network access | None; deps pinned by `Cargo.lock`, CI uses `--locked`. |
| UNC/named-pipe paths | Title file paths can name `\\server\share` — Wine-consistent behavior, documented as out-of-sandbox-scope. |

## Known legal/licensing debt

- `console_insn*.rs` (ported from XCL) is GPL-2.0-only inside a
  GPL-3.0-or-later work — see `THIRD_PARTY_NOTICES.md`.
- `bcpack-tables.bin` — proprietary Microsoft format constants,
  documented exception (`docs/asset-policy.md`).
