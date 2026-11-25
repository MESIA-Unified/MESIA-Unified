# Validation concurrence — O1 `MESIA_WIDE_MERGE` — audit Agent D

## Verdict initial : UNSAFE (corrigé dans `03c53a3`)

**Défaut trouvé** : `relay_merge_all` construisait les spans de groupage avec `shadow.start + from/to`, alors que `from`/`to` des items `work` sont **déjà des adresses titre** (sémantique confirmée par `find_all`/`inside`/`copy_back(address)`). Chaque span se trouvait décalé du `start` de son shadow → deux shadows couvrant les mêmes octets titre (fenêtre sur buffer, buffer imbriqué — cas légitimes : `lookup` choisit l'innermost, les fenêtres LOOSE ne soustraient pas les buffers enregistrés) pouvaient être dispatchés dans **deux jobs différents** → merges concurrents read-modify-write sur les mêmes bytes titre → perte silencieuse des écritures GPU d'un shadow.

**Correctif** (`03c53a3`) : spans = `(ptr, from, to)` non décalés — conservateur (sur-union possible, jamais sous-union).

## Confirmé sain

- **Ordonnancement** : `relay_land` — card_back → `relay_hand_back` (le join `wide()` garantit la fin des merges) → `copy_back_done` par vol → `Late::Write`/`Signal`. Identique au séquentiel ; les merges finissent avant tout bottom-of-pipe.
- **État partagé** : lock `before` par shadow sérialise les merges d'une copie et vs `merge_under_writes` ; scratch `now`/`title` par job ; compteurs atomiques ; `wide()` join par condvar.
- **`merge_groups`** : union-find correct (half-open adjacency, bridging transitif, ordre préservé par groupe) — couvert par test unitaire.
- **Pas de cycle de locks** : `before → GUARDS/REMAPPED` seul nouveau bord.

## Danger préexistant du même type (backlog)

`card_back` (host.rs:3568-3596) découpe les pièces en 8 groupes **sans union-find** — deux shadows « cardés » couvrant les mêmes octets titre ont la même race. À corriger en réutilisant `merge_groups`.

## Statut O1

`MESIA_WIDE_MERGE=1` : **PARTIELLEMENT VALIDÉ** — correction du groupement prouvée par audit + test ; bénéfice sous charge non mesuré (benchmark synthétique lourd à faire). Reste **désactivé par défaut**.
