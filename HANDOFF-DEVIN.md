# Passation MESIA Unified V5.2 à Devin — 26 novembre 2026

## Point de reprise

Le jalon graphique est validé sur la RTX 3060. **La prochaine mesure porte sur les coûts du moteur dans un replay représentatif autorisé.** Le rectangle animé sert de contrôle de non-régression. Il traverse le traducteur VS/PS et une file D3D12 dédiée. Les chemins de copies shadow, de descripteurs du titre et de `declared_writes` nécessitent une charge moteur distincte. Aucune accélération de ces chemins n’a été intégrée. Les paliers 2 à 4 FPS sur cette charge restent ouverts.

## Ouvrir ou restaurer le bon état

- Sur la machine de développement, ouvrir `C:\Users\utilisateur\Desktop\mesia-main`. Ce dossier est la source de vérité locale. Le miroir voisin `mesia-upload` est propre à `293b8a493fc1285f23b832ffb8de6d33834c2a8f`. Ce commit **précède** les changements de campagne listés ci-dessous.
- Sur une autre machine, extraire `MESIA-V5.2-Devin-20261126.zip` dans un dossier vide et ouvrir le sous-dossier `mesia-main`. Le ZIP contient les sources et la documentation complètes ainsi qu’un manifeste SHA-256 de tous ces fichiers. Les binaires Cargo et caches restent sur la machine d’origine. Lire [AGENTS.md](AGENTS.md), [PLANS.md](PLANS.md) et [les mesures](docs/perf-study.md) avant de modifier le moteur.
- Pour vérifier les octets restaurés sous PowerShell, depuis le dossier contenant `MANIFEST-SHA256.csv` et `mesia-main`, exécuter le bloc ci-dessous. Pour intégrer ensuite sous Git, travailler dans une branche ou une copie isolée issue de la baseline. Le miroir utilise `docs/ARCHITECTURE.md`, le snapshot `docs/architecture.md`. Réconcilier la casse sur un système sensible à la casse avant de comparer ou intégrer.

```powershell
$ErrorActionPreference = 'Stop'
$entries = Import-Csv -LiteralPath .\MANIFEST-SHA256.csv
foreach ($entry in $entries) {
    $path = Join-Path .\mesia-main $entry.Path
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $entry.SHA256) {
        throw "Fichier divergent : $($entry.Path)"
    }
}
"$($entries.Count) fichiers vérifiés"
```

## Inventaire de la campagne depuis le miroir

| Fichier | Travail livré |
|---|---|
| `crates/mesia-gpu/src/host.rs` | Sol a vérifié le HRESULT de `Present`. Astra a regroupé la décision dans `Meter::record_present` et testé l’échec, l’occlusion et le succès. `queue::FRAMES` compte toujours les requêtes du titre. |
| `crates/mesia-shader/src/emit/mod.rs` | Fixture compute GFX10 originale, passage par `emit::translate`, huit valeurs dynamiques comparées au readback. |
| `crates/mesia-shader/tests/graphics/stages.rs` | Construction et traduction des deux conteneurs GFX10 originaux avec validation DXIL. |
| `crates/mesia-shader/tests/graphics/native.rs` | Paire HLSL PC de référence compilée par DXC. |
| `crates/mesia-shader/tests/translated_graphics.rs` | Géométrie animée, vertex buffer dynamique, fence, lecture du back buffer, oracle par pixel, présentation DXGI et chronométrage. |
| `crates/mesia-shader/Cargo.toml`, `Cargo.lock` | Dépendances de développement Windows du benchmark et verrouillage associé. |
| `AGENTS.md`, `PLANS.md`, `AUTONOMOUS-NOTES.md` | Règles de contribution, point de reprise courant et historique daté. |
| `docs/architecture.md`, `docs/perf-study.md` | Cartographie réelle, sémantique du compteur hôte, commandes et six séries de mesures. |
| `HANDOFF-DEVIN.md` | Présente passation. |

Le snapshot conserve aussi `scripts/wer.ps1`, utilitaire local antérieur, et les fins de ligne locales de `STATUS.md`, `STATUS_FR.md`, `docs/concurrency-validation.md`, `docs/d3d12-validation.md` et `docs/test-inventory.md`. `HANDOFF-FPS.md` décrit les campagnes historiques. Pour le profil actuel, se référer au plan V5.2 et aux valeurs réellement lues dans `scripts/play.ps1`.

## Preuves acquises

| Contrôle | Résultat et portée |
|---|---|
| 17 tests VALU, un par processus | 17/17 réussis sur le binaire release du 25 novembre. Scène native à deux images et vérification de fraîcheur réussies sur WARP. |
| Compute traduit | Debug et release réussis. P50 release de 21,887, 22,276 et 21,593 ms par dispatch avec lecture. |
| Compteur hôte | `host::failed_and_occluded_presents_do_not_advance_the_meter` réussi. Entrées injectées : `E_FAIL`, `DXGI_STATUS_OCCLUDED`, `S_OK`. Une seule incrémentation. `cargo check` avec la feature `diagnostics` réussi. |
| Graphique traduit et référence PC | Debug graphique traduit réussi. En release, trois passages par mode, 8 images d’échauffement puis 64 mesurées. Sur les trois passages traduits, 192/192 images correctes et acceptées, 190 identifiants d’affichage observés. Le statut à l’écran des deux autres reste inconnu. |
| Cadence et mémoire | 143,937–143,982 présentations correctes/s sur le rectangle 128 × 64, VSync activée. P99 de 7,217–7,292 ms. Working set de 74 Mio, usage GPU du processus de 11 Mio locaux et 26 Mio non locaux en fin de passage traduit. Résultats complets et bornes d’affichage dans [docs/perf-study.md](docs/perf-study.md). |

Les preuves couvrent des essais courts et ciblés. L’incident historique asynchrone `DEVICE_HUNG` après `016dddb` demeure ouvert (23 soumissions anticipées, 131 listes). Le contrôle global de formatage avait signalé des écarts antérieurs. Aucune suite globale récente ni endurance sur charge représentative n’est attestée. Cette passation a vérifié uniquement les fichiers.

## Prochains chantiers, dans l’ordre

1. **Qualifier le replay.** Obtenir la scène originale ou autorisée, la séquence d’entrée, la résolution interne, les images attendues et une référence CPU/readback. Démarrer en environnement propre avec `MESIA_LATE_WAIT=0`, `MESIA_INPLACE=1`, relais absent et copies de retour actives. Conserver un état de cache comparable. Faire trois passages avec la même séquence. Archiver les résultats hors du dépôt avant le lancement suivant.
2. **Identifier le coût dominant.** Relever par image le nombre de draws et de soumissions, les plages et octets copiés dans chaque sens, le temps CPU de préparation, les attentes de fence, les temps GPU, les compilations PSO et la pression mémoire. Sur le petit rectangle, la fence et la cadence écran dominent. Ces mesures ne classent pas les coûts du replay.
3. **Choisir un seul changement sur preuve.** Si `declared_writes` domine, chronométrer la recherche linéaire par shadow et tester un index local en préservant l’ordre des ressources et des plages, y compris les doublons. Si les transferts dominent, mesurer copies redondantes et allocations temporaires dans `shadow.rs` et `watch.rs`. Si la compilation domine, mesurer hits, refus et taille des caches DXIL/PSO ainsi que la RAM des workers. Conserver uniquement un gain répété à rendu et readback équivalents.
4. **Valider et transmettre.** Comparer P50/P95/P99, images correctes et distinctes par seconde, RAM, VRAM et stabilité avant/après. Un seul test GPU à la fois, compilation terminée avant mesure. En cas de TDR, conserver HRESULT, fences et breadcrumbs et arrêter l’essai. Pour le rollback, restaurer les fichiers du lot depuis le snapshot dans une copie isolée. Le suivi des paliers 500/400/333,3/285,7/250 ms commence sur le replay qualifié.
