# KELVAROQ-PHASE1-FINAL — clôture de la phase 1 (V5.6)

Baseline : `951e4e3`. Verdict par composant, mesures, décisions et rollback —
détail des runs dans KELVAROQ-PHASE1-RESULTS.md et MEMORY-OPTIMIZATION-RESULTS.md.

## Score final

| Composant | Provenance | État | Mesure | Décision |
|---|---|---|---|---|
| `plaincopy` | patch 0044, GPL-3.0 propre | porté ~450 LOC, `MESIA_COPY_PLAIN` défaut OFF | labo : **1,5-10× plus rapide** que le noyau (256 Mio/taille, bytes vérifiés) ; jeu : déclenche le résiduel latent (DEVICE_HUNG ~3 min) | **conservé dormant** — efficacité prouvée, stabilité subordonnée au résiduel |
| `dirty_ranges` | render crate, GPL-3.0 | non porté | `Card::dirty`+`joined()` déjà équivalent | **rejeté (redondant)** |
| `texel_rows` | render crate, GPL-3.0 | non porté | `Layout::walk` = même algorithme | **rejeté (redondant)** |
| `residency` | render crate, GPL-3.0 | non porté | mêmes constantes que `mapped_bytes` | **rejeté (redondant)** |
| `upload_pool` | render crate, GPL-3.0 | porté ~160 LOC, `MESIA_UPLOAD_POOL` défaut ON | classes quart-d'octave, limite 256 Mio ; recyclage à `reset()` | **intégré** — soak in-world en cours, 0 crash au-delà des fenêtres de crash |
| 5 sites XCL | GPL-2.0-only | exclus | — | **exclus permanents** tant que licence non vérifiée |

## Incidents et enseignements

- **3 DEVICE_HUNG** (`LATE_WAIT=2`, régime « 4-5 ahead of 10-11 lists ») :
  gating+plain ~5 min, gating seul ~35 s, **plain seul ~3 min**. Les deux
  features indépendantes déclenchent le même hang → **résiduel latent
  sensible au timing de préparation** (HANDOFF-ASTRA §V5.6). Rollback :
  gating reverté au call-site, plaincopy OFF par défaut.
- **Conséquence majeure** : toute optimisation qui accélère la préparation
  expose le résiduel. Le résoudre débloque simultanément plaincopy (10× sur
  les petites copies), le wait-GPU-hors-lock et les futures accélérations.
- `upload_pool` recycle après l'atterrissage des soumissions (`reset()`) —
  le seul point où la sécurité est déjà établie par le moteur ; les listes
  droppées sans reset libèrent à l'ancienne.

## Non couverts (reportés)

- Fixture `animation_transition` : spécifiée dans
  ANIMATION-TRANSITION-VALIDATION.md §2 (CB temps, crossfade A→B, texture
  historique, post-traitement — toutes avec oracle CPU byte-exact). Non
  implémentée cette vague.
- A/B plaincopy en jeu : bloqué par le résiduel (voir §3 PHASE1-RESULTS).

## Phase 2 candidates (ordre roadmap)

1. **Résiduel stage-3** (priorité absolue — bloque tout le reste) : bisects
   `MESIA_SWEEP=0`, `MESIA_LATE_WAIT_LISTS=64`, corrélation VA fautif DRED.
2. Async present borné (0046/0048) — design alternatif pour la classe du bug.
3. Wait GPU hors lock `submitted` (plan B2 écrit).
4. `card_back` send-tôt/wait-tard + merges u8→u64 (audits C1) — CPU pur.

## Rollback rapide

- Tout désactiver : `MESIA_UPLOAD_POOL=0` (uploads) ; `MESIA_COPY_PLAIN`
  absent (copies kernel). Revert code : `git revert 951e4e3` → `aa9be38`.
- Soak témoin de référence : binaire `951e4e3` >50 min in-world, P50=447 ms.
