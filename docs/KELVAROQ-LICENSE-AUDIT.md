# KELVAROQ-LICENSE-AUDIT — provenance et obligations

Audit documentaire du clone `kelvaroq-src` @ `1270c422` (v0.2.0). Rien d'exécuté.
MESIA est `GPL-3.0-or-later` (`Cargo.toml` racine) — compatible avec la licence
principale de Kelvaroq.

## Arbre de provenance

| Composant | Licence | Titulaire / source | Preuve |
|---|---|---|---|
| Kelvaroq (repo racine : launcher `src/`, `native/kelvaroq-render`, `native/patches`, `scripts/`) | **GPL-3.0-or-later** | © 2026 Stellarr Studio SAS | `LICENSE` (GPLv3 complet), `Cargo.toml:12`, README.md:276-278 |
| VIctory (moteur amont) | GPL-3.0-or-later | Marvisak et contributeurs | README.md:268 |
| victory-fork @ `f85101b` (base des 61 patches) | GPL-3.0-or-later | fxceless et contributeurs | README.md:269, documentation.md:5 |
| `research/` (référence CPU + outils) | **GPL-2.0-or-later** | Stellarr Studio — sous-licence montante justifiée par l'import Unicorn | `research/LICENSE` (GPLv2), README.md:279 |
| Unicorn (moteur CPU de `research/`) | **GPLv2 only** | unicorn-engine, v2.1.4 | README.md:215, 270 |
| XCL (référencé « Approach after ») | **GPL-2.0-only** | xcl-dev | 5 sites + `0049-xcl-fixes.patch` entier |
| Dépendances crates | Permissives | anyhow, clap, goblin, roxmltree, serde, serde_json, sha2, windows, windows-numerics, dunce | `Cargo.toml:22-63` |
| Contenu propriétaire | **Aucun trouvé** | — | grep : pas de XEX, clés, SDK, assets ; disclaimer README.md:285, :317 |

## Le point de friction : GPL-2.0-only (XCL)

GPL-2.0-**only** et GPL-3.0-or-later sont **incompatibles en copie littérale** :
on ne peut pas incorporer du code XCL dans MESIA (GPL-3). Les mentions
« Approach after XCL \<sha\> » suggèrent une réimplémentation clean-room chez
Kelvaroq (approche reprise, pas code repris) — hypothèse **probable mais non
vérifiée** : il faut diffuser chaque site contre le commit XCL cité avant toute
reprise littérale.

| Site | Marqueur | Action requise |
|---|---|---|
| `src/adapters.rs:10` | XCL 2dd04da — ordre adapters DXGI | diff vs xcl@2dd04da |
| `src/hardware.rs:31` | XCL 8ba374c — feature/wave query | diff vs xcl@8ba374c |
| `src/native.rs:27` | XCL b49be18 — résolution exe moteur | diff vs xcl@b49be18 |
| `src/launcher/Storage.cs:95-96` | XCL 50a9fde + 817328d — manifeste exe imbriqué | diff vs xcl@50a9fde, 817328d |
| `src/launcher/Archive.cs:186` | XCL 2d94699 — publish après vérif | diff vs xcl@2d94699 |
| `native/patches/0049-xcl-fixes.patch` | patch entier dédié XCL | audit complet avant portage |
| Mentions XCL dans 0050/0052/0053/0055/0057/0063 | éparses | marquer les blocs portés |

**Bonne nouvelle** : les cinq sites `src/` sont dans le launcher — hors scope des
portages perf. Le risque réel se concentre dans les patches flaggés XCL (0049, et
blocs épars de 0050-0063). Pour ceux-là : réimplémenter depuis le **concept** (décrit
dans la doc Kelvaroq, qui est GPL-3) plutôt que copier le diff.

À l'inverse, `research/` GPL-2.0-or-later est **sublicensable vers le haut** : on peut
reprendre ses outils sous GPL-3 en conservant la notice. On n'en a pas besoin pour
les portages GPU (c'est la référence CPU Unicorn).

## Obligations GPL §5 concrètes pour MESIA

Tout fichier MESIA dérivé d'un fichier Kelvaroq (GPL-3.0-or-later → licence
compatible, pas de changement de licence) doit :

1. **Rester sous GPL-3.0-or-later** — pas de recloisonnement.
2. **Porter un avis de dérivation** : ex. header `// Derived from Kelvaroq
   © 2026 Stellarr Studio SAS (GPL-3.0-or-later), modified for MESIA — <date>`.
3. **Marquer les modifications** — le git blame + le header daté suffisent.
4. **Publier le source** à la distribution — déjà notre modèle (repo public miroir).
5. **Conserver les notices existantes** du code repris (patches = fichiers modifiés
   de victory-fork : les headers VIctory/fxceless restent, on ajoute la nôtre).
6. **Ne jamais réutiliser le nom « Kelvaroq » ni le logo** : la GPL ne couvre pas la
   marque ; README.md:262 le réserve explicitement (« a fork must not use them, nor
   present itself as an official Kelvaroq release »). Nommer le portage par son
   mécanisme (`plaincopy`, `frame_limit`…), jamais « kelvaroq ».
7. Pour les blocs inspirés d'XCL : si le diff montre une ressemblance littérale,
   **réécrire** (GPL-2-only ne peut pas entrer dans MESIA GPL-3).
8. Pas d'obligation Copyleft sur les **idées** : les protocoles de mesure, les
   diagnostics (« ~⅔ de la frame = sync mémoire »), les designs (present borné par
   condvar, fingerprint 12 o) sont réimplémentables librement — c'est le code qui
   est licensié.

## Autres éléments juridiques constatés

- **Code 100 % généré par IA** (Moon AI, README.md:16, :266) : la titularité du
  copyright sur du code IA est une question ouverte ; Kelvaroq la revendique
  (© Stellarr Studio SAS). Traiter comme du code licensié normalement — le risque
  est celui de tout projet IA-only.
- **Notice légale UE** (README.md:289-343) : ils invoquent Dir. 2009/24/CE art. 6
  (décompilation pour interopérabilité), CJUE C-406/10, C-355/12, C-13/20, C-159/23.
  Contexte utile si MESIA documente un jour ses propres bases RE.
- **Aucun contenu contournant** : pas de clés, pas de déchiffrement, pas de assets
  (README.md:285) ; seuls des mécanismes d'interopérabilité — propre pour reprise.
- `build.json` du package liste les empreintes des patches appliqués — si nous
  reprenons des blocs, documenter la provenance commit-par-commit dans le fichier
  porté (exigence §5 + traçabilité interne).

## Verdict

Réutilisation **licite** de tout le code GPL-3.0-or-later de Kelvaroq dans MESIA,
sous §5 (attribution + marquage + publication + pas le nom). Zone de vigilance
unique : les blocs « Approach after XCL » → diff avant copie, sinon clean-room.
`research/` réutilisable aussi (GPL-2-or-later monte en GPL-3) mais inutile pour
les portages visés.
