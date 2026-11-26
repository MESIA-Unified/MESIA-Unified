# MEMORY-OPTIMIZATION-RESULTS — V5.6, mesures copies et pools

Baseline : `951e4e3`. Matériel : RTX 3060 12 Go, 16 Go RAM, Windows x64.

## 1. Benchmark indépendant `plaincopy` (A1)

Test `plaincopy::x64::tests::plain_vs_kernel_copy_rates` (`--ignored`) :
256 Mio déplacés par taille, buffers appartenant au processus de test, chaque
octet vérifié, 100 % fast path, source désalignée (pire cas pour `rep movsb`).

| taille | plain ns | kernel ns | speedup |
|---|---:|---:|---:|
| 256 o | 41 | 265 | **6,5×** |
| 4 Kio | 74 | 752 | **10,2×** |
| 16 Kio | 123 | 1 007 | **8,2×** |
| 64 Kio | 1 130 | 3 213 | 2,8× |
| 256 Kio | 5 735 | 14 010 | 2,4× |
| 1 Mio | 51 347 | 77 698 | 1,5× |
| 4 Mio | 296 809 | 1 040 120 | **3,5×** |

Lecture : le surcoût du passage noyau domine sous ~64 Kio — précisément là où
vivent les ~17 000 pièces copiées par frame. Au-delà, la bande passante
mémoire borne les deux chemins. **Gain microarchitectural réel et mesuré** ;
le gain en jeu est subordonné à la résolution du résiduel (voir §3).

## 2. Drop-ins render Kelvaroq — verdict comparatif (A2-A4)

| Composant | Verdict | Raison |
|---|---|---|
| `dirty_ranges` | **REDONDANT** | `Card::dirty` + `joined()` (shadow.rs:336) fusionne déjà au drain sans perdre un octet ; la saturation bornée n'aide qu'en fragmentation pathologique (~500 plages/frame mesurées) |
| `texel_rows` | **REDONDANT** | `Layout::walk` (mesia-formats/texture.rs:617) implémente le même partage de runs + spécialisation `copy_runs::<8/16/32>` |
| `residency` | **REDONDANT** | mêmes algorithme et constantes que `mapped_bytes`/`again` (lignée commune) |
| `upload_pool` | **PORTÉ** | implémente le TODO littéral de `HostList::upload` ; classes quart-d'octave vs match exact de `Spare` |

## 3. `plaincopy` — décision de conservation

- **Microbenchmark : nettement plus rapide** (tableau §1), conformité des
  données vérifiée, 4 tests unitaires + 1 benchmark verts.
- **En jeu : déclencheur du résiduel latent** — le run `COPY_PLAIN=1` seul
  a produit `DEVICE_HUNG` ~3 min in-world (« 5 of 11 », identique aux crashs
  gating). Table 2×2 dans HANDOFF-ASTRA.md : deux features indépendantes
  (lecture différée + lecture rapide) déclenchent le même hang → bug latent
  sensible au timing de préparation.
- **Décision : conservé dormant, `MESIA_COPY_PLAIN` défaut OFF.** Ne pas
  réactiver en jeu tant que le résiduel n'est pas résolu — son efficacité est
  déjà prouvée en laboratoire, il ne manque que la stabilité.

## 4. `upload_pool` — porté, test in-world en cours

- `upload_class` (quart-d'octave ≥64 Kio, ≤+25 % de surcapacité) + `Pool<T>`
  borné à 256 Mio, portés dans `host.rs` près de `Spare` (attribution
  GPL-3.0 dans le code). `MESIA_UPLOAD_POOL=0` revient à l'ancien chemin.
- `HostList::upload` et le gros `scratch()` (>4 Mio) allouent à la classe et
  prennent dans le pool ; `Deferred::reset()` (après exécution des soumissions
  — même point sûr que les chunks) rend les buffers au pool. Les listes
  droppées sans reset libèrent comme avant (sécurité).
- Compteurs `ALIVE` 8→11 : uploads poolés (count/bytes) + réutilisés depuis
  le départ ; ligne mémoire copystat étendue.
- **Critère de mesure** : `MADE[0]` (upload buffers créés) doit chuter — en
  run 60 : 42 607 buffers / 77,9 Go créés en 5 min de streaming.
- 3 tests Kelvaroq portés + suite : **135 verts**.

## 5. État mémoire (soak témoin `951e4e3`, ~50 min in-world)

- RSS : 2,4-4,7 Go, **en baisse** après warm-up (pas de fuite) ; 0 crash.
- Frame : P50=447 ms, P95=536 ms sur les 200 dernières — records de la
  campagne. copies ~43 ms/126 Mo (scène streaming).
