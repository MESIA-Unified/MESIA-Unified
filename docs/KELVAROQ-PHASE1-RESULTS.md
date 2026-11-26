# KELVAROQ-PHASE1-RESULTS — intégration phase 1, mesures et décisions

Baseline : `aa9be38` (audit V5.4) — soak témoin `LATE_WAIT=2` >11 h, 0 crash.
Cette phase n'intègre rien d'activé par défaut : tout est derrière des gates
ou dormant, et **deux crashes DEVICE_HUNG** ont mené à un revert documenté
ci-dessous. Stabilité avant tout.

## 1. Provenance (phase 1A)

| Composant | Source | Licence | Statut |
|---|---|---|---|
| `plaincopy` | `0044-copy-plain.patch` → `crates/mesia-gpu/src/plaincopy.rs` (~450 LOC portés) | GPL-3.0-or-later (Kelvaroq) — compatible | **Vérifié propre** : aucun flag XCL dans `kelvaroq-render/src` ni le patch ; attribution à conserver |
| Drop-ins render (`dirty_ranges`, `texel_rows`, `upload_pool`) | `native/kelvaroq-render/src/*.rs` | GPL-3.0-or-later | Vérifié propre, **non encore intégrés** (nos `dirty`/`rows` existent déjà — gain faible, reporté) |
| 5 sites XCL | flags GPL-2.0-only | Incompatible confirmé | **Exclus** — aucun code XCL importé |

## 2. `plaincopy` — porté, dormant, disculpé d'un crash, gain non prouvé

- `MESIA_COPY_PLAIN=1` active une copie in-process `rep movsb` à la place de
  `ReadProcessMemory` pour les lectures de mémoire titre ≥256 o. VEH ne
  revendique que les fautes de **lecture** sur la propre instruction (écritures
  et `GUARD_PAGE` passent aux handlers suivants — watch conserve ses gardes) ;
  bloc 64 Kio fautif → liste noire 2 s → repli noyau ; <256 o → noyau.
- 4 tests unitaires verts (page manquante, garde Windows, cooling, seuil).
- **Crash A** (15:50:03) : `DEVICE_HUNG 0x887A0006`, « 5 submissions ahead of
  11 lists », ~4-5 min in-world, `COPY_PLAIN=1` **et** gating `Step::Host`
  actifs. → plaincopy alors suspect.
- **Crash B** (16:00:35) : même `DEVICE_HUNG`, « 4 ahead of 10 », ~35 s
  in-world, `COPY_PLAIN` **absent**, gating actif. → **plaincopy disculpé**,
  le gating est le seul changement actif commun.
- Verdict : module conservé (tests + gate), **désactivé par défaut**. Gain
  perf non mesuré de façon propre — le seul run activé (P50=485 ms incluant
  le warm-up, copies ~86-100 ms/~100 Mo) ne permet pas de conclure vs la
  baseline (P50=405 ms, copies 93,7 ms/201 Mo — scènes non isométriques).
  A/B propre à refaire sur le binaire stabilisé.

## 3. `defer_host_read` / gating `Step::Host` — implémenté puis reverté

- WIP préexistant (call-site `draw.rs:2320` posé sans la méthode) : lire les
  vertices d'expansion de rects comme plage déclarée, gatée vs `line().written`
  par `blocked_by` (fix recommandé par l'audit A3, suspect #1 « Step::Host non
  gaté »).
- Implémentation : `Step::Host(Range<usize>, …)`, `defer_host` → `0..0`
  (jamais bloquant), `LATE_STATS` 18→19, `step_text` mis à jour. Compile
  propre, 132 tests verts.
- **2 crashes DEVICE_HUNG** (fenêtres 35 s et ~5 min) avec le gating actif,
  vs >11 h stables sans. **Revert du call-site** (`draw.rs` → `defer_host`) :
  la plomberie reste compilée mais dormante (`0..0` ne touche rien).
- Hypothèse de mécanisme : la plage déclarée (`from..from+n*3*stride` dans un
  vertex buffer titre) peut recouvrir des plages que nos propres lignes en vol
  écrivent (write-back/echo) → attente sur une ligne de la **même soumission
  non encore exécutée** → auto-dépendance → liste bloquée → « sent ahead » →
  DEVICE_HUNG remonté à la fence suivante. Réactivation future = gater
  uniquement sur les écrits des **autres** soumissions en vol, jamais les
  lignes intra-liste.
- Consigné aussi dans HANDOFF-ASTRA.md (le suspect « Step::Host non gaté »
  devient « gating naïf = crash ; causalité à reprendre »).

## 4. État mesuré du run témoin (binaire reverté, plaincopy dormant)

- In-world >10 min, **0 device removed**, ~441-518 ms/frame en steady,
  rendu cohérent (capture : personnage + scène extérieure, 4 160 draws/frame).
- Soak long en cours pour confirmation >1 h.

## 5. Décisions

| Élément | Décision | Rollback |
|---|---|---|
| `plaincopy` | Conservé, `MESIA_COPY_PLAIN` défaut OFF | `unset` l'env |
| `defer_host_read` gating | **Reverté au call-site**, plomberie dormante conservée | `defer_host` (fait) |
| Drop-ins render | Non intégrés (doublons `dirty`/`rows` existants) | n/a |
| `LATE_STATS` 19e compteur | Conservé (inerte hors gating) | n/a |

## 6. Suite (phase 2 candidates, non engagées)

1. A/B propre `MESIA_COPY_PLAIN` sur le binaire stabilisé (même scène, ≥10 min).
2. Re-gating `Step::Host` correct (vols externes seulement) — avec Astra.
3. `async present borné` (P2 roadmap) — design alternatif pour stage-3.
4. `card_back` send-tôt/wait-tard + merges u8→u64 (audits C1) — sans rapport
   Kelvaroq, douleur mesurée.
