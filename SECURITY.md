# Security Policy

## What MESIA is — and is not

MESIA is a **translation layer**, in the spirit of Wine: the title's original
x86-64 code runs **natively inside `mesia.exe`**. It is **not a sandbox**.
Only run title binaries you trust — a hostile title can call into the
allow-listed host Win32 DLLs with your user privileges.

Network access from the title is denied by default: sockets, DNS and WinHTTP
are restricted to loopback (`crates/mesia-hle/src/modules/net.rs`). This is a
compatibility feature, not a security boundary.

## Why security tools may flag this project

The code legitimately uses OS mechanisms that pattern-match malware
heuristics — all of them **strictly inside its own process**:

| Pattern | Why it exists | Scope |
|---|---|---|
| `ReadProcessMemory` / `NtWriteVirtualMemory` | Fault-tolerant copies between GPU shadow memory and title memory | `GetCurrentProcess()` only |
| VEH + CONTEXT modification | Emulates Zen-2-only console instructions (clzero, mwaitx…) on Intel CPUs | own process |
| `VirtualProtect` RWX on title image | Relocations / TLS / IAT patching at load | own process |
| `SuspendThread` / thread contexts | Exit-time teardown, diagnostic stack dumps | own threads only |
| Generated x64 stubs | Import thunks; pages sealed `PAGE_EXECUTE_READ` after write | own process |
| Writes a patched title image, loads it | Windows loader semantics require a real PE on disk | cache dir only, `ensure_outside_repo`-guarded |

No cross-process injection (`CreateRemoteThread`, `VirtualAllocEx`,
`SetWindowsHookEx`, APC — zero uses), no persistence mechanism, no
telemetry, no network code of our own.

## Reporting a vulnerability

Open a private **GitHub Security Advisory** on
[MESIA-Unified/MESIA-Unified](https://github.com/MESIA-Unified/MESIA-Unified),
or an issue marked `[security]` if the finding is not exploitable.
Include the commit, the affected module and a reproduction.

## Supply chain

- All dependencies are crates.io versions pinned by the committed
  `Cargo.lock` (CI builds `--locked`). See `THIRD_PARTY_NOTICES.md`.
- No network access during build; `xtask` runs only `cargo`/`git`.
- Host DLL pass-through loads from `System32` only
  (`LOAD_LIBRARY_SEARCH_SYSTEM32`, regression-tested by
  `crates/mesia-hle/tests/no_dll_planting.rs`).
