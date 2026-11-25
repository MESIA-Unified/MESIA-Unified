# Session autonome — notes de travail (2026-11-25)

## État live
- Run courant : mode 2 (défaut) + `MESIA_D3D12_DRED=1 MESIA_GPU_TIMING=1`, binaire avec fix échange-pair — en gameplay (prompt L/R affiché), veille crash.
- 5 TDR `nvlddmkm` ID 153 : 11:22, 12:04, 12:37, 12:44 UTC. Runs longs (60+ min) sans crash → **stochastique**.

## TDR — mécanisme prouvé (2026-11-25 soir)
- **A/B GROUP_SYNC** : `=1` → corruption visible (cheveux/persos = bouillie magenta, échanges LDS racy). `=2` (défaut) → rendu propre. Donc des shaders avec `s_barrier` en région divergente **existent** ; le carve-out `fence_only` est utilisable en mode 1 mais casse la synchro dont ces barrières ont besoin quand tous les threads arrivent quand même.
- **Conséquence** : en mode 2, une divergence runtime qui échoue à atteindre la barrière (sortie anticipée, ou LOOP_LIMIT `abort`→`Ret` pour les boucles `Jump`-latch — lower.rs:10624) fige le groupe → hang → TDR. Stochastique ✓.
- **Audit lower.rs (agent 1f171d33)** — suspects classés : #1 s_barrier groupe-wait en bloc divergent (défaut `_ => true` wait) ; #2 LOOP_LIMIT abort→Ret échoue les barrières meeting/exchange post-boucle (fix = router abort vers le bloc de sortie `out` du header, pas Ret — à faire, ~30 lignes, voir lower.rs:10750) ; #3 dword `hi` non initialisé (CORRIGÉ, commit 447b6fa) ; #4 shortcut `lds_meetings` two-writes ; #5 cmpxchg retry hors LOOP_LIMIT ; #6 `widen()` activé au retry LANE_MASK malgré historique de perte carte (vérifier `MESIA_WAVE_WIDE=0`).
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
