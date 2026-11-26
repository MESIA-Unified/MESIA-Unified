# KELVAROQ-BENCHMARK-PLAN — protocole de comparaison sans données propriétaires

Objectif : mesurer chaque mécanisme porté de Kelvaroq **sur la charge réelle MESIA**
(RTX 3060 12 Go, 16 Go RAM) avant de le retenir. Aucune donnée du jeu dans le repo ;
les mesures en jeu vivent dans les logs locaux (`%TEMP%\mesia.log`) comme d'habitude.
Règle d'or : **rien n'est intégré sans A/B mesuré** — leurs chiffres RTX 5080 ne sont
pas des attentes pour nous (écart raster ~×2, mémoire ~×4, CPU 9950X3D vs notre hôte).

## 1. Couches de mesure

### 1.1 Micro-benchmarks in-tree (sans titre, reproductibles partout)

Fixtures originales existantes — aucune dépendance au jeu :

| Fixture | Localisation | Ce qu'elle mesure | Usage pour un portage |
|---|---|---|---|
| Bench copie upload | `crates/mesia-gpu` tests `host::batch::*`, `shadow::wide_tests::heavy_frames_of_merges_run_wide_and_merge_the_same` (484 MiB/frame ×6, sortie byte-identical, benchmarks.md) | Copie titre→shadow et merge copy-back | A/B plaincopy 0044 et relay_patch 0057 sans jeu |
| `translated_graphics_baseline` | `crates/mesia-shader` test `--ignored`, 192/192 images traduites acceptées, ~144 présentations/s sur fenêtre 128×64 | Chaîne VS/PS GFX10→DXIL→PSO→Present, P50/P95/P99 | Non-régression rendu pour tout portage touchant draw/present |
| VALU GFX10 | `crates/mesia-shader` `emit::valu_tests` + `compute_correctness` (17/17) | Exactitude shaders traduits | Régression pour ports shaders (0047, 0052) |
| Vertex/pixel fixture | `translated_graphics` (même banc) | Rendu graphique complet comparé CPU | Idem |
| WARP tests | `crates/mesia-gpu` (`MESIA_D3D12_WARP`, host.rs:623-718) | Chemins GPU sur adaptateur logiciel | Validation fonctionnelle quand la RTX est occupée ; **ne valide pas la perf** |
| `bench_write` | `mesia-shader` `emit::bitcode::tests` | Throughput émission DXIL | Référence pour toucher l'émission |

Nouveaux micro-benchmarks à écrire **avant** chaque portage (même pattern que
`heavy_frames_of_merges`) : copie N plages via `copy_mapped` vs chemin porté ; fence
sous lock vs hors lock ; descripteurs K slots dirty vs total.

### 1.2 Charge réelle (vérité terrain, benchmarks.md §Real-title counters)

Protocole A/B calqué sur celui de Kelvaroq (documentation.md:47-60) mais à notre échelle :

- **Même save** (point d'entrée monde identique), **même cache chaud** (copie du dossier
  cache entre les deux bras — jamais le même dossier partagé), même `play.ps1` de base,
  une seule variable `MESIA_*` modifiée ou le portage on/off.
- **300 s par bras** après stabilisation, 3 bras alternés minimum (ordre A-B-A ou
  B-A-B pour diluer la dérive de scène) ; marquer l'entrée monde au log.
- Configuration témoin : `LATE_WAIT=2 INPLACE=1 WINDOW_KB=1024` (config stable
  HANDOFF-ASTRA) ; tout portage se mesure d'abord **sous LATE_WAIT=2**, et en synchrone
  `LATE_WAIT=0` si le mécanisme touche les vols.
- Seuil d'échantillonnage : ≥600 frames dans la fenêtre mesurée (leur protocole exige
  ≥600 images lourdes sur 3 passages — documentation.md:60).

## 2. Métriques par mécanisme porté

Compteurs déjà émis : `copystat.rs` (copies, watch, fallbacks, tex), ligne perf
`mesia.log` (p50/p95 presents), `RELAY_STATS`, `MESIA_GPU_TIMING`, breadcrumbs crash.

| Portage | Métrique primaire | Métriques secondaires | Source du compteur |
|---|---|---|---|
| 0044 plaincopy | ms `preparing: copies` / Mo copié (A/B même tranche de draws) | fallbacks `copy_mapped`, pics p99, `GuardFaults` inchangés | copystat + ligne perf |
| 0046/0048 async present | p50/p95/p99 intervalles Present ; ms passées dans l'appel Present | stabilité : 0 crash ≥ durée LATE_WAIT=3 historique (~18 s→cible >12 min) ; draws droppés | ligne perf + crash dumps |
| 0041 mesh launch | ms GPU des draws mesh (`GPU_TIMING` par commande) ; nombre de UAV waits/clamp passes par frame | taux de pipelines refaits sans étage (`E_INVALIDARG` comptés), draws indirects bornés restants | GPU_TIMING + counters indirect.rs |
| 0055 descriptors | ms de synchro descripteurs par soumission ; slots inspectés vs changés | mémoire du cache TableRecords, époques de release | compteurs à ajouter + copystat |
| 0057 relay_patch | ms `copying back` par frame (78-173 ms actuels) ; Mo réellement recopiés | dwords masqués envoyés, pages over-cap, settles conservés | copystat + STATS relay |
| 0053 watch/ahead | ms de la phase watch+refresh dans la soumission | hit rate des 32 slots, invalidations au remap | compteurs à ajouter |
| dirty_ranges / upload_pool / texel_rows | ms passe textures (7,9 ms baseline V5.3) ; allocations staging/frame | saturité file invalidation (plages perdues = 0) | copystat |
| Waits GPU hors lock (inspiré 0036/0040) | ms d'attente sous `submitted` (~150-200 ms actuels) | débit de listes soumises/s, listes en vol | RELAY_STATS/late_line |
| 0063 UMA (recherche) | Mo copiés/frame (vs ~200 Mo) ; hits directory GPU | RAM/VRAM pic, settles, correctness readback | à instrumenter entièrement |

Métriques globales pour tout portage : ms/frame p50/p95/p99, RAM RSS (cible : rester
≤ ~4,2 Go, budget AGENTS.md 3-4 Go dispo), VRAM locale allouée, wait-sous-lock ms,
copy-back ms, textures ms, draws droppés, **0 crash sur la durée de qualification**
(LATE_WAIT=2 : >12 min minimum ; tout portage touchant flights : ≥30 min sans DEVICE_HUNG).

## 3. Critères d'acceptation par item

| Item | Accepter si | Rejeter si |
|---|---|---|
| Drop-ins render crate | Tests unitaires portés verts ; byte-identical sur fixture merge ; aucune régression p95 in-game | Diff de sortie, perte de dirty bytes à saturation |
| 0044 plaincopy | copies ms −10 % min sur tranche ≥5 000 draws **et** p99 non dégradé ; 0 régression GuardFaults | p95 empire sur les tranches légères (leur propre p95 a empiré 108,8→170,8 ms sur 3-4 k draws, doc:145) |
| Async present borné | p95 présents −20 % min ; **0 crash stage-3 sur 3×10 min** (corréler avec HANDOFF-ASTRA) | DEVICE_HUNG reproduit → le mécanisme n'est pas le fix |
| 0041 mesh launch | −30 % ms GPU sur fenêtre draws mesh ; 0 draw perdu vs témoin | `E_INVALIDARG` non rattrapés, ordre d'amplification casse un effet |
| 0055 descriptors | synchro descripteurs −50 % et slots changés ≪ inspectés | Epoch leak (RAM croissante), descripteur périmè lu par un flight |
| 0057 relay_patch | copy-back −30 % min à contenu vérifié identique | Patch masqué manquant → désync CPU/GPU |
| Wait hors lock | wait-sous-lock −50 %, débit soumissions +; 0 crash | Deadlock/livelock (leur propre risque : host_copy→execute) |

## 4. Méthode et biais à contrôler

- **Même scène, pas même instant** : leurs propres A/B se comparent par tranches de
  draws (documentation.md:134-142) — adopter ce regroupement, nos scènes driftent aussi.
- **Un seul processus titre à la fois**, compilations terminées avant les runs
  (AGENTS.md), `MESIA_GPU_TIMING` absent du timing (il force le drain synchrone).
- **Captures de contrôle** : `scripts/shot.ps1` avant/après dans les deux bras ;
  comparaison visuelle + hachage de crops identiques sur fixture ancrée.
- **Journal** : consigner scène, draws/frame moyens, RVA des arrêts, fidélité — mêmes
  champs que leur protocole (documentation.md:60) pour comparabilité.
- **Publication** : résultats dans `docs/perf-study.md` sous l'entrée du portage, avec
  hypothèse, mesures, limites, rollback (AGENTS.md §Intégrer).

## 5. Ce qu'on ne peut PAS mesurer (à déclarer dans chaque rapport)

- Pas de victory-fork complet cloné → les patches sont évalués par lecture, pas par
  exécution de leur build.
- Pas de contenu propriétaire → aucune comparaison directe MESIA-vs-Kelvaroq sur le
  même titre n'est possible en in-tree ; la seule comparaison chiffrée est
  MESIA-avant vs MESIA-après.
- Leur matériel reste inaccessible : tout chiffre « chez eux » cité est contexte,
  jamais critère.
