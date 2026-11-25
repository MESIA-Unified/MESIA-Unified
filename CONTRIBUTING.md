# Contributing to MESIA Unified

Thank you for your interest in MESIA Unified — an experimental Xbox Series
compatibility engine for Windows.

This repository is the project's **public, documentation-only** presence.
The engine source code is private while it undergoes license review, so the
ways you can contribute today are different from a typical open-source
project — but they are real, and they matter.

## How to contribute now

### Fix or improve the documentation

Typos, broken links, unclear wording, formatting problems, missing context —
all fair game:

1. Fork the repository (or use the GitHub web editor for small fixes).
2. Edit the Markdown files.
3. Open a pull request and complete the PR template.

CI lints all Markdown with `markdownlint-cli2`, so keeping files tidy makes
review faster.

### Report problems

Found an error but not sure how to fix it? Open a **documentation bug
report** using the issue template. Be as specific as you can — page,
section, exact wording.

### Ask technical questions

Use **GitHub Discussions** for questions about the project, its goals, or
the published design notes. If Discussions are not enabled yet, open a
regular issue and frame it as a question.

### Share expertise

A console-to-PC compatibility layer touches deep topics: GFX10/RDNA shader
translation, DirectX 12, GPU memory models under WDDM, compatibility-system
design in Rust. If you have experience in any of these areas:

- Comment on relevant issues and discussions.
- Review published design notes and point out technical problems or better
  approaches.
- Point the project at public research, prior art, or tooling worth knowing
  about.

High-signal technical participation is the fastest way to be noticed when
code contributions open. See "Becoming a collaborator" below.

## Ground rules

- Documentation in this repository is licensed **CC BY 4.0** (see
  `LICENSE`). By contributing, you agree your contribution is distributed
  under that license.
- Every contribution must be your **original work or compatibly licensed**,
  and you must have the right to contribute it.
- **Never** submit proprietary or leaked console material, console SDK
  content, keys, dumps, DRM-circumvention material, or game files. This is a
  hard rule — such contributions are rejected on sight and reported where
  required.
- Follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## When the source opens (planned process)

The engine will be published after the license review concludes. When that
happens, contributions will follow this flow:

1. **Open an issue first** for anything non-trivial so the design can be
   discussed before you invest effort.
2. **Fork and branch.** Use descriptive branch names
   (`fix/shader-barrier`, `feat/memory-heaps`).
3. **Sign off your commits.** The project uses DCO-style sign-off: add a
   `Signed-off-by: Your Name <you@example.com>` trailer to every commit
   (`git commit -s`). This certifies you wrote the change or have the right
   to submit it under the project's license.
4. **Open the PR** and complete the template, including the license and
   provenance attestation.
5. **Review.** Every PR is reviewed by the project owner. Expect direct,
   technical feedback.

### Contributor license consent

When source contributions open, every contributor will be required to accept
the following statement (it already appears in the PR template):

> I confirm that this contribution is my original work or is compatibly
> licensed, that it contains no proprietary or leaked console content, no
> DRM circumvention material, and no game files, and that I have the right
> to submit it under this project's license.

If you cannot make that statement, do not submit the contribution.

## Becoming a collaborator

Collaboration is **by application only**, and every application is reviewed
personally by the project owner:

1. Open a **collaborator application** issue using the
   `collaborator_application` issue template.
2. The owner reviews the application and may follow up with questions in
   the issue.
3. If accepted, you may be invited to private working areas.

**There is no automatic write access — ever.** Write access is granted
manually, per person, and only after review. Visible, high-quality
participation in issues and discussions is the strongest application you
can make.

## License and provenance

- Documentation in this repository: **CC BY 4.0**.
- Engine source: private; licensing terms will be announced when it opens.
- If you are ever unsure whether material you want to contribute is clean,
  ask first — guessing wrong on provenance is the one mistake this project
  cannot accept.
