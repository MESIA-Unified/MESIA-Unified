# HANDOFF-ASTRA — résiduel DEVICE_HUNG `MESIA_LATE_WAIT=3`

Baseline : `3a46661` (2026-11-26). Charge : `game_scarlett_master.exe`,
`INPLACE=1`, `WINDOW_KB=1024`, `WIDE_REFRESH/MERGE/BACK=1`, `ASK_ONCE` défaut.

## Fait établi

- `LATE_WAIT=0` synchrone : stable indéfiniment (~470-520 ms/frame).
- `LATE_WAIT=2` : stable >12 min in-world, 139 listes GPU en vol, P50=405 ms.
- `LATE_WAIT=3` : `device removed` à chaque run, 2-18 s après l'entrée du monde.
  Signature dump constante : "device removed after N submissions sent ahead
  of M lists" (M 70-131), listes en vol = compute+mesh pipelines, "refresh a
  buffer's host copy", "copy N bytes from title memory to upload memory".

## Différence stage 2 → 3

`late_free_present()` : la copie present est envoyée comme flight via
`execute_own` (host.rs:6507-6554), slot `p.ahead` réutilisé gaté par
`flown <= FINISHED` ; `queue::present` ne settle plus (`release_held(false)`
seul, queue.rs:783-787) ; `sweep()` peut se déclencher dans `execute_own`'s
submit (host.rs:4337-4342, cadence 1500 ms).

## Éliminé (audits vérifiés étanches — ne pas reprendre)

- Anneau `PLANE_VIEWS` (fix v2 `80e6410` : leases `dynamic` par liste).
- `mirror_shown` check→write gap (fix v3 `29bfe44` : re-check + re-defer).
- `memory::ready` `raw_uav_at` run partagé (fix v1 `016dddb` : settle_and_hold).
- Durée de vie `texture` du vol present (fix `3a46661` : `copy.keep(texture)`).
- `p.ahead` gating, `retire`/`bury` numbering, `in_the_air`/`in_air` gating,
  scratch/IDLE/SPARE/prelude/CARD_WORK pools, `detach`/`detach_parts`,
  dynamic-slot leasing, `staging.submission`/`in_air`, `line` accounting,
  `unmake`/`mirror`/`strides` bounds — audit exhaustif agent A3, tous sains.
- RTV/DSV rings (descripteurs CPU copiés par OMSetRenderTargets dans la liste).

## Suspects restants ordonnés (agent A3)

1. **`sweep()` dans `execute_own`'s submit** (host.rs:4337-4342) : seul
   drain+free exclusif au stage 3 ; `let_go_idle`→`retire` est refcounté mais
   le timing ~premier sweep éligible coincide avec la fenêtre de crash.
   Bisect : `MESIA_SWEEP=0` (si l'env existe — vérifier `now_ms`/`SWEPT` gate)
   ou instrumentation.
2. **`host.state` staleness des barrières present** (queue.rs:866 →
   host.rs:6525) : `state` lu avant `host.present`, `prelude()` échange
   `HostTexture.state` au submit — une soumission intercalée laisse la
   barrière enregistrée avec un `before` faux → UB D3D12. Fix proposé : capturer
   `host.state` dans `execute_own`'s submit sous `submitted`, ré-émettre les
   transitions là.
3. **`Step::Host` skip dans `blocked_by`** (host.rs:2238) : `draw.rs:2320`
   `expand_rects` lit la mémoire titre — pas de settle → lecture déchirée
   possible vs `line.written`. Plutôt corruption que hang, mais c'est la seule
   classe d'écrivain non gatée.
4. **Drop COM bypassant `retire`** : chemins `object::release_boxed` non
   audités — une ressource libérée qu'un vol enregistre encore = la classe de
   bug exacte.

## Instrumentation suggérée

- Corréler le VA fautif DRED (`%TEMP%\crash_v5_1.txt`) avec : scratch `Chunk.gpu`,
  `strides` VA, ou `HostTexture` VA — discrimine suspects 1/2/3.
- Compteur `in_the_air` flips + `slot_flight` tracking proposés par l'audit V5.2.
- `MESIA_LATE_WAIT_LISTS=64` sous stage 3 : si le crash recule, c'est
  profondeur-dépendant (ressource épuisée), pas timing.

## Fichiers clés

- `crates/mesia-gpu/src/host.rs` : submit 4304-4636, settle 4152-4298,
  execute_late 4026-4065, execute_own 4074-4101, present 6458-6568,
  sweep 4337-4342, blocked_by ~2228.
- `crates/mesia-gpu/src/queue.rs` : present 768-810, release_held 309.
- `crates/mesia-gpu/src/descriptor/host.rs` : mirror_shown ~1480-1640.
- `crates/mesia-gpu/src/memory.rs` : ready 244-338.
- Log : `%TEMP%\mesia.log` ; dumps : `%TEMP%\crash_v5_1.txt`.
