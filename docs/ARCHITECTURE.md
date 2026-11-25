# Architecture

```
 user's files (outside repo)             MESIA
 ───────────────────────────             ─────────────────────────────────────────
 game assets ──────────────────────────► mesia-pipeline + mesia-game
                                         (mesia-formats, mesia-shader)
                                                    │
                                                    ▼
                                         cache_dir: converted data
                                                    │
 game executable ─► mesia-loader ────► title code (native x86-64)
                    prepare, map, bind      │ imports
                                            ▼
                                         mesia-hle ── host Windows (shared APIs)
                                            │ D3D12.X
                                            ▼
                                         mesia-gpu ─► host D3D12
                                            │ shaders at pipeline creation
                                            ▼
                                         mesia-shader ◄─► cache_dir
```

Shaders are translated when the title creates a pipeline, from the bytes it
passes, wherever it loaded them from (executable, archive, memory).
Converting them ahead of time from files is only a warm-up.

## Crates

| Crate | Role |
|-------|------|
| `mesia-core` | config, asset-policy guard, logging |
| `mesia-pe` | read-only PE32+ and import-library analysis |
| `mesia-loader` | prepare, map, bind and start the title image |
| `mesia-hle` | console API implementations, host pass-through policy |
| `mesia-gpu` | D3D12.X objects, command lists and GPU memory → host D3D12 |
| `mesia-shader` | Xbox shader containers → host shaders |
| `mesia-formats` | the GPU's texture layouts (sizes, tiling, detiling), audio codecs; planned: BCPack |
| `mesia-pipeline` | converter trait, content-addressed cache |
| `mesia-game` | the title-specific formats, build identities, hooks |
| `apps/mesia` | command-line front end |
| `xtask` | asset-policy check, git hooks |
| `tests/fixtures/hle-probe` | a stand-in "title" (our own code) for loader tests |

Everything except `mesia-game` is platform-level and can be tested with
any Xbox Series title.

## Import resolution

For every `dll!function` the title imports, in order:

1. our implementation in `mesia-hle::modules` (always wins);
2. the host's own function, if `HostPolicy` allows that DLL;
3. an export of a DLL the title ships beside its executable, mapped like
   the executable (only DLLs neither of the above serves);
4. a generated stub that names the import when called (`--unimplemented
   trap` breaks into the debugger and aborts; `zero` logs once and returns 0).

`LoadLibrary*` of a console-only DLL we implement (e.g. `xgameruntime.dll`)
returns a pseudo handle; `GetProcAddress` on it finds our functions.
`LoadLibrary*` of a title DLL returns its mapped image, mapping it first if
nothing imports it. By name, as by import, a title gets only host DLLs
`HostPolicy` passes through, and `GetProcAddress` on one returns our
override where import binding would.
`QueryApiImpl` hands out our runtime objects by provider and interface id
(`mesia-hle::modules::xgameruntime` lists the areas); online areas answer
as an offline console. Game saves are files under `MESIA_DATA_DIR`.

Title storage is a drive on the console, and titles build paths on that.
`mesia-hle::modules::files` binds every path-taking title import
(kernel32 and the C/C++ runtime) and rewrites paths on a virtual drive to
the host folder behind it; persistent local storage is one such drive over
a folder under `MESIA_DATA_DIR`. Other paths reach the host unchanged.

An exe's code reaches its static TLS as slot 0; mapped as a DLL it gets
another slot, so the loader redirects those loads (`mesia-loader::tls`).
`mesia run --execute` installs a crash reporter that names faults as
`module+rva` and unwinds the faulting thread's stack
(`mesia-loader::crash`).

`mesia census` shows this classification without running anything (it
does not map title DLLs; `mesia run` without `--execute` does).

## Shaders

`mesia_shader::ir::build` turns a native program into SSA, one
instruction per machine instruction. `mesia_shader::structure` then
removes EXEC: lane masks become per-lane booleans, wave-wide mask tests
become per-lane branches, and vector instructions keep a guard only where
a lane can reach them with its EXEC bit clear. `mesia_shader::resource`
traces each descriptor and scalar memory address to the root parameter it
comes from (table, heap, root descriptor or constant). A descriptor a
waterfall loop reads from the first lane (`v_readfirstlane_b32`) is traced
to the per-lane load or choice that gives each lane its own (a mask's
`v_cndmask_b32`, or an if/else whose arms each set it: traced on the paths
the lanes take, from structurisation), and accessed with a non-uniform heap
index in one pass. `mesia translate`
runs every shader it finds (containers, SGD5 files) through these and
lists what they cannot handle yet.
