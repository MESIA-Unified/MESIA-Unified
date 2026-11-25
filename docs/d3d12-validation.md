# Validation DirectX 12 — audit Agent C

Baseline `a06cfaa`. Périmètre : `mesia-gpu/{pipeline,host,descriptor,device,resource,queue,list}.rs`, `mesia-pipeline`.

## Verdicts

| Zone | Statut | Détail |
|---|---|---|
| Durée de vie allocators/fences (RELAY=2) | ✅ VALIDÉ | Vols atterrissent par préfixe de fence (`landable`, host.rs:1810) ; `reset()` uniquement après `GetCompletedValue` ; `Detached` retient list+upload+State jusqu'à l'atterrissage ; `FINISHED` ne progresse que dans settle/relay_land. Aucun trou de réutilisation trouvé. |
| Barrières shadow copies / readbacks | ✅ VALIDÉ (2 remarques) | `prelude()` enregistre real→first sous le verrou `submitted` ; readback = buffer frais par appel ; copies `Through` = COMMON→COPY_DEST→COPY_SOURCE→COMMON. |
| E_INVALIDARG pipelines compute | ⚠️ PARTIEL | `warn host pipeline refused` (pipeline/host.rs:524) ne capture ni le DXIL ni si l'échec vient de `LoadComputePipeline` (avalée dans `trail`) ou `Create*PipelineState`. `MESIA_SHADER_SAVE=<dir>` existe (pipeline.rs:139) et écrit `{hash}.{stage}.dxil` — à armer avant le run. **Fix minimal** : dump auto du DXIL refusé + distinguer Load/Create dans le warn. (Doc : `MESIA_SHADER_DUMP` n'existe pas — coquille corrigée.) |
| Heap 1M vues NVIDIA | ⚠️ CONFIRMÉ cause visuelle | `PORTABLE_VIEWS=1M` → cap : `[0,737855)` slots mirroirs, `[737855,999999)` dynamiques, 999999=null. Indices titre ≥737855 hors clamp → lisent la région dynamique → **texture fausse** (pire que noir). ≥1M → null → noir. Confirmé par warn `title slot past the host heap`. Fix : second heap + sélection par plage dans `heap_handle` (mesia-shader). |
| TOCTOU `host_transition` | ⚠️ MOYEN — backlog | queue.rs:829-832 fait `state.swap` **hors** du verrou `submitted` → une `prelude()` concurrente peut enregistrer une barrière avec mauvais `StateBefore` sous RELAY=2. Fix : swap/restore à l'intérieur de `submit` sous le verrou. |
| Drift d'état `card_before` | ⚠️ MINEUR | host.rs:3070 laisse le buffer en NON_PIXEL_SHADER_RESOURCE après la passe hash ; transition retour vers COPY_DEST à ajouter. |
| Descriptor slot écrasé en vol | 📌 architectural | `mirror`/`keep` réécrivent des slots que les vols lisent — `retire` garde la ressource vivante mais le contenu peut déchirer. Inhérent au mirroring. |

## Actions backlog

1. (P1) Second heap de vues + sélection par plage → textures >737855 correctes.
2. (P1) `host_transition` sous verrou `submitted`.
3. (P2) Dump DXIL refusé + Load-vs-Create dans le warn.
4. (P2) Restore COPY_DEST après `card_before` hash.
