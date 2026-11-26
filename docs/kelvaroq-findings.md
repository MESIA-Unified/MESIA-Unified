# Kelvaroq audit — raw findings (input for the 5 deliverable docs)

Repo: github.com/stellarr-studio/kelvaroq @ 1270c422 (v0.2.0), cloned to
C:\Users\utilisateur\Desktop\kelvaroq-src (read-only audit only, nothing executed).
License: GPL-3.0-or-later (root + Cargo.toml). MESIA is GPL-3.0-or-later →
code reuse legal under GPL §5: keep GPL, add attribution notice
("derived from Kelvaroq, © 2026 Stellarr Studio SAS"), mark modifications,
publish source. Do NOT use the Kelvaroq name/logo. `research/` subtree is
GPL-2.0-or-later (Unicorn dep) — sublicensable upward, keep notice.

## What the repo actually is

The ENGINE is not in the repo. The repo ships: CLI/launcher (~50kLOC src/),
native/kelvaroq-render crate (~1.3kLOC, 9 modules), and
native/patches/00NN-*.patch = **65 patches applied at build time onto
github.com/fxceless/victory-fork @ f85101b** by scripts/build_native.py
(pinned SHA, reproducible). victory-fork is the same lineage MESIA's crates
mirror (loader/hle/gpu/shader/formats/pipeline) → patches are near-directly
applicable. All code is AI-generated ("Co-authored-by: Moon AI"); ~40
mechanisms tagged "Approach after XCL <sha>" — XCL is GPL-2.0-only →
DIFF those five sites before copying (adapters.rs:10, hardware.rs:31,
native.rs:27, Storage.cs:95, Archive.cs:186). Repo created ~24 Nov 2026 —
3 days old, 104★/27 forks, fork-bait risk; engineering docs are candid
(they publish regressions + "no global FPS gain demonstrated").

## Claims verdict

- 10 fps prologue, 7-8 fps heavy, A/B measured — REAL on RTX 5080/4090
  (9950X3D, 64GB). NOT extrapolable to our RTX 3060/16GB.
- 36 min external playtest no crash — real, single session.
- "Unified memory 100% shared / copies 100MB→8MB / 18s→60ms" — IN TESTING,
  opt-in VICTORY_UMA*, NOT in default build, not measured in game.
- "#1 of 44 tracker" — marketing (trackers show ~#5-6).
- CI = fmt/clippy/unit only, zero GPU validation. No raw logs published.

## Mechanisms (ranked by MESIA relevance — MESIA pains: ~200MB/frame
kernel copies via ReadProcessMemory, serialized GPU waits under `submitted`
lock ~150-200ms, copy-back 78-173ms, present drains 3-4×/frame, indirect
mesh draws exist (indirect.rs+mesh pipelines), CARD_COMPARE exists)

1. **0044 plaincopy.rs (~420LOC, quasi-standalone)**: vectored rep-movsb
   in-process copy + VEH recovering READ faults only, 64KB block blacklist
   2s, <256B→kernel. Replaces ReadProcessMemory entirely. Their numbers:
   7-12% frame + >1s spikes gone. → direct hit on MESIA copy_mapped cost.
   Caution: writes must stay inside page guards.
2. **0041 amplification-launch for indirect mesh draws (~300LOC)**: AS
   reads the arg record on-GPU, clamp+budget via atomics, DispatchMesh;
   removes clamp pass + 2 UAV waits per draw (3200/3950 waits→~0);
   p50 198→120ms there. Needs: mesh pipeline + 4B payload + budget buffer.
3. **0046+0048 bounded async present (~130LOC)**: FrameLimit mutex+condvar,
   presents enqueued Held::Present, FRAMES_AHEAD=2 clamp 1-8; p95 568→262ms.
   Conceptually MESIA's LATE_WAIT=3 present-flight but bounded/queued —
   relevant to our stage-3 crash residual (see HANDOFF-ASTRA.md).
4. **0057 GPU masked-patch copy-back relay_patch.rs (~540LOC)**: compute
   shader Load→mask→Store applies CPU dword edits on-card without recopy
   (cap 128 pages); ~15ms/frame avoided. → MESIA copy-back is 78-173ms.
5. **0055 incremental descriptor mirror (~700LOC)**: write-watch the guest
   descriptor heap; kernel page snapshot; record_key=12B fingerprint
   (tag+2 payload words) → only changed slots processed (200k inspected→
   ~9k changed/frame); TableRecords cache keyed by packed keys with lazy
   release epochs; BufferPass BTreeSet span dedup.
6. **0053 watch/ahead.rs speculative refresh snapshots (~360LOC)**: worker
   pre-copies guarded pages into 32 slots (ticket+generation); submit
   consumes; invalidate on remap. + GuardRun VirtualProtect batching
   (bridge <4-block gaps).
7. **0052/0049 OFF-BY-DEFAULT switches**: wave64-pairing (XCL, unvalidated
   on hw), descriptor incremental flags, indirect batch, pipeline standins
   — real code+tests, no in-game A/B → treat as research, not gains.
8. **0063 UMA pool + GPU page directory (~800LOC, opt-in, unmeasured)**:
   placed heaps on CreateFileMapping sections; directory u32[2]/64K page
   (slot,offset) → shaders read game memory without copies; publish under
   submission lock + settle. Long-term architecture, high coupling.
9. Drop-ins: dirty_ranges merge-into-nearest-gap (~90LOC, never lose dirty
   bytes), upload_pool quarter-octave size classes + HeapRanges first-fit
   (~300LOC), standin pipeline donors (~150LOC), frame_wait staged
   telemetry (~600LOC — MESIA has copystat, partial overlap).
10. Already-present-in-MESIA equivalents (do NOT port): ask_once write-
    watch batching (MESIA_ASK_ONCE on since run 81 = their 0045), CARD_
    COMPARE GPU hash (host.rs:1595 — compare theirs is 64-thread/piece
    parallel vs ours), WIDE_* worker pools (their wide flags), placed
    heaps on title memory (their inplace/XCL = our INPLACE), pipeline
    library serialization (both have), keep-until-fence lists (both).
