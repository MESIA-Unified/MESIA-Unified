# Campagne V5.2 — atteindre 4 images traduites distinctes par seconde

## Reprise active — Devin

Le [handoff du 26 novembre](HANDOFF-DEVIN.md) donne l’inventaire des fichiers, la restauration par SHA-256 et les preuves déjà acquises. Le prochain livrable est une baseline sur replay représentatif autorisé avec copies de retour et oracle graphique. Mesurer ensuite séparément le coût de `declared_writes`, des transferts et des caches sur cette charge. Les travaux Sol, Luna et Astra ci-dessous constituent l’historique de référence.

## Vague graphique Astra du 26 novembre

**Jalon graphique validé en release.** La fixture exécute les deux shaders GFX10, vérifie chaque pixel du back buffer et présente ce buffer par DXGI. Les trois passages traduits totalisent 192 images correctes acceptées et 190 identifiants affichés observés. Les trois passages PC natifs fournissent la comparaison. Les mesures de phases, quantiles et mémoire ainsi que la commande de reproduction figurent dans [l’étude de performances](docs/perf-study.md).

**Reprise Sol.** Utiliser cette fixture comme contrôle graphique et le test `host::failed_and_occluded_presents_do_not_advance_the_meter` pour le compteur. Le benchmark utilise le traducteur MESIA et une file D3D12 dédiée. `mesia-gpu::host::declared_writes`, les copies shadow et le trafic de descripteurs du titre ne sont pas exercés. Pour qualifier les paliers 500 à 250 ms, il manque un replay autorisé avec résolution, mouvements et oracle fixés qui traverse ces chemins. Relever les volumes draw/copies/descripteurs et les temps sous le profil cohérent avant tout changement. Luna peut reproduire ensuite les contrôles sur le même replay.

| Missions | Propriétaire à la construction | Fichiers |
|---|---|---|
| A–C, format des entrées et shaders | Agent shaders | `crates/mesia-shader/tests/graphics/stages.rs` |
| D–G, dessin, fences, oracle et DXGI | Agent rendu | `crates/mesia-shader/tests/translated_graphics.rs` |
| H, compteur hôte, référence PC, compilation, mesures et intégration | Orchestrateur | `crates/mesia-gpu/src/host.rs`, `crates/mesia-shader/tests/graphics/native.rs`, manifeste de la crate, Cargo.lock, documents de campagne et d’architecture |

Après épuisement du quota des agents, l’orchestrateur a repris seul les fichiers de test pour la validation et les mesures. Le premier passage debug a exercé les deux étages traduits, 64 images exactes et distinctes et 64 retours DXGI `S_OK`. Le test unitaire du compteur hôte injecte `E_FAIL`, `DXGI_STATUS_OCCLUDED` puis `S_OK`. Une seule présentation fait avancer le compteur.

## Point de départ au 26 novembre 2026

La source de travail est `C:\Users\utilisateur\Desktop\mesia-main`, sans historique Git local. Le miroir voisin `mesia-upload` est propre à `293b8a4` sur `main`. Avant cette initialisation, ses 196 fichiers suivis concordaient avec le dossier principal après normalisation des fins de ligne. Le principal contient aussi des fichiers locaux, dont `docs/architecture.md` et `scripts/wer.ps1`. Préserver ces écarts à chaque transfert. Le remote configuré est [MESIA-Unified/MESIA-Unified](https://github.com/MESIA-Unified/MESIA-Unified).

| Observation | Portée pour la campagne |
|---|---|
| AUTONOMOUS-NOTES.md rapporte ~837 ms/image en V4 (~1,19 FPS), puis ~500 ms en synchrone avec Fast. | Les scènes et la cohérence diffèrent. La plage annoncée de 500–540 ms (1,85–2 FPS) reste à reproduire avec images distinctes. Le dernier échantillon de `%TEMP%\mesia.log` affiche 865,7 ms le 26 novembre à 00:31:49 UTC. La scène et l’état du cache de cette mesure sont inconnus. |
| `016dddb` protège l’écriture des slots UAV partagés dans `memory::ready` via `host::settle_and_hold`. | Le journal local `crash_v5_1.txt` rapporte encore 23 soumissions anticipées, 131 listes et DEVICE_HUNG (0x887A0006). DRED renvoie 0x887A0004 dans ces traces. Les breadcrumbs applicatifs restent exploitables. Le blocage résiduel reste à expliquer. |
| Le drain synchrone a tenu environ 18 minutes dans le test consigné. `293b8a4` rétablit INPLACE=1 après régression du niveau 2. | Utiliser le profil synchrone avec copies de retour. Fast conserve les uploads, supprime les copy-backs et espace certains rafraîchissements. Garder ses chiffres dans une série distincte. |
| RTX 3060 de 12 288 Mio, pilote 617.14 relevé à l’initialisation. Budget communiqué de 16 Go DDR4 à 2666 MT/s, 15,8 Go utilisables, environ 9,2 Go occupés au repos et jusqu’à 15,3 Go sous charge. | Relever la mémoire disponible avant chaque vague. Le lanceur fixe 12 workers de pipelines. Mesurer leur coût et réduire leur nombre dans un profil explicitement consigné si la marge disparaît. |

Les résumés V4 et V5 figurent dans AUTONOMOUS-NOTES.md et dans les commits du miroir. Aucune directive autonome V4, V5 ou V5.2 retrouvée dans les deux arbres source. Ce plan interprète la demande du 26 novembre. Il manque une scène **originale ou autorisée, représentative et rejouable sur le chemin traduit**, accompagnée d’une référence de pixels et de données, de sa configuration et d’une série de mesures comparables. Aucun `mesia.toml` local identifié à la racine. Les rapports datés ne constituent pas un passage actuel de la suite.

### Reprise d’exécution — 26 novembre, 01:39–01:43 UTC

La tentative Cargo ciblée a commencé par recompiler `mesia-shader`, puis son lien MSVC a été interrompu après plus de deux minutes de silence. Il n’a produit aucun binaire. À ce moment-là, le seul processus rustc observé occupait ~209 Mio et la RAM disponible était retombée à 4,24 Gio. Les deux exécutables de tests préexistants étaient datés du 25 novembre. Le plus récent a été exécuté une première fois avec le filtre trop large `valu_tests::` : libtest a démarré ses 17 cas en parallèle. Six réussites ont été observées, plusieurs tests GPU sont ensuite restés actifs plus de 60 s. Le processus a été interrompu. Aucun résultat complet ne peut en être tiré.

La reprise a vérifié l’absence de processus cargo/rustc ou de l’exécutable test interrompu. RAM libre ~4,40 Gio. Une exécution ciblée et sérialisée avec le binaire existant, `mesia_shader-53dcddc2fbebbe39.exe --exact emit::valu_tests::v_fract_f32_never_returns_one --test-threads=1 --nocapture`, a réussi en 2,97 s : **un test de conformité VALU synthétique**. Cela confirme un cas du chemin GFX10 traduit sur l’adaptateur employé par ce binaire ; ce n’est ni une scène animée ni une mesure de FPS. Utiliser désormais `--exact` avec les exécutables harness et un seul cas GPU à la fois.

### Suite séquentielle et scène de référence — reprise suivante

OBS est arrêté. Avant test : RTX 3060, pilote 617.14, 11 093 Mio de VRAM libres, utilisation GPU 5 %, RAM libre 6,53 Gio / total 15,84 Gio. L’exécutable de tests du 25 novembre `target/release/deps/mesia_shader-53dcddc2fbebbe39.exe` contenait les 17 tests `emit::valu_tests`. Tous ont réussi, lancés un par un avec `--exact --test-threads=1`. Durées des tests GPU rapportées par libtest : bfe 10,28 s, cvt-16 10,08 s, atomics-across-waves 9,82 s, atomics-order 42,91 s, omod/clamp 9,91 s, signed zero 10,18 s, packed-add 7,33 s, cmp-class 5,45 s, cmp-NaN 5,21 s, cvt-norm 19,38 s, cvt-u16 5,46 s, fract-f16 2,67 s, fract-f32 2,70 s, med3-f16 2,54 s, med3-f32 2,66 s, sin/cos-f16 5,08 s, sin/cos-f32 10,24 s. Zéro échec et aucun test concurrent.

L’exécutable récent `synth_scene-eae9b63def607ef6.exe` listait zéro test. L’ancien `synth_scene-732c46282774161d.exe` contenait cependant `synthetic_scene_renders_and_refreshes`. Avec `MESIA_WARP=1` et DXC PC du Windows SDK 10.0.26100.0, il a réussi en 1,30 s, en vérifiant les pixels goldens et les changements entre deux images. Cela établit l’oracle de rendu natif synthétique uniquement ; ce binaire ne compile pas MESIA et ne mesure pas son chemin traducteur. Usage final mesuré : GPU 3 %, 11 385 Mio VRAM libres, RAM 8,89 Gio. Aucune scène MESIA traduite et animée avec readback comparable n’est présente dans les fixtures disponibles ; ne pas appeler ces résultats une baseline FPS.

À 01:39 UTC, `nvidia-smi` indiquait 18–22 % d’utilisation et environ 10,4 Gio de VRAM libres, avec OBS actif. Une mesure ultérieure attribuable au benchmark serait contaminée. Le GPU n’a pas été benchmarké. L’origine du lien MSVC prolongé reste non diagnostiquée. Aucun moteur ou test n’a été modifié.

## Vagues et critères de passage

1. **Établir la référence.** Fixer scène, mouvements, résolution interne, durée et état de cache. Relever build, pilote, RAM libre et variables `MESIA_*`. Utiliser un environnement propre, `scripts/play.ps1 -Game <chemin-exe-autorisé>` sans Fast, `LATE_WAIT=0`, `INPLACE=1`, relais absent. Le script préfixe ses variables par `MESIA_`. Écarter `scripts/play-test.ps1`, qui force le relais et les attentes différées. Si la scène manque, préparer d’abord une fixture originale et établir son oracle.
2. **Fermer les risques P0.** À partir des traces existantes, inventorier les écrivains de descripteurs, lectures en vol, transitions, recyclages d’allocators et sorties de boucle avant barrière collective. Distinguer la fence qui autorise chaque réutilisation. Construire une reproduction originale minimale et bornée, tester la correction isolément. Au premier reset, arrêter, conserver motif HRESULT, dernier progrès de fence, breadcrumbs et DRED si disponible. Garder le mode asynchrone désactivé par défaut.
3. **Profiler le chemin cohérent.** Mesurer préparation, pages parcourues, copies aller/retour, attente GPU, verrous, coût par draw, PSO manquants et charge des workers. Séparer les durées qui se chevauchent. En build diagnostics, la présence de `MESIA_GPU_TIMING`, même à zéro, force `late_stage() = 0`. Comparer aussi sans instrumentation. Archiver les journaux hors dépôt avant que le lanceur les remplace.
4. **Changer une variable.** Choisir le coût dominant mesuré. Exemples à départager par profilage : copies de plages inchangées, contention au binding, redondances DXIL, pression des registres et compilation répétée. Tester avec l’option expérimentale désactivée puis activée. Contrôler barrières, opérations vectorielles, résultats numériques, images et fraîcheur inter-image. Vérifier clé/version et aller-retour de sérialisation des caches si la traduction change.
5. **Comparer et tenir la charge.** Faire trois passages par variante à conditions identiques, caches froid et chaud dans des séries séparées. Rapporter durée, FPS internes et images distinctes valides par seconde, ms/image moyen, P50/P95/P99, temps CPU/GPU/attente, erreurs, draws omis et différences de pixels/readbacks. Mesurer working set, private bytes, mémoire engagée, RAM libre, pagination, VRAM dédiée/partagée, tailles et pics des caches, temporaires et workers. Vérifier la progression attendue et les données relues sur plusieurs images. Un menu, un écran noir, une image répétée, un pic isolé ou une passe native ne qualifient aucun palier.
6. **Décider.** Viser successivement 500, 400, 333,3, 285,7 et 250 ms/image (2 à 4 FPS par demi-FPS). Avant annonce, effectuer un essai prolongé sur la scène traduite avec durée consignée, résultats fidèles, cadence régulière et mémoire stable. Revenir au dernier état validé si défaut, retracer la cause puis réévaluer le prochain goulot. Inscrire mesures dans `docs/perf-study.md`, incidents et décisions dans AUTONOMOUS-NOTES.md, reprise dans HANDOFF-FPS.md.

## Matrice de propriété

Les 24 missions sont des responsabilités, pas un nombre de processus à lancer. Pour cette initialisation, les audits GPU, shaders/tests et démarrage sont en lecture seule. L’orchestrateur possède exclusivement AGENTS.md, PLANS.md et docs/architecture.md. Pour chaque future vague, inscrire les **chemins exacts**, un propriétaire et les dépendances avant le premier changement. Les lignes suivantes définissent les périmètres d’audit. Un fichier partagé tel que `host.rs` passe à un seul auteur par vague. Les autres remettent leurs observations en lecture seule.

| Groupe | Missions | Livrable attendu avant intégration |
|---|---|---|
| A · Stabilité | A1 DRED, A2 fences, A3 durée de vie, A4 listes/allocators, A5 concurrence et sûreté, A6 validation | Chronologie de l’incident, dernière fence consommée et preuve de réutilisation sûre. |
| B · Ressources | B1 descripteurs, B2 buffers temporaires, B3 cohérence, B4 pression | Propriétaire et dernier lecteur de chaque slot/plage, readback de contrôle. |
| C · Shaders | C1 numérique, C2 synchronisation, C3 optimisation DXIL, C4 cache | Test différentiel, contrôle des barrières et invalidation démontrée si nécessaire. |
| D · CPU | D1 transferts, D2 coût par draw, D3 threads/verrous | Profil du chemin critique, hypothèse et comparaison isolée. |
| E · Mémoire | E1 RAM/VRAM, E2 allocations/caches, E3 adaptation | Pics, évolution en régime établi et effet sur la pagination. |
| F · QA | F1 unitaires, F2 intégration, F3 benchmarks, F4 intégration finale et endurance | Rapport des tests exécutés ou sautés, séries brutes et décision de l’orchestrateur. |

Intégrer après contrôle des dépendances entre lots : A/B avant toute expérience en vol, C/B avant de comparer une image traduite, D/E avant le benchmark F3. F4 utilise un seul GPU libre. Une faible marge RAM impose des missions séquentielles. Chaque lot fournit son diff, sa commande de test, son résultat et son rollback.

## Tests et preuves disponibles

### Vague V5.2 en cours

Matrice de propriété effective : agent A possède exclusivement `crates/mesia-shader/src/emit/mod.rs` pour une fixture graphique synthétique originale ; agent B/E a audité en lecture seule le coût de `declared_writes` ; l’orchestrateur possède exclusivement `crates/mesia-gpu/src/host.rs`, `PLANS.md` et `AUTONOMOUS-NOTES.md`. Aucun test GPU ni compilation simultanés. Le diagnostic B/E relève la recherche linéaire `back.iter_mut().find` de `declared_writes`, mais son temps dans le chemin critique reste à mesurer avant modification.

Correction P0 intégrée dans `host.rs` : vérifier le HRESULT de `IDXGISwapChain::Present`, journaliser l’échec et éviter d’avancer le compteur de présentation de l’hôte après cet échec. `cargo check --locked -j 1 -p mesia-gpu` et `cargo test --locked -j 1 -p mesia-gpu --lib --no-run` réussissent. La source n’a encore subi aucun test de présentation en conditions d’échec. Le compteur `queue::FRAMES` compte les appels du titre ; il ne doit pas servir à valider des images visibles. Le contrôle global `cargo fmt --all -- --check` a échoué sur de nombreux écarts préexistants et n’a modifié aucun fichier.

L’agent A a ajouté dans `crates/mesia-shader/src/emit/mod.rs` une fixture GFX10 originale sérialisée en DXBC/XDAT/XBCH/FSSM. Elle traverse `emit::translate`, crée un pipeline compute D3D12, envoie huit valeurs dynamiques et vérifie chaque readback contre la référence CPU `frame = 1..8`. `cargo test --locked -j 1 -p mesia-shader --lib original_translated_frames_match_their_cpu_reference -- --test-threads=1 --nocapture` a réussi en debug. Une erreur initiale de type `E0689` dans la fixture a été corrigée par l’agent A.

Le build release a pris 1 min 37 s avec un job, RAM libre observée 7,93–8,31 Gio. Son test a réussi, huit sorties correctes, P50 24,648 ms et maximum 45,411 ms. Trois répétitions release sans recompilation ont aussi réussi avec P50 respectifs 21,887, 22,276 et 21,593 ms ; maximums 37,546, 24,517 et 27,409 ms. Après les runs, RAM libre 8,47 Gio, VRAM libre 11 384 Mio et GPU 6 %. Ces temps incluent l’enregistrement du dispatch, l’attente et le readback. Le test alloue des ressources par appel et n’a ni géométrie ni présentation DXGI. Les huit valeurs distinctes ne sont pas des images visuelles. **FPS effectifs de la cible traduite : non établis.**

Prochain jalon A/F : construire une scène originale avec vertex/pixel GFX10 traduits, géométrie et ressources animées, readback pixel par pixel et présentation DXGI vérifiée. Comparer au rendu HLSL PC sous les mêmes conditions puis seulement profiler les fonctions de transferts et `declared_writes`. Le format source XDAT/XBCH/FSSM et les métadonnées de vertex export rendent cette étape architecturale ; un passage à GPT-6.2 Astra est recommandé pour la conception et la revue de ce harness. La correction P0 de `Present` n’a pas de gain de performance annoncé et nécessite un essai ciblé avec échec DXGI provoqué avant validation complète.

| Contrôle | Entrée et interprétation |
|---|---|
| CLI et configuration | `cargo build --release --locked -j 1 -p mesia` puis `target\release\mesia.exe --help`. Le profil `diag` avec `--features diagnostics` sert aux traces détaillées. |
| Unitaire GPU | `cargo test --release --locked -j 1 -p mesia-gpu -- --test-threads=1`. Ajouter un filtre de nom pour isoler un cas. Le nombre de 127 tests réussis est un résultat historique V4. |
| Shaders traduits | `cargo test --release --locked -j 1 -p mesia-shader --features host valu_tests -- --test-threads=1 --nocapture`. GFX10 synthétique, cas numériques et atomiques. Les tests de `emit/diff_tests.rs` comparent rendu et compute avec DXC PC et un compilateur console autorisé si disponibles. |
| Référence native | `cargo test --release --locked -j 1 -p mesia-shader --features host --test synth_scene -- --test-threads=1 --nocapture`. Deux images avec vérification de fraîcheur. `compute_correctness` et `compute_advanced` exercent les références HLSL. `heavy_load` est un banc natif optionnel sous `MESIA_HEAVY=1`. Relever adaptateur matériel ou WARP, outils disponibles et tests sautés. |
| Autres intégrations | Fixture originale `tests/fixtures/hle-probe`, tests loader `load_probe` et `title_dlls`, test HLE `no_dll_planting`. Microbenchmarks ignorés : `bench_write`, `texture_checks_per_submission` et `heavy_frames_of_merges_run_wide_and_merge_the_same`. Précompiler avec `--no-run` avant de chronométrer. |
| Suite globale | Après les contrôles ciblés, `cargo test --release --workspace --locked -j 1 -- --test-threads=1`. Pour compter sans exécuter les tests, utiliser `-- --list` (compilation possible). Détails dans docs/testing.md et docs/benchmarks.md. Aucun seuil de couverture chiffré trouvé dans la CI. |
