# KELVAROQ-COMPATIBILITY-MATRIX — comparaison par domaine

Kelvaroq @ `1270c422` (victory-fork @ `f85101b` + 61 patches) vs MESIA @ ~`3a46661`.
Classes : **déjà présent** (MESIA a l'équivalent, ne pas porter) · **réutilisable**
(drop-in ou quasi) · **à adapter** (même concept, réécriture sur nos structures) ·
**à réimplémenter** (concept à reprendre, code non transférable) · **non public**
(leur build ne l'inclut pas / non mesuré) · **incompatible** · **provenance à vérifier**
(marqueur « Approach after XCL » → XCL est GPL-2.0-only, diff avant copie).

| Domaine | Kelvaroq a | MESIA a | Différence | Classif |
|---|---|---|---|---|
| **Descripteurs** | Traduction 1× à l'écriture : write-watch du heap invité + fingerprint `record_key` 12 o→u64, cache `TableRecords` + époques de release (0055) ; « <1 ms/soumission » revendiqué | Synchro par soumission via `descriptor.rs` + `descriptor/host.rs` ; caches `Shown` epoch existants (`a201f1a`) | Leur delta = ne traiter que les slots changés (200 k inspectés→~9 k) ; chez nous le coût n'est pas isolé en compteur | à adapter (0055, risque B — état partagé) |
| **Mémoire : copies titre→hôte** | `plaincopy.rs` : rep-movsb in-process + VEH (fautes **lecture** seules, bloc 64 Kio blacklisté 2 s, <256 o→noyau) ; écritures jamais in-process (0044) | `copy_mapped` = `ReadProcessMemory` sur le processus courant, ~200 Mo/frame, ~93,7 ms en V5.3 (host.rs:5894-6022) | Même bouleversement ; leur VEH ne couvre que les lectures, nos fautes d'écriture gardent les gardes de pages | à adapter (0044, 422 LOC, quasi-standalone) |
| **Mémoire : write-watch** | Une requête `GetWriteWatch` par réservation et par soumission, réponse réutilisée ; pages hors plage journalées (0045) | `MESIA_ASK_ONCE=1` actif par défaut (watch.rs:145-165) ; `Step::Texture` couvert depuis `3a46661` | Équivalent fonctionnel ; leur découpage « par réservation » peut différer de notre « par soumission » | déjà présent (0045 ≈ ASK_ONCE) |
| **Mémoire : refresh spéculatif** | `watch/ahead.rs` : worker pré-copie les pages gardées dans 32 slots ticket+génération, invalidé au remap ; `GuardRun` batche `VirtualProtect` (0053) | Pas d'équivalent ; notre guard/copies est synchrone à la soumission | Pré-calcul hors chemin critique ; chevauchement avec notre `WIDE_REFRESH` à vérifier | à adapter (0053) |
| **Soumission** | Fil `victory-submit` + `VICTORY_ASYNC_SUBMIT` relâche le verrou file invité pendant l'attente GPU ; échec→synchrone conservé | `LATE_WAIT` flights + thread de soumission ; waits GPU **sous** lock `submitted` ~150-200 ms (rapport B2, plan « wait hors lock » rédigé non implémenté) | Concept identique (attendre sans tenir le verrou) ; leur implémentation = référence de design pour notre plan B2 | à adapter (0036/0040 + notre plan B2) |
| **Présentation** | Async borné : `FrameLimit` mutex+condvar, `Held::Present` en queue, `VICTORY_FRAMES_AHEAD=2` clamp 1-8 ; p95 568→262 ms (0046+0048) | `LATE_WAIT=2` stable >12 min (139 listes, P50=405 ms) ; **`LATE_WAIT=3` = present-flight qui crashe** (HANDOFF-ASTRA.md:10) | Leur file bornée par condvar vs notre slot `p.ahead` `flown<=FINISHED` — design plus simple à auditer | à adapter — **lien direct avec notre crash stage-3** |
| **Transferts / copy engine** | Deux files copie fences croisées `VICTORY_QUEUE2=3`, landing pools conservés (0035) ; ReBAR copies `VICTORY_REBAR_COPIES_MB` (0039) | `MESIA_RELAY` thread copie + `CARD_BUFFERS_*`, copy engine non séparé documenté | Leur dual-queue est une variante de notre relay ; ReBAR placement = `CARD_WINDOWS` proche | à adapter (drop-in partiel) |
| **Textures** | `dirty_ranges` merge-into-nearest-gap à saturation (jamais perdre un dirty byte) ; `texel_rows` plan de copie par mip réutilisé ; `residency` requêtes groupées 256 blocs ; `TEXTURE_NO_WAIT` | `texture.rs` stage refresh, `TEXTURE_NO_WAIT` équivalent (`3a46661`), textures 7,9 ms/frame post-fix ; queue d'invalidation à vérifier pour saturation | Leur fix « fusionner vers la voisine la plus proche » traite un débordement que notre file peut avoir aussi | réutilisable (dirty_ranges 132 LOC, texel_rows 107 LOC) |
| **Shaders** | GFX10→DXIL du fork ; correctif ordre signature DXIL + 2 cas swap de masque (0047) ; wave64-pairing non validé (0052) | `mesia-shader` ir/structure/resource + emit DXIL ; wave64→32 en mode 2 avec suspects TDR identifiés (lower.rs) | Lignée identique ; leurs fixes DXIL peuvent manquer chez nous (18 entrées de cache validées chez eux) | à vérifier par diff (0047 petit ; 0052 recherche) |
| **Pipelines** | Standins : pipeline provisoire + vrai en arrière-plan (0050, XCL-flaggé) ; warm-list persistante sans library (0026/0036) ; serialisation library | `pipeline/` make+host+library, stand-in au premier bind déjà fait (leçon xcl), tempête résolue `fbf8577` | Équivalents partiels des deux côtés ; leur standin « ne dessine rien » vs notre queue-at-bind | déjà présent en partie / à adapter (standins) |
| **Indirect / mesh** | Étage d'amplification `launch.hlsl` devant chaque mesh traduit : lit l'arg record sur GPU, borne 65 535, budget atomique, `DispatchMesh` ; supprime passe de clamp + 2 UAV waits (0041) ; payload 4 o obligatoire | `indirect.rs` = passe compute de clamp budget (ce que 0041 élimine !) + pipelines mesh existants | Même diagnostic (clamp passe + waits) ; besoins : support AS shader, 4 o payload dans nos mesh traduits, budget buffer | à adapter (0041, ~300 LOC + HLSL) |
| **Concurrence / waits GPU** | `FrameLimit` borné ; wait counters publiés | Waits sous `submitted` ~150-200 ms ; `LATE_WAIT=3` crashe | — | couvert par les lignes Soumission/Présentation |
| **RAM/VRAM** | Pools staging classes de taille (`upload_pool`, quarter-octave) ; `CARD_BUFFERS`/`QUEUE2_LANDING` sizés par profil matériel auto (README.md:189-195) ; `kept`/budgets | Pools `CARD_*` + `MESIA_WINDOW_KB=1024` ; RSS 3,5-4,2 Go stable (V5.3) ; gaspillage grille 4 Mio audité | Leur auto-profiling matériel (VRAM≥10/RAM≥16→budgets) est launcher-side ; nos budgets sont env-only | réutilisable (upload_pool 143 LOC) / non prioritaire |
| **Telemetry** | `copystat.rs` (même nom, patch 0064), `stats_ring` protocole monitor v1, `pacing` p50/p95/p99 par 30 intervalles, `frame_wait` staged | `copystat.rs` (MESIA), compteurs frame-line, GPU_TIMING, breadcrumbs crash | Lignée commune évidente ; leur ring protocol + monitor live est plus complet côté UI | déjà présent (chevauchement partiel) |
| **Launcher** | Workspace WPF, snapshots ZIP SHA-256+rétention, profils matériels auto, update check, i18n 6 langues, monitor live | `apps/mesia` CLI, `play.ps1`, `mesia translate/census`, pas de GUI | Domaine orthogonal ; réutilisable si un jour GUI — faible priorité perf | réutilisable (hors scope perf) |
| **UMA / mémoire unifiée** | Pool UMA opt-in + directory page u32[2]/64 Kio, heaps placés sur sections `CreateFileMapping`, publication sous lock + settle ; **non livré, non mesuré** (0063) | `inplace.rs` : `OpenExistingHeapFromAddress(1)` probé, INPLACE=1 actif, INPLACE=2 interdit sous flights | Même idée (mémoire console unique) ; leur réalisation = directory dans shaders, notre `inplace` = heaps ouverts | non public / recherche (0063, ~800 LOC) |

## Sites « Approach after XCL » — provenance à vérifier avant copie

XCL (`github.com/xcl-dev/xcl`) est **GPL-2.0-only** : leurs fichiers marqués sont
probablement du clean-room (« approach after » = réimplémentation d'une idée, pas copie
de code), mais chaque site doit être diffusé contre le commit XCL cité avant toute
reprise littérale dans MESIA :

| Fichier Kelvaroq | Ligne | Marqueur |
|---|---|---|
| `src/adapters.rs` | 10 | XCL 2dd04da — ordre DXGI high-performance, skip software |
| `src/hardware.rs` | 31 | XCL 8ba374c — query shader support + wave widths |
| `src/native.rs` | 27 | XCL b49be18 — résoudre l'exe moteur relatif au launcher |
| `src/launcher/Storage.cs` | 95-96 | XCL 50a9fde + 817328d — manifeste exe imbriqué, XML decode |
| `src/launcher/Archive.cs` | 186 | XCL 2d94699 — publish après vérification complète |

Les patches eux-mêmes référencent aussi XCL : `0049-xcl-fixes.patch` entier, plus des
mentions dans 0050/0052/0053/0055/0057/0063 — à marquer lors du portage. Les items
launcher (ci-dessus) sont hors scope perf ; le vrai risque de provenance est dans les
patches flaggés XCL, pas dans ces cinq lignes.

## Synthèse des classes

- **déjà présent** : write-watch batching (ASK_ONCE), hash GPU CARD_COMPARE (host.rs:1595
  vs leur `CARD_HASH_WIDE` 64-fils — le leur est parallèle par bloc de 64 Kio, le nôtre
  séquentiel par pièce → *différence de degré, pas de présence*), WIDE_* pools, INPLACE,
  pipeline library, keep-until-fence, stand-in-at-bind, textures no-wait.
- **réutilisable** : `dirty_ranges.rs`, `texel_rows.rs`, `upload_pool.rs`, `residency.rs`,
  `compare_slots.rs`, `stats_ring.rs` (crate render = Rust pur, GPL-3.0, ~1 kLOC total).
- **à adapter** : 0044 plaincopy, 0046/0048 async present, 0041 mesh launch,
  0055 descriptors, 0057 relay_patch, 0053 watch/ahead, async submit waits.
- **non public / recherche** : 0063 UMA, 0052 wave64, 0049 xcl-fixes, standins 0050.
- **provenance à vérifier** : 5 sites launcher + patches XCL-flaggés (GPL-2.0-only).
