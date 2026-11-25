# Cohérence mémoire & durée de vie — audit Agent B

Baseline `a06cfaa`. Périmètre : `crates/mesia-gpu/src/{shadow,watch,inplace,texture}.rs`, `draw.rs`, `mesia-hle/xmem.rs`.

## Cause racine confirmée — bug d'images périmées

`MESIA_UNWATCHED_FRAMES` n'agit qu'en **draw.rs:1381-1401** : pour un shadow `NOT_WATCHED`, `shadow.refresh` est sauté tant que `frame < last + every`. Avec `=12`, les buffers sur mémoire non write-watchée (private `XMemVirtualAlloc` sans `MEM_WRITE_WATCH`) affichent des données vieilles de jusqu'à ~12 frames → images/buffers rendus périmés. 

- N'affecte que le sens **titre→hôte** (jamais de corruption titre). N'affecte **pas** les textures (chemin `Staging::refresh` distinct) ni les shadows watchés.
- `watched=NOT_WATCHED` n'est **jamais ré-armé** (shadow.rs:2513-2525) : si le runtime remap plus tard en mémoire gardable, la copie reste aveugle.

### Correctifs proposés (par ordre de pertinence)

1. **Racine** : étendre `MEM_WRITE_WATCH` aux réservations non-placeholder dans `mesia-hle/src/modules/xmem.rs:129-137` (aujourd'hui limité à `constrained`) → plus de shadows `NOT_WATCHED`, refresh par page chaque frame, correct **et** moins cher.
2. **Chirurgical** : ré-armer `watched` quand `title_memory_remapped` (watch.rs:641) couvre le span de la copie.
3. **Immédiat** : laisser `MESIA_UNWATCHED_FRAMES` à 1 (défaut) — le réglage >1 échange la correction contre ~30 ms/frame.

## Durée de vie — constats

- Bien gardé : `Step::*` retient `Arc<Shadow>` jusqu'au reset ; `let_go_idle` exige `strong_count==1` + `retire` différé après vols (shadow.rs:1691-1722) ; `before` libéré seulement quand `pending` se vide.
- **Risque 1** : `unregister` (shadow.rs:2798-2800) droppe le shadow sans `retire` — si un slot descripteur conserve `shadow.buffer`, le nouveau shadow est rafraîchi mais le slot pointe l'ancien figé → slot périmé permanent.
- **Risque 2** : `Staging::refresh_timed` écrit le mapping upload pendant que le vol N-1 peut encore le lire (`blocked_by` ne couvre pas les buffers staging) → texels déchirés transitoires, auto-corrigé au refresh suivant.
- Race acceptée par design : merges titre font read-modify-write via kernel sans lock contre les écritures du titre (last-writer-wins par octet).

## Verrous (cartographie)

`Host::submitted` (sérialise refresh/snapshot/copy-back), `before` Mutex par shadow, `pending`/`flights`, `watch::JOURNAL`, `GUARDS`, `MAPPING` RwLock (remap exclusif vs refresh), `Staging.last`, `LOOSE`/`BUFFERS`.
