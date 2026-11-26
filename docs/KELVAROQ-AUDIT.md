# KELVAROQ-AUDIT — analyse architecturale du dépôt Kelvaroq

Audit documentaire, novembre 2026. Dépôt audité : `github.com/stellarr-studio/kelvaroq`
@ `1270c422` (v0.2.0), clone local en lecture seule `C:\Users\utilisateur\Desktop\kelvaroq-src`.
Aucun code exécuté, aucun binaire téléchargé, aucun test lancé. Base factuelle :
`docs/kelvaroq-findings.md` (synthèse de deux audits par sous-agents), vérifiée et corrigée
ci-dessous sur les points que le clone permet de trancher.

## Ce qu'est réellement le dépôt

**Le moteur n'est pas dans le repo.** Kelvaroq publie trois choses :

1. **Un launcher/CLI Rust** (`src/`, ~6,6 kLOC réelles : 5 058 lignes `.rs` + 1 576 lignes
   `.cs`, 35 fichiers) — et non « ~50 kLOC » comme l'indiquait la synthèse brute. Le
   lanceur couvre : sélection du dossier de jeu via `MicrosoftGame.config`, workspace
   desktop WPF/.NET 4.8 (`src/launcher/*.cs`, locales 6 langues), snapshots de sauvegardes
   ZIP avec manifeste SHA-256 (`src/launcher/Archive.cs`), profils matériels automatiques
   (`src/hardware.rs`, `src/adapters.rs`), préchargement de shaders (`src/preload.rs`),
   moniteur de performance live (`src/monitor/`), mise à jour 24 h
   (`src/launcher/Updates.cs`).
2. **Une crate de rendu** `native/kelvaroq-render` (~1,5 kLOC, 10 modules) : logique de
   rendu testable hors GPU — `dirty_ranges` (fusion de plages bornée), `heap_ranges`
   (first-fit), `upload_pool` (classes de taille), `residency` (requêtes groupées 256
   blocs), `texel_rows` (plans de copie par mip), `compare_slots`, `shadow_bytes`,
   `pacing` (percentiles sur intervalles Present), `stats_ring` (protocole monitor v1).
3. **61 patches** `native/patches/00NN-*.patch` (~34 k lignes de diff total), numérotés
   0000–0064 avec quatre numéros absents (0003, 0007, 0008, 0011) — la synthèse disait
   « 65 patches », le décompte réel est **61 fichiers**. Ils sont appliqués au build par
   `scripts/build_native.py` sur `github.com/fxceless/victory-fork` épinglé au SHA
   `f85101b6095e8cf3cecf8f0647cc7f5428daa177` (documentation.md:5). Le reçu `build.json`
   identifie l'arbre compilé, chaque patch et le binaire.

Plus `research/` (référence CPU Unicorn + outils de mesure, ~7 kLOC Python/Rust,
licence propre GPL-2.0-or-later, `research/LICENSE`) et `scripts/` (build, packaging,
vérification shaders).

**Ligne de parenté avec MESIA.** victory-fork est la même lignée que les crates de
MESIA reflètent (loader/hle/gpu/shader/formats/pipeline — noms de fichiers quasi
identiques : `host.rs`, `descriptor.rs`, `watch.rs`, `shadow.rs`, `copystat.rs` existe
aussi chez eux, cf. `0064-monitor.patch`). Les patches ciblent `crates/victory-gpu/src/*`
→ application conceptuelle quasi directe sur `crates/mesia-gpu/src/*`, mais **les diffs
ne s'appliqueront pas textuellement** : bases de code divergentes, noms de structures
et d'env vars différents (`VICTORY_*` vs `MESIA_*`). Chaque portage est une réécriture
guidée, pas un `git apply`.

**Provenance du code.** Tout est produit par IA (« Co-authored-by: Moon AI »,
README.md:16, :266). Le dépôt a ~3 jours au moment de l'audit (créé ~24 nov 2026),
104★/27 forks — risque « fork-bait » : popularité instantanée sans historique de
maintenance. Les docs d'ingénierie sont inhabituellement candides : régressions
publiées (README.md:96, le p95 empire de 262 à 568 ms sur une tranche), et aveu explicite
« cette exécution en jeu ne démontre ni un gain global en FPS ni la disparition des
objets manquants » (documentation.md:296).

## Part réelle vs annoncée vs stub, par domaine

| Domaine | Annoncé | État réel vérifié | Source |
|---|---|---|---|
| Perf prologue | ~10 fps, 7-8 fps lourd | Réel, mesuré A/B 300 s sur RTX 5080/9950X3D ; p50 198→120→108 ms | README.md:74-99 |
| Playtest externe | 36 min sans crash | Réel, une session RTX 4090/7950X, 68 échantillons | README.md:103-122 |
| Mémoire unifiée | « 100 % partagée, 100 Mo→8 Mo, 18 s→60 ms » | **Opt-in non livré** (`VICTORY_UMA*`, patch 0063 marqué en test ; test WARP `#[ignore]`, 0063:526). Pas dans le build par défaut | README.md:38, 0063-uma.patch:624 |
| « #1 de 44 trackers » | marketing | Les trackers cités montrent ~#5-6 (findings) ; la doc cite PortWatch elle-même comme source externe | documentation.md:226 |
| Copies in-process | 7-12 % frame, pics >1 s disparus | Réel et mesuré par tranches de draws ; `plaincopy.rs` 422 LOC dans 0044 | documentation.md:128-147 |
| Async present borné | p95 568→262 ms | Réel, `FrameLimit` mutex + `Held::Present`, `FRAMES_AHEAD=2` | 0046-async-present-frame-waits.patch |
| Descripteurs incrémentaux | 200 k→9 k slots/frame | Code réel dans 0055 (record_key 12 o→u64, TableRecords) ; gain in-game non isolé publié | 0055-descriptors.patch |
| Wave64-pairing | traduit | Code + tests, `VICTORY_WAVE64_PAIRED` défaut 0, **jamais validé sur GPU physique** | README.md:195, 0052-wave64.patch |
| Ray tracing | structures AS | Volontairement coupé : ~515 draws droppés/frame | documentation.md:161 |
| CI | fmt/clippy/unit | **Zéro validation GPU en CI**, aucun log brut publié | findings ; CONTRIBUTING.md |
| Launcher complet | workspace+snapshots+monitor | Réel, fonctionnellement dense pour ~6,6 kLOC | src/ |

## Ce qui manque ou est trompeur

- **Pas de moteur cloné** : la relecture des patches exige de fetcher victory-fork
  @ f85101b séparément. L'audit n'a pas vérifié l'application réelle des 61 patches ni
  compilé quoi que ce soit — chaque affirmation « ~N LOC » vient des headers de patch.
- **Matériel unique** : toutes les mesures jeu = RTX 5080 + 9950X3D + 64 Go
  (documentation.md:272-279). L'écart RTX 5080→RTX 3060 (×~2 de raster, ×~4 mémoire)
  rend **aucun** chiffre extrapolable. Voir KELVAROQ-BENCHMARK-PLAN.md.
- **Requêtes propriétaires** : aucun XEX, clé, SDK ou asset de jeu dans le repo
  (vérifié par grep + disclaimer README.md:285). Légal à lire et réutiliser sous GPL.
- **Documentation en français, code en anglais** ; la doc mélange mesures réelles et
  pistes non mesurées — chaque chiffre cité ici est rattaché à sa ligne source.

## Mécanismes porteurs (détails dans la matrice et la roadmap)

Le cœur exportable est dans les patches, pas dans le repo : 0044 (copies in-process,
ReadProcessMemory → rep-movsb+VEH), 0041 (lancement de mesh draws par étage
d'amplification GPU), 0046/0048 (present asynchrone borné), 0055 (descripteurs
incrémentaux), 0057 (patch copy-back GPU masqué), 0053 (refresh spéculatif
watch/ahead), 0063 (UMA, recherche). Plus 9 drop-ins de la crate render.

## Sources consultées et leurs limites

| Source | Portée | Limite |
|---|---|---|
| `kelvaroq-src` clone local | Tout le repo public @ 1270c422 | Lecture seule, aucune exécution |
| `kelvaroq-findings.md` | Synthèse de 2 audits sous-agents | Quelques chiffres inexacts corrigés ici (LOC, nb patches) |
| victory-fork @ f85101b | **Non cloné** | Les patches sont lus hors contexte ; l'application réelle non vérifiée |
| README/documentation/RELEASE_NOTES | Affirmations de l'éditeur | Auto-déclaré ; mesures non reproductibles sans jeu |
| AUTONOMOUS-NOTES.md, HANDOFF-ASTRA.md, perf-study.md | État MESIA | — |

Incertitudes marquées : toute ligne « ~LOC » = taille de patch, pas code relu ; tout
gain « ms » = leur matériel ; tout item « opt-in » = non mesuré en jeu même chez eux.
