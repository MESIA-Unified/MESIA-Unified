# MESIA Unified

[![Licence : CC-BY-4.0](https://img.shields.io/badge/license-CC--BY--4.0-blue)](LICENSE)
[![Statut : recherche expérimentale](https://img.shields.io/badge/status-experimental%20research-orange)](ROADMAP.md)
[![Tests : 100 vérifications automatisées (labo privé)](https://img.shields.io/badge/tests-100%20automated%20checks%20%28private%20lab%29-green)](docs/DEVELOPMENT_STATUS.md)

**Un moteur de compatibilité expérimental pour les binaires Xbox Series sous
Windows**, écrit en Rust au-dessus de DirectX 12, avec traduction de shaders
GFX10→DXIL. MESIA Unified est un projet de recherche personnel, mené sur mon
temps libre, qui étudie comment les exécutables Xbox Series pourraient être
chargés, leurs flux de commandes GPU interprétés et leurs shaders recompilés
pour le matériel PC — dans un moteur unique et unifié.

> **Pourquoi ce projet ?** Honnêtement — pour le plaisir. Je suis développeur
> systèmes/graphique avec plus de cinq ans de bas niveau derrière moi, et un
> week-end je me suis demandé si du travail GPU de niveau console pouvait
> tourner sur une simple pile D3D12 de PC. Ce qui a commencé comme une petite
> expérience « charger un exécutable bizarre » est devenu du mappage PE, puis
> un problème de discipline de fences, puis un traducteur de shaders, puis un
> moteur de flux de commandes — le genre de projet qu'on commence « pour un
> week-end » et pour lequel on débogue encore des barrières des mois plus tard.
> C'est un terrain de jeu pour problèmes GPU difficiles, partagé publiquement
> au cas où le voyage aiderait — ou amuserait — quelqu'un d'autre.

> **Ce dépôt ne publie que de la documentation.** Le code source du moteur est
> développé dans un laboratoire privé et n'est pas encore publié — une analyse
> de compatibilité des licences de ses composants amont est en cours. Voir la
> [FAQ](docs/FAQ.md) et la section [statut du code source](#statut-du-code-source)
> ci-dessous.

**English:** see [README.md](README.md).

---

## Ce que c'est — et ce que ce n'est pas

MESIA Unified est une **infrastructure de recherche**, pas un produit ni un
émulateur téléchargeable permettant de jouer. Tout ce qui a été vérifié à ce jour
l'a été sur des **fixtures synthétiques assemblées à la main dans le laboratoire —
pas sur de vrais jeux**.

- **Ambition à long terme :** exécuter *GTA VI* (build Xbox Series) sur PC —
  la mission lunaire qui rend le projet amusant. Ambition déclarée, rien de
  plus : rien ici n'exécute, ne démarre ni n'affiche ce titre ni aucun autre
  jeu réel, et ce n'est peut-être jamais le cas. L'intérêt, c'est tout ce
  qu'on apprend en chemin.
- **Aujourd'hui :** un squelette de moteur unifié comprenant un chargeur PE, un
  backend D3D12 avec mémoire GPU budgétée, un cache de pipelines persistant, un
  service de traduction de shaders GFX10→DXIL et une couche de
  décodage/filtrage/exécution des commandes invité — chaque étape vérifiée
  individuellement sur les fixtures du laboratoire.
- **Ce n'est pas :** un crack, un portage ni une redistribution. Il n'y a aucun
  fichier de jeu, aucun contenu propriétaire et aucun contournement de DRM dans
  ce projet. Voir [Statut légal et avertissement](#statut-légal-et-avertissement).

## Le moteur en bref

Six crates, un moteur — détails dans
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) :

| Crate | Rôle |
|---|---|
| `mesia-loader` | Chargement PE : IR `PreparedImage` partagée, mappeur manuel (VirtualAlloc + relocalisations + IAT + TLS), résolveurs d'imports interchangeables, cycle de vie `LoadedImage` |
| `mesia-mem` | Arènes mémoire hôte, `FencePool` — résidency budgétée + LRU arbitrée par les époques de fence GPU |
| `mesia-core` | Configuration unifiée, harnais de comparaison PE `PeFacts` inter-moteurs |
| `mesia-gpu` | L'unique backend D3D12 : device/queue/époques de fence, `GpuAllocator` (`Evict`/`MakeResident` réels), heaps de descripteurs avec slot de garde nul, `PipelineLibrary` dédupliquée avec cache persistant `D3D12_CACHED_PIPELINE_STATE` + journal hors-ligne |
| `mesia-shader` | Traducteur GFX10→DXIL, analyse des conteneurs invité (DXBC/DXIL/XDAT/XBCH/FSSM + signatures `ISG1`/`OSG1`), abaissement wave64→largeur hôte, types de sortie neutres `TranslatedShader` |
| `mesia-command` | Couche de commandes invité : décodeur du flux laboratoire `MESC` → IR typée `GuestCommand` → `Recording` + `IndirectGuard` → exécuteur émettant sur les surfaces D3D12 partagées |

## Où en est le projet

Six phases d'intégration achevées dans le laboratoire privé, **100 tests
automatisés au vert, 0 avertissement** sur le matériel du laboratoire (NVIDIA
RTX 3060 12 Go, Windows 11, 16 Go de RAM). Le texte complet des jalons est dans
[ROADMAP.md](ROADMAP.md) et
[docs/DEVELOPMENT_STATUS.md](docs/DEVELOPMENT_STATUS.md). Points marquants :

- **Chargeur PE** — un pipeline d'image unifié prouvé en parité octet à octet
  avec les chargeurs des deux moteurs amont ; callbacks TLS et points d'entrée
  `DllMain`/EXE exécutés de bout en bout sur fixtures.
- **Backend D3D12** — device unique sur RTX 3060, libérations différées par
  époques de fence, résidency réelle `Evict`/`MakeResident` sous budget strict,
  heaps de descripteurs durcis (slot de garde nul permanent + remplissage nul).
- **Cache PSO persistant** — blobs `D3D12_CACHED_PIPELINE_STATE` par PSO,
  vérifiés par hachage avant que le pilote ne les voie, plus un journal
  hors-ligne qui reconstruit un cache chaud sans réexécuter la charge.
- **Traduction GFX10→DXIL** — dispatch compute vérifié par readback, dont un
  OR-reduce cross-lane wave64 correctement abaissé sur un hôte 32 lanes ; une
  paire VS+PS traduite a rendu un triangle pixel-exact (2016/4096 texels).
- **Traduction de commandes** — flux laboratoire `MESC` → décodage → IR → garde
  indirecte → exécution : draws, dispatchs et arguments indirects produits par
  le GPU tous vérifiés ; la règle de neutralisation indirecte U-16 est
  GPU-vérifiée.

*Tous les résultats ci-dessus ont été vérifiés sur des fixtures synthétiques,
pas sur de vrais jeux.*

## Comment c'est construit : développement multi-agents

Les phases 5 et 6 ont été construites par des **équipes de sous-agents isolés** :
chaque agent travaillait dans son propre worktree git sur la base d'un **contrat
d'interface gelé** publié avant le début des travaux, et un orchestrateur a
intégré les résultats dans un ordre fixe — en touchant lui-même les fichiers
partagés. Phase 5 : quatre worktrees, zéro conflit de fichiers. Phase 6 : même
modèle, tests GPU sérialisés par l'orchestrateur. Les contrats d'interface sont
conservés comme artefacts du laboratoire — une méthodologie notable pour quiconque
coordonne du travail parallèle sur un moteur Rust fortement couplé.

## Statut du code source

Le code source du moteur reste **privé** dans l'attente d'une analyse de
compatibilité des licences des composants tiers qu'il consomme — résumons :
« analyse de licence en cours » ; le laboratoire ne distribuera ni builds ni code
tant qu'elle n'est pas résolue. Ce dépôt publie entre-temps la documentation,
les rapports d'état et la feuille de route du projet sous
[CC-BY-4.0](LICENSE).

## Rejoindre l'équipe de développement MESIA Unified

Le projet est petit, de niveau recherche, et animé par des collaborateurs. Nous
recherchons des contributeurs ayant de l'expérience — ou un intérêt sérieux —
dans :

- **Rust** système (discipline de l'unsafe, conception de workspaces)
- **DirectX 12** (command lists, root signatures, résidency, `ExecuteIndirect`)
- **Traduction de shaders GFX10 / DXIL** (décodage ISA, abaissement de
  wave-size, SM 6.x)
- **Gestion mémoire GPU** (budgets, éviction, suivi de durée de vie par fences)
- **Couches de compatibilité** (chargement PE, HLE/binding d'imports, IR de
  commandes)
- **Mesure de performance** et **ingénierie de tests** sur matériel réel
- **Documentation** (le dépôt que vous lisez)

Pour postuler, lisez [CONTRIBUTING.md](CONTRIBUTING.md) et ouvrez un ticket avec
le modèle **collaborator application** (`.github/ISSUE_TEMPLATE/`), ou partez du
[gestionnaire de tickets](https://github.com/MESIA-Unified/MESIA-Unified/issues).

## Documentation

- [ROADMAP.md](ROADMAP.md) — ce qui est fait, en exploration, planifié
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — schéma des crates, pipelines de commandes et de shaders, règles de sécurité
- [docs/DEVELOPMENT_STATUS.md](docs/DEVELOPMENT_STATUS.md) — registre par phase : objectifs, résultats, limites
- [docs/FAQ.md](docs/FAQ.md) — les réponses honnêtes d'abord

## Statut légal et avertissement

MESIA Unified est un **projet de recherche personnel non officiel**, développé
pour le plaisir sur mon temps libre. Il n'est
ni affilié à, ni sponsorisé ni approuvé par Rockstar Games, Take-Two
Interactive, Microsoft ou toute autre société. « Xbox », « Xbox Series »,
« GTA » et « GTA VI » sont des marques de leurs propriétaires respectifs ; les
références servent uniquement à identifier les objectifs de recherche.

Le projet **n'est ni un crack, ni un portage, ni un contournement** : il ne
contient aucun code de jeu, aucun asset de jeu, aucun firmware de console ni
élément de SDK, et n'implémente aucun contournement de DRM. Tout le
développement et toute la vérification utilisent des fixtures synthétiques
assemblées en salle blanche (clean-room). Exécuter un vrai titre exigerait les
propres fichiers légalement obtenus de l'utilisateur — et aucun titre réel n'est
actuellement pris en charge.

La documentation de ce dépôt est publiée sous licence
[Creative Commons Attribution 4.0 International (CC-BY-4.0)](LICENSE).
