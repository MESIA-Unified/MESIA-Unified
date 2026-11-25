# Inventaire des tests — baseline `a06cfaa`

## Compteurs

| Périmètre | Annoncé | Mesuré |
|---|---|---|
| `cargo test -p mesia-gpu` (session O1) | 111 | 111 pass + 1 ignoré |
| Workspace complet (`--workspace`) | « 207 tests » (historique xcl) | **501 fonctions, 495 exécutées, 5 ignorées, 489 pass** |

| Crate | Listés | Ignorés |
|---|---|---|
| mesia (app) | 1 | 0 |
| mesia-core | 11 | 0 |
| mesia-formats | 28 | 4 (bcpack/xma throughput, longs) |
| mesia-game | 2 | 0 |
| mesia-gpu | 112 | 1 (benchmark `texture_checks_per_submission`) |
| mesia-hle | 95 | 0 |
| mesia-loader | 24 | 0 |
| mesia-pe | 7 | 0 |
| mesia-pipeline | 4 | 0 |
| mesia-shader | 212 | 2 |
| xtask | 4 | 0 |

L'écart 207↔501 : le « 207 » datait de l'ancien projet xcl (suite plus petite) ; le « 111 » n'était que la crate mesia-gpu. Aucun test n'a disparu.

## Résultat baseline (run complet)

- **489/490 exécutés passent.** 1 échec : `emit::valu_tests::ds_min_max_f32_order_floats_and_return_the_old_value` (mesia-shader, test GPU réel).
- **Re-run en solo : PASSE (77 s)** → échec flaky sous contention GPU pendant la suite, pas un bug fonctionnel. À retenir : tests GPU sensibles à la parallélisation de cargo.
- Warnings build : aucun nouveau (`cargo build --release` propre).

## Point de restauration

Tag git `baseline-a06cfaa` poussé sur `MESIA-Unified/MESIA-Unified` (= commit O1). Le dossier `mesia-main` n'est pas versionné ; `mesia-upload` (clone) est la source git.

## Agents déployés

- B (memory coherency) : ✅ terminé — voir `memory-coherency.md`.
- C (D3D12 validation) : en cours.
- D (O1 concurrency) : en cours.
- A (graphics integrity), E (profiling), F (QA) : contrats définis dans `graphics-integrity.md` (à faire valider par benchmark synthétique — infra absente du repo).
