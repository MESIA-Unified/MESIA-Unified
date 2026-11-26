# MESIA — Étude d'optimisation mesia-gpu

## Mesures V5.5 — Kelvaroq phase 1 (27 novembre, binaire reverté)

Contexte : binaire post-`aa9be38` + `plaincopy` (dormant) + `defer_host_read`
(reverté au call-site après bisect). `LATE_WAIT=2`, même save, cache chaud.

| Run | Config | Résultat |
|---|---|---|
| A | gating `Step::Host` + `MESIA_COPY_PLAIN=1` | **DEVICE_HUNG** ~4-5 min in-world (« 5 of 11 »), P50≈485 ms warm-up inclus, copies ~86-100 ms/~100 Mo |
| B | gating seul (COPY_PLAIN absent) | **DEVICE_HUNG** ~35 s in-world (« 4 of 10 ») |
| témoin | reverté, plaincopy dormant | in-world >10 min, **0 crash**, ~441-518 ms/frame steady, 4 160 draws/frame |

Verdict : le gating naïf `Step::Host` est le crashur (auto-dépendance
intra-liste probable) — reverté ; `COPY_PLAIN` disculpé du crash mais gain
non démontré (A/B à refaire sur charge isométrique). Soak long en cours.

## Baseline graphique V5.2 du 26 novembre 2026

Windows 11, RTX 3060 12 Go, pilote 617.14, release MSVC, un job de compilation terminé avant les tests. Chaque processus ouvre une fenêtre 128 × 64 et rend deux triangles magenta sur fond noir. Le buffer de sommets change à chaque image. La référence GFX10 construit ses propres signatures et métadonnées, puis fait passer **le VS et le PS** par `emit::translate` et le validateur DXIL du SDK. Le mode PC compile deux entrées HLSL originales avec DXC. Les deux modes utilisent le même PSO graphique par configuration, le même oracle CPU et la même boucle de présentation.

Après 8 images d’échauffement, chacune des 64 soumissions doit terminer sa fence, reproduire exactement les 8 192 pixels prévus à une position unique et obtenir `S_OK` pour ce même back buffer. La fence précède la réécriture du VB, la réinitialisation de l’allocateur et la lecture des résultats. Le heap RTV, les deux buffers DXGI et les ressources de lecture restent vivants jusqu’à la fin. Trois timestamps GPU encadrent le dessin et la copie. `Present(1, 0)` utilise la synchronisation verticale. L’observation des IDs via `GetFrameStatistics` continue jusqu’au dernier ID ou pendant 250 ms supplémentaires. Un ID manqué reste inconnu. La borne en dernière colonne inclut cette fenêtre supplémentaire.

| Passage | Shaders | Moy. ms | P50 ms | P95 ms | P99 ms | Images correctes acceptées/s | IDs affichés observés | Borne affichée/s |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 1 | GFX10 traduit | 6.9473 | 6.9432 | 7.0698 | 7.2172 | 143.937 | 63/64 | 90.693 |
| 2 | PC natif | 6.9444 | 6.9104 | 7.1767 | 7.2563 | 143.997 | 63/64 | 90.554 |
| 3 | PC natif | 6.9473 | 6.9472 | 7.0577 | 7.1450 | 143.938 | 63/64 | 112.339 |
| 4 | GFX10 traduit | 6.9463 | 6.9328 | 7.1457 | 7.2919 | 143.959 | 63/64 | 90.510 |
| 5 | GFX10 traduit | 6.9451 | 6.9472 | 7.0917 | 7.2749 | 143.982 | 64/64 | 137.249 |
| 6 | PC natif | 6.9443 | 6.9391 | 7.1287 | 7.1595 | 144.001 | 62/64 | 89.179 |

Sur les trois passages traduits, initialisation D3D12 176,4–182,3 ms, construction et traduction des conteneurs 0,65–1,04 ms, validation 1,59–1,64 ms, création du PSO 0,50–0,54 ms, fenêtre et ressources 29,4–59,3 ms, échauffement 49,1–52,1 ms. Les caches du pilote étaient conservés. Les trois compilations HLSL de référence ont pris 34,1–36,0 ms par paire.

En régime établi, moyennes par image traduite : préparation CPU et soumission 0,090–0,101 ms de temps mural, attente de fence 6,61–6,64 ms, vérification CPU 0,0165–0,0173 ms, appel Present 0,195–0,212 ms, rendu GPU 0,0022–0,0023 ms et copie GPU 0,0029 ms. Le compteur de temps CPU du processus n’a pas avancé à sa résolution sur ces fenêtres courtes. La copie du back buffer et la vérification exhaustive font partie de la mesure. La résolution des timestamps et les requêtes DXGI ajoutent aussi du travail. L’attente synchrone sous cadence écran domine cette fixture. Elle ne chiffre pas les économies potentielles du moteur sur plusieurs milliers de draws.

Fin des passages traduits : working set et pic du processus 74 Mio, mémoire GPU locale attribuée 11 Mio et non locale 26 Mio, RAM système disponible 8 255–8 277 Mio, engagement 13 591–13 616 Mio pour une limite de 65 368 Mio. Chaque passage a réussi les contrôles DXIL, PSO, fence, pixels et HRESULT. La stabilité observée porte sur ces courtes séries.

Depuis PowerShell dans la racine du projet, lancer un seul processus sur le bureau interactif. Pour la comparaison PC, fixer `MESIA_GRAPHICS_NATIVE=1` et éventuellement `MESIA_PC_DXC`. Le mode par défaut sélectionne les shaders traduits.

```powershell
cargo test --release --locked -j 1 -p mesia-shader --features host --test translated_graphics --no-run
cargo test --release --locked -j 1 -p mesia-shader --features host --test translated_graphics -- --ignored --exact translated_graphics_baseline --test-threads=1 --nocapture
```

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
