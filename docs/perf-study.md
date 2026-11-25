# MESIA — Étude d'optimisation mesia-gpu

> [English below](#english-version) · Mesures : RTX 3060 12 Go, 16 Go RAM, runs du 2026-11-25.

## État mesuré (combo env validé)

| Phase | fps | ms/frame | Poste dominant |
|---|---|---|---|
| Écrans légers | 30 | 33 | — |
| Intermédiaire | 15–22 | 45–65 | preparing léger |
| Monde | ~1.4–1.5 | ~670 | copies ~250 + copy-back ~420 + attente GPU ~220 |

Config : `RELAY=2 LATE_WAIT=3 WIDE_BACK=1 PIPELINE_WAIT_MS=24 CARD_BUFFERS_MIN_MB=1 CARD_BUFFERS_MB=3072 CARD_WINDOWS_MB=2048 UNWATCHED_FRAMES=12 INPLACE=1 TEXTURE_NO_WAIT=1 JOIN_COPIES=1 CARD_REHASH_RUNS=1`.

## Résultat du test INPLACE=2 (2026-11-25)

Crash du titre à ~4 min (Breakpad dump, mémoire système à 94 %). Testé avec `LATE_WAIT=0` — seule combinaison autorisée par la doc de `crates/mesia-gpu/src/inplace.rs` (le niveau 2 place les buffers du titre ; avec des vols différés, un buffer placé peut être lu alors qu'une soumission non exécutée doit y écrire). Régression → retour au niveau 1. **Ne pas réessayer sans correction du suivi des écritures en vol.**

## Chemin critique identifié

Dans l'ordre du coût en monde :

1. **Copy-back ~420 ms/frame** — le thread relay rapatrie les buffers shadow modifiés vers la mémoire titre. Mono-thread.
2. **Copies ~200–300 ms/frame** — uploads de buffers dirty vers le GPU (write-watch + hash comparaison).
3. **Attente GPU ~150–280 ms** — le thread de soumission attend la carte ; GPU busy 49–63 % seulement → le goulot est côté CPU, la carte attend le travail.
4. **Refresh textures** — résolu par INPLACE=1 + TEXTURE_NO_WAIT (~1 ms léger, ~26–92 ms monde vs 3–4 s avant).

Chevauchement : copies et copy-back passent par le même relay thread (`MESIA_RELAY=2`) ; l'attente GPU inclut l'attente derrière les copies — les postes 1+2+3 ne sont **pas** indépendants, le chemin critique est séquentiel : watch → compare → copy → submit → wait → copy-back.

## Optimisations proposées (par impact estimé)

### O1 — Copy-back multi-threadé (implémenté, gate `MESIA_WIDE_MERGE=1`)
Le scan des pièces différentes était déjà parallèle (`find_all`/`find_pieces`) ; le **merge** (`Shadow::relay_merge` par pièce de 64 KiB : lecture kernel + merge + écriture kernel dans la mémoire titre) était séquentiel sur le thread relay. Implémentation : `merge_groups` (union-find) regroupe les items dont les plages titre se chevauchent **ou qui partagent un shadow** → chaque groupe garde l'ordre séquentiel dans un job ; les groupes disjoints tournent sur les threads `wide` (2–6). Sémantique identique : même ordre de merges par shadow, écritures titre disjointes entre jobs. Fichiers : `shadow.rs` (`wide_merge`, `relay_merge_all`, `merge_groups`), `host.rs` (`relay_hand_back`). Test unitaire : `merge_groups_keep_only_what_can_meet_together` — 111/111 tests passent. Benchmark : voir « Résultats O1 » ci-dessous.

### O2 — Batching des uploads GPU (impact : moyen-élevé)
`JOIN_COPIES=1` ne fusionne que les copies contiguës. Étendre : grouper les copies < 64 Ko dans un staging buffer unique, une `CopyBufferRegion` par destination. Cible : `crates/mesia-gpu/src/shadow.rs` (`JOIN_COPIES` path). Test : compteur `title buffer copies recorded per frame` + total MB stable.

### O3 — Compare GPU des shadow buffers (impact : moyen)
`CARD_BUFFERS_MB` permet déjà des comparaisons côté carte ; étendre `CARD_REHASH_RUNS` pour pipeliner : comparer frame N-1 pendant que N se soumet. Cible : `crates/mesia-gpu/src/card_buffers.rs` (si présent) / shadow compare path.

### O4 — Slots de vue > 1M (impact : moyen, visuel)
Heap D3D12 plafonné à 1M vues (limite NVIDIA) ; ~245k vues lisent un slot nul → textures absentes en monde. Options : heap secondaire + copies, ou recyclage LRU de slots inactifs. Cible : `crates/mesia-gpu/src/texture.rs` (view heap).

### O5 — Préchauffage pipelines (impact : visuel)
Les draws droppés faute de pipeline prêt = écrans buggés. La bibliothèque (92 Mo, auto-save 20 s) s'enrichit seule ; un mode « précompile » qui charge le bin et compile les shaders manquants au lancement réduirait les drops du début. Cible : `crates/mesia-gpu/src/pipeline/library.rs`.

### O6 — Pipelines compute refusés 0x80070057 (impact : faible, correctif)
2–3 pipelines compute rejetés par le driver (DXIL E_INVALIDARG). Identifier le shader fautif via log `host pipeline refused`, tester `MESIA_SHADER_DUMP` + recompilation isolée.

## Protocole de validation par optimisation

- Mesurer indépendamment : activer **une** optimisation à la fois, run 250 s même scène (chargement → monde), comparer les compteurs `fps / preparing / on the GPU / copying back`.
- Non-régression : zéro crash, zéro panic, `let go because their memory changed` stable, pas de hausse des `dropped … without a host pipeline`, rendu visuel intact (pas de scintillement).
- Documenter chaque mesure ici (tableau « État mesuré »).

---

## English version

See table above for measured state. Key findings:

- **INPLACE=2 test failed** (title crash ~4 min in, 94 % system memory) even with the only doc-sanctioned config `LATE_WAIT=0`. Reverted to level 1.
- **Critical path** in world scenes: sequential relay work — copy-back ~420 ms + copies ~250 ms + GPU wait ~220 ms ≈ 670 ms/frame. GPU only 49–63 % busy → CPU-side host work is the bottleneck.
- **Proposed work**, highest impact first: O1 multithreaded copy-back (`shadow.rs`/relay), O2 batch small uploads into one staging buffer, O3 pipeline the card-side compares, O4 fix >1M view-heap slots (null textures), O5 pipeline prewarm, O6 chase the 0x80070057 refused compute pipelines.
- Each item ships alone, measured on the same 250 s run, with zero-crash / zero-regression gates.

---

## Résultats O1 — benchmark synthétique (`a9155a0`+, mesuré 2026-11-25)

`heavy_frames_of_merges_run_wide_and_merge_the_same` (ignoré, benchmark) —
`cargo test -p mesia-gpu --release -- --ignored --nocapture heavy`.
Machine : Windows 11, RTX 3060, 16 Go RAM.

Charge : 6 frames × 72 copies (8 « windows » dans la plage d'une autre
copie), 7 752 pièces, **484 MiB fetched/frame**, 64 groupes — modèle
fidèle de `card_back` (mêmes fonctions de découpe/groupage/merge).

| | Total | /frame |
|---|---|---|
| Pièce par pièce | 2,605 s | ~434 ms |
| Threads `wide` | 1,542 s | ~257 ms |

**Speedup 1,7×** — et **sortie octet-par-octet identique** (mémoire titre,
sides CPU, compteurs d'écriture par range : assertions du test).
Note : le modèle couvre le merge CPU ; le chemin réel ajoute les appels
kernel (`NtWriteVirtualMemory`) dont la latence se parallélise aussi — le
gain en jeu reste à mesurer. ~434 ms/frame séquentiel recoupe le ~420 ms
de copy-back mesuré en monde (run 92).

Statut : flag `MESIA_WIDE_MERGE` toujours **off** par défaut — bascule
par défaut conditionnée à une run réelle A/B sans crash.
