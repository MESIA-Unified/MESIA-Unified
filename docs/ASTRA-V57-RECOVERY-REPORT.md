# Astra V5.7 — rapport de récupération (Devin, 2026-11-26)

## État récupéré

Session Codex interrompue par quota. Travail trouvé **uniquement dans `mesia-main`,
non commité**. Aucune branche `campaign/v5.7` ni commits `a7e3372`/`ace72fb`
n'existent localement ni sur `origin` (vérifié `git ls-remote`, `git log --all`,
kelvaroq-src). Les rapports des sous-agents `/root/lease` et `/root/phase_two`
n'ont pas été retrouvés sous forme de fichiers ; leur contenu est inféré des
diffs seulement.

Sauvegarde Astra : `.restore-v57-20261126-205100/` = copies **pré-changement**
des 4 fichiers touchés + snapshot `integration/` (état intermédiaire, avant la
toute dernière retouche de `host.rs`).

## Modifications identifiées (toutes appliquées, toutes vérifiées)

### fences possédées par les opérations différées — le fix architectural

Avant : `Late::Signal(usize, u64)` (host.rs) et `Held::Wait/Signal(usize, u64)`
(queue.rs) stockaient l'adresse brute de la fence COM. Si l'appelant relâchait
sa référence avant que l'opération différée ne soit consommée, l'objet pouvait
être libéré en vol → use-after-free sur l'objet COM.

Après :

- `fence::FenceRef` — `add_ref` au `retain`, `release` au `Drop`, `Send`
  (fence.rs:40-69).
- `Late::Signal(FenceRef, u64)` — le signal différé garde la fence vivante
  jusqu'à l'atterrissage du vol (host.rs:2312, 4052, 4521).
- `Held::Wait(FenceRef, u64)` / `Held::Signal(FenceRef, u64)` — l'attente et le
  signal différés de queue retiennent la fence jusqu'à consommation ou
  annulation (queue.rs:47-55, 326-327, 409-411, 619-630, 663).
- `late_signal` déclarée `unsafe` avec exigence `// SAFETY: caller holds the
  fence` (host.rs:2520-2531).

### Instrumentation (la retouche « +4/-4 » interrompue — complétée proprement)

Le `Instant::now()` inconditionnel `called` est remplacé par `acquiring` +
`lock` sous `#[cfg(feature = "diagnostics")]` (host.rs:4552-4556). La mesure du
verrou de soumission ne paie plus le timestamp en build non-diagnostics.
Diff vérifié ligne à ligne : aucune variable orpheline, `started` encore
utilisé, attributs cfg corrects.

### Benchmark plaincopy renforcé

`plain_vs_kernel_copy_rates` : chaque chemin part de l'inverse des octets
attendus et est validé avant que l'autre chemin n'écrase — la conformité des
données est vérifiée **entre** les mesures, pas seulement après (plaincopy.rs).

## Tests réellement exécutés (Devin, ce jour)

| Test | Résultat |
|---|---|
| `cargo check -p mesia-gpu` | propre |
| `fence::deferred_fence_survives_caller_release_and_balances_refs` | **ok** |
| `fence::cancelling_a_deferred_signal_releases_its_reference_without_signalling` | **ok** |
| `queue::deferred_queue_operations_retain_both_fences_until_the_wait_passes` | **ok** |
| Suite lib complète | **138 passés, 0 échoué, 7 ignorés** (benchmarks marqués) |

Le test queue vérifie les **deux** fences : refs comptées à 3 (appelant +
observateur + Held), la cible complète à 7 après le signal de la gate, tous les
objets relâchés à 0 à la fin — durée de vie conforme aux exigences D3D12.

## Corrections conservées / annulées

**Tout conservé.** Aucune annulation nécessaire : le design ne fait qu'allonger
des durées de vie (jamais raccourcir), la compilation est propre, les tests de
durée de vie passent, aucune instrumentation orpheline.

## Hypothèse de lien avec le DEVICE_HUNG timing-sensible

Le résiduel V5.5/V5.6 était déclenché indifféremment par le gating `Step::Host`
OU `MESIA_COPY_PLAIN=1` — deux chemins qui accélèrent la préparation. Une fence
COM libérée prématurément est exactement le type de défaut qu'un timing CPU
plus rapide expose (la fenêtre « appelant relâche → opération consomme » se
décale).

**Validation repro (ce jour)** : run `MESIA_COPY_PLAIN=1` sous le binaire
`f8c897c` — **0 DEVICE_HUNG en >12 min in-world** au moment de l'écriture
(warm ~600-900 ms puis steady 426-475 ms, copies 90-122 ms/frame), contre le
crash systématique à ~3 min sous le binaire précédent (fenêtre ×4 dépassée,
run laissé actif). **Preuve forte** que l'UAF de fence différée était (au
moins un contributeur majeur du) résiduel timing-sensible. À confirmer par un
soak long et le second déclencheur (`defer_host_read`, réservé à une campagne
dédiée).

## Commits

- `f8c897c` — récupération du code Astra (4 fichiers).
- `4bf43f1` — synchronisation docs dérivés (STATUS.md, audits O1/D3D12,
  test-inventory, `wer.ps1`).

## Risques restants

- Le fix couvre la classe « fence différée » ; d'autres COM droppés hors
  `retire` restent listés dans HANDOFF-ASTRA.
- `Plaincopy` et le gating `Step::Host` restent dormants ; leur réactivation
  est conditionnée à la validation in-world en cours.

## Rollback

`git revert f8c897c` restaure les `usize` bruts — non recommandé (réintroduit
l'UAF). Le `.restore-v57-*` contient les versions pré-changement des 4 fichiers.
