# KELVAROQ-INTEGRATION-ROADMAP — opportunités classées et phasage

Classement des mécanismes Kelvaroq par pertinence pour les douleurs MESIA mesurées
(~200 Mo/frame de copies `ReadProcessMemory` dans `copy_mapped` host.rs:5894 ;
waits GPU sous lock `submitted` ~150-200 ms ; copy-back ~109-173 ms ; textures
7,9 ms post-fix ; `indirect.rs` = passe de clamp que leur 0041 élimine).
Coûts : S <300 LOC / M 300-800 / L >800. Risque régression : A faible / B moyen
(touche vols ou état partagé) / C élevé (chemin critique ou non validé ailleurs).
« Gain potentiel » = extrapolation pour notre charge, **hypothèse** ; « mesuré chez
eux » = leurs chiffres RTX 5080/9950X3D, **non transférables** (règle : rien
n'est intégré sans A/B sur charge réelle — KELVAROQ-BENCHMARK-PLAN.md).

## Top 10

| # | Mécanisme | Source Kelvaroq | Compat. MESIA | Licence | Coût | Risque | Mesuré chez eux | Gain potentiel MESIA | Tests requis | Prio |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **Copies in-process `plaincopy`** : rep-movsb + VEH (fautes lecture seules), bloc 64 Kio blacklisté 2 s, <256 o→noyau ; écritures gardent le noyau | `0044-copy-plain.patch` (`plaincopy.rs` 422 LOC, quasi-standalone) | Directe : remplace `ReadProcessMemory` dans `copy_mapped` ; `MESIA_COPY_MODE=plain` existe déjà pour timing | GPL-3 OK | S-M (~450 LOC portés) | B (VEH process-wide ; écritures doivent rester dans les page guards) | −7-12 % frame ≥5 k draws, pics >1 s éliminés ; −0,34→0,28 ms/Mo | Élevé : nos copies = poste dominant (~94-360 ms/frame selon run) ; effet réel limité si le coût est dans l'entourage des lectures (~2,5 µs/lu chez eux) | A/B copies ms + fallbacks ; fixture merge byte-identical ; GuardFaults inchangés | **P1** |
| 2 | **Present async borné** : `FrameLimit` mutex+condvar, `Held::Present` en file, `FRAMES_AHEAD=2` (clamp 1-8) | `0046-async-present-frame-waits.patch` + `0048` (28 LOC) | Proche : notre `p.ahead`/`flown<=FINISHED` est un cas particulier moins borné ; **lien direct avec le crash stage-3** (HANDOFF-ASTRA : `late_free_present`/`execute_own`/sweep) | GPL-3 OK | S-M (~150 LOC) | B (touche la classe de bug exacte du crash stage-3 — peut le fixer OU le déplacer) | p95 568→262 ms ; p50 115,5→108,5 ; titre ne passe plus que 0,1 ms en Present | Double : perf modeste + **un design alternatif auditable pour stage-3** ; peut remplacer `late_free_present` par une file bornée sans sweep | 3×10 min stage-3 sans DEVICE_HUNG ; p95 presents ; 0 draw perdu | **P2** |
| 3 | **Descripteurs incrémentaux** : write-watch du heap invité, `record_key` 12 o→u64, cache `TableRecords` + époques, dedup spans | `0055-descriptors.patch` | Bonne : nos `descriptor/host.rs` ont la même forme (lignée) ; caches epoch `Shown` déjà en place | GPL-3 OK | M-L (~700 LOC) | B (état partagé + vols : un slot libéré trop tôt = descripteur déchiré, la famille de bugs V5.2) | 200 k inspectés→~9 k changés/frame ; « <1 ms/soumission » | Moyen-élevé : notre coût descripteurs n'est pas isolé ; d'abord instrumenter, puis décider | Compteur slots inspectés/changés ; epoch leak RAM ; crash sous flights | **P2** |
| 4 | **Copy-back GPU masqué `relay_patch`** : compute shader Load→mask→Store applique les edits CPU dword en carte, sans recopie (cap 128 pages) | `0057-relay.patch` (`host/relay_patch.rs` ~540 LOC) | Bonne : `shadow.rs`/`copystat`/`CARD_COMPARE` existent ; notre copy-back = 78-173 ms/frame | GPL-3 OK | M (~550 LOC) | B (patch raté = désync CPU/GPU silencieuse ; ordre des écritures) | ~15 ms/frame évités | Élevé si copy-back reste ~100+ ms ; chez eux c'était ~15 ms/frame, chez nous jusqu'à 173 ms → gain possible plus grand | Correctness : bytes GPU→CPU identiques ; A/B copy-back ms | **P3** |
| 5 | **Mesh draws lancés par la carte** : étage d'amplification `launch.hlsl` lit l'arg record sur GPU, borne 65 535, budget atomique, `DispatchMesh` ; payload 4 o requis | `0041-mesh-launch-wide-hash.patch` (~300 LOC + HLSL) | À adapter : `indirect.rs` fait déjà le clamp par passe compute (ce que 0041 supprime) ; pipelines mesh existent ; besoin : AS support dans les shaders traduits + 4 o payload + budget buffer | GPL-3 OK | M (~350 LOC) | B-C (ordre des groupes d'amplification non garanti ; `E_INVALIDARG` à rattraper ; RTX 3060 = Ampere, mesh/AS OK en principe — à vérifier au runtime) | p50 198→120 ms avec 0042 (waits 3 200/3 950→~0) ; 14 µs→~4 µs par draw | Moyen : nos draws indirects sont déjà clampés GPU-side ; gain = supprimer 2 UAV waits/draw ; dépend de notre taux de draws mesh indirects (non mesuré) | GPU_TIMING par draw ; taux E_INVALIDARG ; A/B même scène ; comptage draws perdus | **P3** |
| 6 | **Refresh spéculatif `watch/ahead`** : worker pré-copie les pages gardées en 32 slots ticket+génération ; `GuardRun` batche `VirtualProtect` | `0053-submit.patch` (~360 LOC) | À adapter : notre watch/guard est synchrone à la soumission ; recouvre partiellement `WIDE_REFRESH` | GPL-3 OK | M (~400 LOC) | B (invalidation au remap critique : un slot périmé = données fausses) | Non isolé publiquement | Moyen : réduit la phase watch+copie du chemin critique ; chevauchement avec WIDE_* à évaluer | Hit-rate slots ; invalidations ; A/B phase watch | P3 |
| 7 | **Drop-ins render crate** : `dirty_ranges` (fusion vers voisine à saturation — jamais perdre un dirty byte, 132 LOC) ; `upload_pool` classes quarter-octave + `heap_ranges` first-fit (~300 LOC) ; `texel_rows` plan par mip (107 LOC) ; `residency` batch 256 (186 LOC) | `native/kelvaroq-render/src/*.rs` | Excellente : Rust pur, sans GPU, testable hors ligne | GPL-3 OK | S (~150-300 LOC chacun) | A (logique pure, tests portables avec le code) | Textures : 104 µs→6,6 µs residency, plan mip conservé | Faible-moyen : nos textures sont déjà à 7,9 ms post-fix ; le vrai intérêt = robustesse saturation file invalidation | Tests unitaires portés ; fixture saturation 32 768 plages | **P1** |
| 8 | **Waits GPU hors lock / async submit** : le fil soumission relâche le verrou de la file invité pendant l'attente GPU | `0036`/`0040` (+ doc « décisions ») | À adapter : notre plan B2 est déjà écrit (audit waits, non implémenté) ; leur code = référence de design | GPL-3 OK | M | B (risque deadlock : leur rejet « host_copy→execute » = notre `host.execute` sous lock — même piège documenté) | Soumission ~86 ms travail, wait 15,2 ms post-chantier | Élevé : 150-200 ms de wait sous `submitted` chez nous | Deadlock watchdog ; débit soumissions ; A/B wait-sous-lock ms | P2 (notre chantier, eux comme référence) |
| 9 | **Hash GPU 64-fils** : `CARD_HASH_WIDE` lit 64 Kio en 64 threads côte à côte (1 Kio/fil) puis combine | `0041` (partie wide) | Déjà présent en concept : `CARD_COMPARE` (host.rs:1595) fait le même travail mais séquentiel par pièce | GPL-3 OK | S | A-B | ~390 compares/frame, passage à 140-156 ms en warmup seul | Faible-moyen : 380 hash passes + 89 compare passes/frame chez nous ; paralléliser les pièces | Byte-identical hash output ; A/B temps dispatches | P3 |
| 10 | **Standins / pipeline provisoire** : pipeline vide immédiat + vrai en arrière-plan (XCL-flaggé) | `0050-standins.patch` | Partiel : notre queue-at-bind existe (leçon xcl déjà appliquée `fbf8577`) ; leur version « ne dessine rien » évite le drop | **provenance à vérifier** (XCL GPL-2-only) | S-M | B (draw droppé vs draw silencieusement vide : compromis fidélité) | Non mesuré en jeu | Faible (tempêtes déjà résolues chez nous) | Diff XCL obligatoire avant tout code | P4 |

## Phasage

**Phase 1 — drop-ins + plaincopy** (S/M, risque A-B) : render crate (`dirty_ranges`,
`texel_rows`, `upload_pool`, `residency` — ~700 LOC Rust pur avec leurs tests portés)
+ `plaincopy` 0044 derrière `MESIA_*` env-gate, fallback `ReadProcessMemory` au premier
doute. Livrable : benchmark copies ms sur fixture merge, puis A/B 3×300 s.
**Statut V5.5** : `plaincopy` porté et gate `MESIA_COPY_PLAIN` (défaut OFF, 4 tests
verts) ; drop-ins différés (doublons `dirty`/`rows` existants). A/B encore à refaire —
voir KELVAROQ-PHASE1-RESULTS.md (le seul run activé a été interrompu par le crash
gating, non imputable à plaincopy).

**Phase 2 — async present borné + descripteurs incrémentaux** (M, risque B) :
`FrameLimit`/`Held::Present` 0046/0048 — **à traiter en lien avec HANDOFF-ASTRA** :
notre stage-3 crash = leur domaine d'intervention (present-flight en file bornée vs
notre slot+`execute_own`+sweep). Le portage peut doubler comme redesign du crashur ;
si le DEVICE_HUNG persiste sous file bornée, le suspect sweep/`host.state` reste.
En parallèle : instrumenter d'abord les compteurs descripteurs (slots inspectés/changés)
avant de porter 0055 — ne porter que si le poste est mesuré significatif.

**Design extrait (V5.8, lecture directe des patches)** — async present borné,
principes publics transférables :

- `async_present()` : le `Present` du titre retourne immédiatement ; le present
  réel est enfilé comme `Held::Present(Box<closure>)` sur la file d'opérations
  retenues de la queue — le titre enregistre la frame suivante pendant que le
  worker présente la courante. Actif par défaut (`VICTORY_ASYNC_PRESENT=0`
  désactive) ; requiert `behind()` + `late_free_present()`.
- `FrameLimit` (mutex+condvar) : chaque frame en file occupe un **slot de
  pacing** jusqu'à ce que sa copie atterrisse — `frames_ahead()` borné
  (`FRAMES_AHEAD=2`, clamp 1-8). La borne est *temporelle* (atterrissage de la
  copie), pas seulement un compteur de vols.
- Invariants clés : seules la texture et les listes fermées sont retenues pour
  le present différé ; un titre réutilisant leur stockage attend sa fence
  comme avant ; le refresh ordinaire au submit voit encore les écritures titre.
- `frame_wait` (0046) : instrumentation `Call::Pacing`/`FenceLanding`/locks par
  étape — leurs chiffres (RTX 5080) : temps titre dans Present → 0,1 ms/frame,
  p50 lourd 115,5→108,5 ms, p95 568→262 ms ; le thread de soumission devient
  le chemin critique.
- **Équivalence MESIA** : notre `late_free_present` envoie déjà la copie
  present en vol via `execute_own` ; ce qui manque vs leur design = la borne
  `FrameLimit` par atterrissage de copie et le `Present` DXGI lui-même en file
  (chez nous `queue::present` release seulement). Leur `frame_wait` est aussi
  le modèle pour instrumenter nos attentes (mission 7 : wait sous lock
  `submitted` = candidat connu).

**Phase 3 — copy-back GPU patch + mesh launch** (M, risque B-C) : `relay_patch` 0057
(copy-back 78-173 ms → cible ~15 ms équivalent chez eux) puis `launch` 0041 si le
comptage de draws mesh indirects le justifie. Les deux demandent des chemins GPU
neufs (compute patch, amplification stage) — après stabilisation des phases 1-2.

**Phase 4 — recherche** (L, risque C, non mesuré même chez eux) : UMA 0063 (converge
avec notre `inplace` long terme : directory u32[2]/64 Kio + heaps placés sur sections),
wave64-pairing 0052 (jamais validé sur GPU physique même chez eux — notre mode 2
a ses propres suspects TDR), standins 0050 (après diff XCL), `watch/ahead` 0053.
Aucun item de phase 4 n'entre sans prototype instrumenté.

## Règles transverses

- Chaque portage : env-gate `MESIA_*` défaut off ou fallback intégré, attribution
  GPL §5 en header, tests unitaires portés **avec** le code, puis A/B charge réelle
  (3×300 s, même save, cache chaud) — cf. KELVAROQ-BENCHMARK-PLAN.md.
- Aucun chiffre Kelvaroq n'est un critère d'acceptation ; les critères sont nos ms.
- Tout bloc « Approach after XCL » porté passe d'abord par le diff de provenance
  (KELVAROQ-LICENSE-AUDIT.md).
- Un portage qui régresse est retiré, pas « retuné » indéfiniment (AGENTS.md :
  rejeter les régressions et faux succès).
