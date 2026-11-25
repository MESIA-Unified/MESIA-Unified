# mesia

[English](README.md) | **Français**

**mesia** exécute nativement la version Xbox Series du jeu sur PC Windows
avec une carte graphique NVIDIA, à partir des fichiers du jeu que vous
fournissez vous-même.

**Ce dépôt ne contient aucun fichier ni asset du jeu.** Vous devez posséder
votre propre copie du jeu. Projet non affilié ni approuvé par son
développeur ou son éditeur.

## État (2026-11-25)

Testé sous Windows 11 avec une **NVIDIA GeForce RTX 3060** (12 Go) et 16 Go de RAM.
Notes de progression détaillées et blocages actuels : **[STATUS_FR.md](STATUS_FR.md)**

| | |
|---|---|
| Démarrage, écrans de premier lancement, intro, crédits | fonctionnel |
| Prologue | jouable de la cellule à la poursuite en bateau : cellule, cafétéria, braquage, poursuite policière (niveau recherché, minimap), bateau, jumelles |
| Stabilité | 1 h 30+ en une session sans crash |
| Framerate | 1-3 fps dans le monde (le premier run est plus lent pendant la traduction des shaders ; ils sont ensuite mis en cache) |
| Problèmes connus | tout ce qui est dans l'ombre est noir et les scènes lumineuses sont surexposées (l'éclairage ray-tracé du jeu ne tourne pas encore) ; quelques artefacts en blocs ; les bateaux sont enfoncés dans l'eau |

## Prérequis

- Windows 10/11 x64, un CPU avec AVX2
- Un GPU Direct3D 12 avec shader model 6.6 et mesh shaders (NVIDIA RTX série 20 ou plus récent)
- Rust (la version indiquée dans `rust-toolchain.toml`)
- Plusieurs Go de mémoire commit Windows disponible (augmenter le fichier d'échange si `crashes.log` signale l'erreur 1455)

## Compilation

```powershell
cargo build --release -p mesia
```

## Jouer

```powershell
powershell -ExecutionPolicy Bypass -File scripts\play.ps1 -Game "<dossier du jeu>\<exe du jeu>"
```

`scripts\play.ps1` conserve le cache de shaders et les sauvegardes dans les
dossiers que vous choisissez (`-Cache`, `-Data`) et n'appuie sur aucune
touche à votre place. Manette : pads XInput (un DualShock/DualSense via
DS4Windows fonctionne). Sans manette connectée, le clavier fait office de
manette quand la fenêtre du jeu a le focus.

## Réglages plus rapides (optionnels, désactivés par défaut)

`MESIA_RELAY=2`, ou `MESIA_LATE_WAIT=3 MESIA_WIDE_BACK=1` réduisent le
temps par frame sur les machines de test. Les définir dans PowerShell
avant de lancer le script.

## Options de débogage

| Option | Effet |
|---|---|
| `MESIA_CAPTURE=<dossier>` + `MESIA_CAPTURE_FRAMES=<liste>` | enregistre ces frames en PNG (build diagnostics) |
| `MESIA_KEYS=30:Enter,35:X` | appuie sur des touches à ces secondes (build diagnostics) |
| `MESIA_D3D12_DRED=1` | en cas d'échec GPU, rapporte les commandes en vol |
| `MESIA_PIPELINE_WAIT_MS=24` | attente par frame plus courte pour les pipelines en cours de construction |

Build diagnostics : `cargo build --profile diag -p mesia --features diagnostics`

## Licence

GPL-3.0-or-later (voir `LICENSE`). Basé sur MESIA (Marvisak) et mesia-fork (fxceless).
