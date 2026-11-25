# Publishing Workflow — Private Lab → Public Repository

MESIA Unified is developed in a **private lab** and published through a
**docs-only public repository**. The engine source code stays private until
the license strategy is resolved. This document describes how work moves
from the lab to the public repo, and the rules that govern every step. It
is written both as the operating procedure for today and as onboarding for
future collaborators.

## The two repositories

| | Private lab | Public repo (this one) |
|---|---|---|
| Contents | Engine source, vendored dependencies, private reports, fixtures, test harnesses | Documentation: changelog, milestone reports, design summaries, this workflow |
| Access | Project owner and lab agents | Everyone |
| Rule | Nothing leaves it verbatim | Everything in it has passed the publishability audit |

## The pipeline

Publishing is a **manual, audited process**. Nothing is ever auto-published.

```text
1. Develop in the private lab
        │
2. Build + test green (all workspace tests, zero warnings)
        │
3. Publishability audit
   ├─ license check
   ├─ provenance check
   ├─ secrets scan
   └─ binary/file check
        │
4. Write the redacted public summary
        │
5. Open a PR on the public repo
        │
6. Owner review + merge
        │
7. GitHub Release (major milestones only)
```

### 1. Develop in the private lab

All engine work happens in the lab. Lab commits may contain anything the
lab legitimately contains: vendored source, private paths, internal task
IDs, hardware details, scratch notes. None of that is publishable raw.

### 2. Build + test green

A milestone is publishable only when the lab build compiles cleanly and the
full workspace test suite passes with zero warnings. A partially green
result is documented honestly as such — or not published.

### 3. Publishability audit

Before anything is written for the public repo, the material passes four
checks:

- **License check.** Does the content expose or depend on vendored code in
  a way the current license strategy doesn't allow? The lab combines
  components under incompatible-at-first-glance GPL variants; that
  interplay is precisely why the engine is private. Mechanism-level
  descriptions are fine; code, file structure, and internals of vendored
  components are not.
- **Provenance check.** Can every claim be traced to a real lab result?
  Every published number (test counts, timings, pixel results) must come
  from an actual run. If it wasn't measured, it isn't claimed.
- **Secrets scan.** No keys, tokens, credentials, private paths, or
  machine-identifying details the project hasn't chosen to disclose.
- **Binary check.** No binaries, blobs, ROM content, game files, dumps,
  keys, or executable artifacts — ever. This repo is text only.

### 4. Write the redacted public summary

The public artifact (changelog entry, milestone page, design note) is
written fresh from the lab reports — it is a summary, not a copy. Standard
redactions:

- **No commit hashes.** Git history is lab-internal.
- **No private file paths.** Reference public docs paths only.
- **No vendored-component internals or code excerpts.** Mechanism names,
  format names, and design contracts are fine (the public docs already say
  "xcl-style interception", "a `victory-shader` derivative", "descriptor
  guard slot"); implementation internals of third-party code are not.
- **Lab bookkeeping stays lab-side.** Already-public identifiers (e.g.
  the U-16 rule name used across these docs) may be referenced; raw
  tracking details — branch names, worktree layouts, agent assignments —
  are summarized, not transcribed.
- **Honest verification level.** Every result must state that it was
  verified against synthetic fixtures. Where the lab itself flags a limit
  (e.g. "lab record format, not console record-format compatibility"),
  the public text carries that caveat forward — it does not soften it.
- **Test names are publishable when descriptive** — they read like
  behavior specs (`skipped_producer_neutralizes_indirect` documents the
  guard contract better than prose would).

### 5. Open a PR on the public repo

Public documentation lands via pull request, even when written by the
project itself. The PR description links the milestone it summarizes and
confirms the audit checks passed.

### 6. Owner review + merge

The project owner is the only merger to the public repo's main branch. The
owner re-checks the audit items at review time; the writer having checked
them does not substitute for review.

### 7. GitHub Release — major milestones only

A GitHub Release is cut when a milestone is significant enough to warrant a
named snapshot of the public docs (a completed phase or a
capability-defining result). Releases carry the milestone summary and the
synthetic-fixtures disclaimer. Minor doc fixes ride the commit history
without a release.

## Hard rules

1. **Never auto-publish lab commits.** No mirroring, no subtree pushes, no
   CI sync of lab content. The public repo only ever receives
   human-written, audited summaries.
2. **Engine source stays private** until the license strategy resolves.
   Public artifacts describe what the engine does and what was verified —
   never how the code reads.
3. **Contributor code requires provenance attestation.** Any external
   contribution must come with a statement of origin: the contributor
   wrote it, has the right to submit it, and it contains no code copied
   from sources whose licenses we cannot satisfy. Contributions that
   cannot be attested are not merged.
4. **No game files, dumps, keys, or DRM bypass — ever.** Not in the repo,
   not in docs, not in tests, not in examples. Compatibility research uses
   synthetic fixtures only. Do not ask the project for these materials,
   and do not submit them.
5. **No real-title claims.** Stated ambitions stay stated as ambitions.
   Results are always reported with their verification basis (synthetic
   fixtures on the test hardware).
6. **GTA VI phrasing.** The project names GTA VI-class compatibility as a
   long-term ambition. Any public text must keep it clearly aspirational —
   never imply work-in-progress on that or any retail title.
