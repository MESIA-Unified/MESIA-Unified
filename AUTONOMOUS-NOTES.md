# Session autonome — notes de travail (2026-11-25)

## État live
- Run courant : mode 2 (défaut) + `MESIA_D3D12_DRED=1 MESIA_GPU_TIMING=1`, binaire avec fix échange-pair — en gameplay (prompt L/R affiché), veille crash.
- 5 TDR `nvlddmkm` ID 153 : 11:22, 12:04, 12:37, 12:44 UTC. Runs longs (60+ min) sans crash → **stochastique**.

## TDR — mécanisme prouvé (2026-11-25 soir)
- **A/B GROUP_SYNC** : `=1` → corruption visible (cheveux/persos = bouillie magenta, échanges LDS racy). `=2` (défaut) → rendu propre. Donc des shaders avec `s_barrier` en région divergente **existent** ; le carve-out `fence_only` est utilisable en mode 1 mais casse la synchro dont ces barrières ont besoin quand tous les threads arrivent quand même.
- **Conséquence** : en mode 2, une divergence runtime qui échoue à atteindre la barrière (sortie anticipée, ou LOOP_LIMIT `abort`→`Ret` pour les boucles `Jump`-latch — lower.rs:10624) fige le groupe → hang → TDR. Stochastique ✓.
- **Audit lower.rs (agent 1f171d33)** — suspects classés : #1 s_barrier groupe-wait en bloc divergent (défaut `_ => true` wait) ; #2 LOOP_LIMIT abort→Ret échoue les barrières meeting/exchange post-boucle ; #3 dword `hi` non initialisé (CORRIGÉ, commit 447b6fa) ; #4 shortcut `lds_meetings` two-writes ; #5 cmpxchg retry hors LOOP_LIMIT ; #6 `widen()` activé au retry LANE_MASK malgré historique de perte carte (vérifier `MESIA_WAVE_WIDE=0`).

### Fix #2 — design validé (à implémenter, ~50 lignes dans lower.rs)
Problème : latch `b` d'une boucle `Jump`-latch (`header: s_cbranch out; corps; s_branch header`) fait `CondBr(ok, header, abort)` avec `abort`→`Term::Ret` (lower.rs:10634-10638, `abort_block` :10761). Le lane quitte → barrières groupe post-boucle bloquées → TDR.
Correct : `abort` doit sauter à `out` (sortie du header `t`) **avec les phis remplis** : la valeur entrante pour l'arête `t→out` est définie au plus tard dans `t` → domine `b` → rematérialisable dans le bloc abort.
- Identifier `out` : `st.exits[t]` ; si `LaneExit::Branch{taken,not_taken}` → le successeur dont `rpo` ∉ (rpo[t], rpo[b]] est la sortie ; si ambigu → garder Ret (conservateur).
- Créer `ab` (new DXIL block) terminant `Br(self.block[out])`; enregistrer `(ab, t, out)` dans une `Vec` sur self.
- Dans `fill_phis` : pour chaque phi de `out`, pour chaque `(ab, t, out)` enregistré où `t` = pred index n → matérialiser la même valeur entrante (`phi.incoming[n]` / `args[n]`) dans `ab` et `incoming.push((x, ab))`.
- Ne PAS rappeler `self.edge(t,out)` (pas de dédup — crée un 2e bloc orphelin = préd sans incoming).
- Couverture : les boucles `Branch`-latch sont déjà correctes (limite ANDée à la condition → sortie naturelle, phis OK : lower.rs:10672-10679).
- **Souris→caméra validée en gameplay** : drag souris → la caméra pivote (shots s34→s35).

## TDR — ce qui est écarté / confirmé
- Allocators `Reset` : les warnings "in flight" sont un **symptôme** du hang (file GPU bloquée → toutes les listes derrière restent in-flight). Les 6 sites de Reset sont tous fence-gatés correctement.
- Heaps de descripteurs >1M : volontaires (probe `PORTABLE_VIEWS` + fallback cappé null-slot).
- Indirect dispatch : clamp de budget 2^27 — max observé 519 groupes, sain.
- `MESIA_DEFER_FETCH` : OFF dans play.ps1 → innocenté.
- Commandes GPU : max 134 ms (timing run) — aucune commande seule n'approche le TdrDelay=2 s en régime courant.
- Suspect restant : **shader traduit en boucle infinie** (lowering wave64→32, ballots/mbcnt) ou page fault GPU. Le breadcrumb DRED au prochain crash tranchera.

## Fix appliqués (commit mesia-upload, non pushés)
- `0552eed` input : vélocité souris accumulée avec decay (~200 ms) — right_x atteint enfin `gamepad_state` (cause racine prouvée : delta 8 ms vs poll 30 Hz).
- `5588171` shader : `v_permlanex16_b32` loweré (agent c5c203bb).
- `b6827d6` diagnostics : probes xgamesave + marqueurs reset par site + scripts shot/drive.
- `6034e61` cache-key inclut host_max_wave (déjà poussé).

## Campagne V3 — audits livrés (2026-11-25)

### A3 — durées de vie GPU (agent 6fe76fd0)
- **#1 PRIME** : écritures `mirror`/`unmake` heap shader-visible depuis thread titre sans vérif in-flight (descriptor.rs:758-843→host.rs:1258) ; shaders bindless `root_indexed_views` lisent tout le heap (`heap_buffers` draw.rs:1270) → descripteur déchiré mi-vol → fault GPU → removed. Matche sync-stable/late-crash. Fix en cours (agent 92b5afbe).
- **#2** : échecs `SetEventOnCompletion`/`Signal` avalés → land/finish/recycle contre travail non terminé (host.rs:4070, 4346-4389). Fix en cours (agent 5477a801).
- **#3** : anneaux CPU RTV/DSV (`host.rs:4631-4660`, RTV_RING 1024) réécrits à l'enregistrement sans suivi d'exécution — course latente sous late.
- Vérifiés sains : retire/bury, pools scratch/prelude/listes, `staging.in_air`, `p.ahead` (flown<=FINISHED), SHADER_HEAPS TOCTOU.

### F1 — hot-path CPU (agent d1551c8b)
- Top 1 estimé : pipeline refresh/copy sérialisé sur le thread de soumission (`run_deferred` + `Batch::flush` + watch/guard ~10 GB scannés FULL à chaque soumission — scan non incrémental). Flags d'isolation : `MESIA_ASK_ONCE=0`, `MESIA_GUARD_HOLES=0`, `MESIA_COPY_MODE=plain|none`, `MESIA_UNWATCHED_FRAMES`.
- Top 2 : `relay_wait` ~200 ms/submission — le soumetteur attend le travail CPU du relay pendant GPU idle (host.rs:2346). Mesurer via `late_line`/`RELAY_STATS[4]`/`card_clock_line`.
- Top 3 : `tables()`/`bind`/`heap_buffers` ×5000 draws (hash mémoire titre par table fraîche, `dynamic()` scan linéaire sous mutex, mirror par slot).
- Quadratic flaggés : `find`+`Vec::contains` dédup O(ranges²) à declared_writes/card_before/settle/relay_land ; `watch::wrote` journal par page ; RTV/DSV ring.
- Compteurs morts : `C::Queries/QueryUs/Skipped/SkippedBytes` imprimés mais jamais écrits — "queries 0.00 ms" n'est pas une donnée réelle.

### A4 livré : breadcrumb applicatif (commit 11bbf2c)
`fail()` dump `in-flight list` par liste en vol (soumission, pipelines, derniers steps ; vraies descriptions commandes sous feature diagnostics) — DRED unsupported (0x887A0004) confirmé.

## En attente / backlog
- Mouse caméra : fix implémenté, **pas encore vérifié en gameplay** (cinématique en cours, input sans effet).
- Cinématiques noires : slots device `0x1c8`/`0x218` non implémentés (16k appels/run) — RE profond.
- Audio cinématique : `BinkSetSoundSystem` title-provider (PCM va dans RAGE, SHAPE jamais exécuté).
- mesh+pixel `0x80070057` en library load → recréés à chaud (hitch ~450 ms).
- Compute `0x80070057` : WaveSize(64) sur hôte 32 lanes — confirmé live.
- Visages/ghosting : vu seulement au 1er run froid ; pas reproduit depuis (skins warm ?).

## Tooling
- `scripts/shot.ps1` capture fenêtre ; `scripts/drive.ps1` injecte touches/souris (focus réel requis).
- `scripts/wer.ps1` liste les events nvlddmkm.
- Env utiles : `MESIA_D3D12_DEBUG`, `MESIA_D3D12_DRED`, `MESIA_GPU_TIMING`, `MESIA_LOG=info,mesia_hle::modules::gameinput=trace` (besoin build `--features diagnostics` pour debug/trace).

## 2026-11-25 (soir) — tempête de pipelines : cause racine + fixes

### Pourquoi l'écran de chargement revenait toujours
- `library::save` n'était appelé que quand la file se vide (`drained()`) ou à l'arrêt propre. Une tempête de ~2400 pipelines ne se vide JAMAIS avant un kill → tous les PSO `StorePipeline`'d depuis le boot perdus → tempête rejouée à zéro. Fix `fbf8577` : `save(false)` appelé dans la boucle worker à chaque make (throttle 20 s interne) → un kill ne perd que ≤20 s.
- Workers à `THREAD_PRIORITY_BELOW_NORMAL` en permanence → affamés par les threads titre pendant le chargement (vidange ~0.2/s, ~40 s par make). Fix `fbf8577` : `wait()` passe le worker qui construit le pipeline attendu à `NORMAL` (`builder: AtomicU32` + OpenThread/SetThreadPriority), re-baisse en tête de boucle. Inspiré de `pso_async::want()` de xcl. Vidange mesurée ~2/s après fix (×10).
- `play.ps1` : `MESIA_WAVE_WIDE=0` (widen retry path known-bad, audit B1) + `MESIA_PIPELINE_WORKERS=12` ajoutés.
- `shot.ps1 -PrintWindow` : capture la fenêtre jeu même occultée (PW_RENDERFULLCONTENT).

### Leçon xcl (github.com/xcl-dev/xcl, pso_async.rs)
- Stand-in pipeline immédiat + vrai pipeline en worker (MESIA droppait les draws à la place).
- `XCL_LAZY_PSO` : 51 000 pipelines créés/30 s, peu utilisés → builder au premier bind seulement. MESIA fait déjà l'équivalent (queue au premier `ready()`).
- `xcl shaders --precompile --used-only` : précompile hors-jeu depuis `used-pipelines.txt`. MESIA a `mesia translate` (shaders seulement ; les PSO ont besoin de l'état graphique — pas de dump persistant de ça, à faire si les tempêtes restent un problème).
- Worker count = taille du pool DXC (`compile_threads()`).

### `mesia run` attend l'EXE du jeu, pas le dossier
`fs::read(exe)` sur un dossier → os error 5 « Accès refusé ». play.ps1 `-Game` = chemin de `game_scarlett_master.exe`.

## 2026-11-25 (fin de session) — résultats

- Boot à cache chaud : sélecteur de profil atteint en ~1 min (avant : tempête ~40 min ou infinie à cause des kills).
- Tempête finie (~2400) en ~7 min à ~2-15/s (vs 0.2/s avant le boost worker).
- `mesia run` prend l'EXE : `-Game ...\game_scarlett_master.exe`.
- Resize fenêtre ≠ résolution interne (rendu à résolution fixe du titre) — pas de levier GPU par là.
- Frame cinématique ~766-826 ms : ~47% GPU (~380 ms) + ~390-590 ms CPU soumetteur (card idle behind us). Côté CPU = la cible Phase 2 (F1: watch/copies/binding, relay_wait ~200 ms).
- `play.ps1 -Fast` : COPY_MODE=plain, GUARD_HOLES=0, UNWATCHED_FRAMES=4 — 1er essai fluidité>fidélité (textures pouvant stagner).
- GitHub : tout pushé jusqu'à 62068a3.

## Campagne V4 (reçue) — perf recovery 3-5 fps
Plan : baseline reproductible (Phase 1) → CPU (watch incrémental, relay_wait, binding) → GPU/shaders → cache → paliers 500/333/250/200 ms → endurance. Handoff : HANDOFF-FPS.md. Ordre d'exécution fixé : mesure → goulot #1 → re-mesure → #2 → #3 → GPU.

## 2026-11-25 — campagne V4, vague 1

### Mesures (frame in-world, nouveau binaire 19:58)
- ~837 ms/frame, ~4500 draws ; carte ~48-51 % occupée, ~600 ms/frame de travail CPU pendant que le GPU attend derrière.
- `preparing: copies` ~237-360 ms ; `copying back` ~109-173 ms ; ~380 hash passes / ~2400 pièces + ~89 compare passes / ~17 000 pièces par frame.
- Le split GPU : ~90-95 % = le rendu du titre lui-même (shaders traduits), ~25-45 ms = notre overhead (dispatches hash/compare, copies, readbacks). Source : audit agent C (rapport dans l'historique de session).

### Crash DEVICE_HUNG (0x887A0006) — bisection (corrigé après 5e crash)
- Le breadcrumb applicatif (21a93e2) a FONCTIONNÉ : dump des listes en vol + pipelines nommés aux crashs. Blocs dans %TEMP%\crash_20533.txt et crash_2_relay.txt.
- 5 crashs reproduits, tous in-world, tous avec soumissions en vol (« sent ahead of N lists »).
- **Tout mode flight finit par crasher** : `LATE_WAIT=1` a tenu ~5,5 min une fois (chance) puis a crashé en ~1,5 min ; `LATE_WAIT=2` crash en ~10 s ; `LATE_WAIT=3` ~40 s ; `RELAY=2` ~10-60 s. Seul le drain synchrone (`LATE_WAIT=0` ou GPU_TIMING) est stable (soak 18 min + runs répétées).
- Le bug est donc dans la mécanique flights elle-même (n'importe quel stage ≥1), pas spécifiquement relay/late_free_present. Piste principale : écritures descripteurs non couvertes par le gating 176b425 pendant les vols ; vie des ressources soumises en vol.
- Config livrée : **`LATE_WAIT=0` + `-Fast`** (play.ps1) → ~600 ms/frame in-world, 0 crash, rendu correct vérifié.

### Changements V4 intégrés (commits)
- `444839b` COPY_MODE=none rendu réel (était dead code en release) + pool wide 4-10 + dédup declared_writes/card_before.
- `a201f1a` caches epoch `Shown` dans descriptors, dédup defer/gpu_wrote, IASet skips, Weak VB/IB — ~15-45 ms/frame estimés.
- `985daf4` **correctif sémantique no_copies** : « none » = no copy-BACKS seulement ; uploads titre→shadow→GPU restaurés (update_free copie, copy_mapped memcpy, card_before garde dirty sends). Sans ce fix : écran noir à ~5000 draws (shaders lisaient des zéros). writing() reste court-circuité → pas de snapshots → back() no-op via declared_writes vide. watch::probed supprimé (orphelin).
- 127 tests mesia-gpu verts.

### À faire ensuite
- ~~**Fixer le bug flights**~~ → fait : `016dddb` (voir § V5 vague 1 ci-dessous).
- Mesurer le gain réel des commits A+B en synchrone propre (baseline 837 → ~600 ms observé, mais scènes différentes — refaire une mesure même scène).
- Candidats GPU agent C : barrières superflues shaders traduits (UAV fence global par s_barrier — 50-150 ms estimés), hash-on-CPU pendant upload, batch compares, coalescing ranges upload.
- Caveat `no_copies` : écrits shader→mémoire titre jamais rapatriés — OK perf, pas correct pour gameplay (animation/visibilité GPU→CPU figées). Durable : copy-back « cheap but correct » (items C 2-6) OU `MESIA_INPLACE=2` (écrits directs en mémoire titre — voir ci-dessous).

## 2026-11-25 — campagne V5, vague 1

### Bug flights — cause racine identifiée et fix commité (`016dddb`)
- Agent read-only : la mutation **non gatée** = `memory::ready` (memory.rs) qui écrit des descripteurs UAV 32 octets dans le run partagé heap-lifetime (`raw_uav_at`) pendant qu'un flight est en vol → shader en vol lit un descripteur déchiré → VA sauvage → DEVICE_HUNG. Trou dans le gating 176b425 (couvre mirror/mirror_shown, pas raw_uav_at).
- Fix : `host::settle_and_hold()` — settle les flights en vol **puis garde le lock `submitted`** pendant l'écriture des slots manquants (aucun nouveau flight ne peut partir entre settle et writes). Reste du code inchangé ; directory dwords atomiques et slots null = bénins.
- Secondaire (non fixé, data-level) : `strides` écrit même quand le descripteur est deferred ; `Step::Host` skipped by blocked_by ; `Staging.submission` under-stamp ; ordering directory-avant-slot (lectures null bénignes).

### Mémoire (audit agent D1) — top consommateurs
- Windows host copies 4 MiB : **2,7-4,3 Go**, ~50 % gâchis de grille → `MESIA_WINDOW_KB=1024` ajouté à play.ps1.
- `MESIA_INPLACE` : stage 1 (windows) déjà actif ; **stage 2 (title buffers) interdit sous late_wait** (ranges déclarés inconnus pour buffers placés) — play.ps1 reste à 1 tant que LATE_WAIT≥1.
- Autres : snapshot POOL 512 MB const, `fetched` jamais réduit, `kept` épingle toute ressource vue, `COPIES=1023` conservés toute la run.

### Résultat du test `016dddb` (LATE_WAIT=2 + INPLACE=1 + -Fast)
- **Crash reproduit** : device removed ~23 s in-world, « 23 submissions sent ahead of 131 lists » — même signature qu'avant. Le gating `raw_uav_at` était nécessaire mais **pas suffisant** : un autre chemin d'écriture partagée sous flight existe. Dump conservé : `%TEMP%\crash_v5_1.txt` (479 pipelines dont 32 communs avec crash_2_relay).
- Input autonome résolu : `scripts/enter.ps1` (clic réel sur la fenêtre → focus → keybd_event Enter). Le sélecteur de profil se passe seul maintenant.
- Suspects suivants pour le même symptôme : (a) un second writer non-atomique sur état partagé (auditer tout `Create*View`/`copy_descriptors`/volatile >4B vers heap GPU-visible hors `defer`/`ready`), (b) la fenêtre résiduelle post-settle (soumission entre release du guard et enregistrement), (c) `strides`/staging races jugées « data-level » mais à revérifier côté DEVICE_HUNG réel, (d) épuisement/lifetime ressource à ~130 listes en vol (allocateurs, `late_most` listes=128 — coïncidence avec « 131 lists » ?).
- Config stable livrée inchangée : `LATE_WAIT=0` + `-Fast` (+`INPLACE=2` possible puisque sync) → ~500 ms in-world, 0 crash.
