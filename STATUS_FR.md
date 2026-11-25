# MESIA — État d'avancement & Feuille de route

[English](STATUS.md) | **Français**

_Dernière mise à jour : 2026-11-25. Machine de test : Windows 11, NVIDIA GeForce RTX 3060 12 Go (pilote à jour), 16 Go de RAM._

MESIA exécute nativement la version Xbox Series du jeu sous Windows : le
code du titre tourne non modifié dans un processus Win32 ré-hébergé,
et ses appels kernel, D3D12X (variante Xbox), DirectStorage, XAudio/Opus
et GameInput sont traduits vers leurs équivalents PC. Les shaders sont
traduits en DXIL hôte et mis en cache sur disque entre les runs.

## Ce qui fonctionne aujourd'hui

| Domaine | État |
|---|---|
| Démarrage & écrans de premier lancement | Fonctionnel — atteint le flux propre du titre sans patch |
| Séquence d'intro / crédits d'ouverture | Fonctionnel, présenté à ~30 fps |
| Prologue | Atteint les sections jouables (cellule → cafétéria → braquage → poursuite policière → bateau) |
| Entrées | GameInput → GameInput hôte ; manette + clavier énumérés |
| Audio | Décodage Opus validé stable sur des runs de plusieurs minutes (aucun underrun/pop observé à 48 kHz stéréo) |
| Sauvegardes / lecteurs virtuels | `G:` (dossier du jeu) et `T:` (temp du titre) mappés ; stockage par titre dans `<data>/<id du titre>/` |
| Cache de pipelines shaders | Bibliothèque persistante (`cache/pipelines/host-pipelines.bin`, sauvegarde auto toutes les 20 s) — ~92 Mo actuellement ; chaque run compile moins de shaders à la première rencontre |
| Images préparées | Binaires du titre ré-hébergés une fois dans `cache/images/mesia-image-*.dll` (imports liés, TLS redirigés) |

## Performances mesurées

| Phase | Framerate | Remarques |
|---|---|---|
| Écrans 2D / chargement | ~30 fps | Scènes légères ; les frames présentées sont réelles, pas un splash |
| Monde (début, pipelines froids) | 0,2 – 1,7 fps | Dominé par les compilations de pipelines à la première rencontre et les uploads de textures |
| Monde (cache plus chaud) | 1 – 3 fps | S'améliore run après run à mesure que `host-pipelines.bin` se remplit |

Répartition d'une frame lourde (~1–5 s) selon les compteurs du moteur :

- **Refreshes de textures : jusqu'à ~3–4 s/frame** — lecture, hash, détiling
  et upload de la mémoire textures du titre (des milliers de vues choisies
  par index chaque frame pendant le streaming du monde ; plus de 20 000
  uploads en file observés).
- **Copies de buffers : ~100–500 ms/frame** — mémoire du titre → copies hôte.
- **Copy-back (relais) : ~400–900 ms/frame** — renvoi des résultats dans la
  mémoire du titre depuis un thread séparé (`MESIA_RELAY`).
- **Passes heap-buffers : ~300–1 300 ms/frame** — passes de comparaison
  côté carte.

Le thread de soumission n'attend la carte que quelques dizaines de ms —
le goulot est le travail CPU côté hôte (refresh/copie/hash), pas le GPU.

## Blocages actuels

1. **Plafond de heap de descripteurs NVIDIA (1 000 000 de vues).**
   Le titre construit un heap de ~1,24 M de slots. D3D12 sur NVIDIA refuse
   les heaps au-delà d'1 M ; MESIA plafonne donc le heap hôte et traduit
   chaque shader avec le clamp (`set_view_heap_limit`) : les lectures
   au-delà du plafond renvoient un slot nul au lieu de faire planter le GPU.
   Conséquence : des textures liées via des indices élevés apparaissent
   manquantes/nulles. Un vrai fix demande une couche de remapping des
   slots, pas un heap plus grand.

2. **Tempête de pipelines à la première rencontre.**
   Le monde lie >1 100 pipelines qui n'existent pas encore. Les draws dont
   le pipeline compile encore sont droppés pour cette frame
   (`MESIA_PIPELINE_WAIT_MS` borne l'attente), donc les scènes de premier
   run montrent des éléments manquants/clignotants — les écrans de
   chargement « buggés ». Ça disparaît à mesure que la bibliothèque sur
   disque se remplit entre les runs.

3. **Pipelines compute refusés (`0x80070057`, E_INVALIDARG).**
   Une poignée de blobs DXIL compute traduits sont rejetés par le
   `CreateComputePipelineState` du driver. Rare (2–3 par session) mais
   chaque refus droppe ses draws définitivement pour le run.
   En cours d'investigation.

4. **Éclairage ray-tracé non actif.**
   Les ombres sont totalement noires et les scènes lumineuses surexposées :
   le chemin d'éclairage/GI ray-tracé du jeu n'est pas encore traduit.
   Artefacts en blocs par endroits ; les bateaux sont enfoncés dans l'eau
   (problème physique/simulation, pas graphique).

5. **Plafond de débit.**
   Même avec tous les réglages perf actifs, les frames du monde sont
   limitées par le refresh textures/buffers CPU + le copy-back. Les deux
   plus gros leviers restants : batcher davantage les uploads de textures
   et élargir la fenêtre de buffers côté carte (`MESIA_CARD_BUFFERS_MB`,
   `MESIA_CARD_WINDOWS_MB`).

## Réglages perf utilisés par `scripts/play.ps1`

```
MESIA_RELAY=2                # le copy-back tourne à côté du thread de soumission
MESIA_LATE_WAIT=3            # point de settle le plus tardif
MESIA_WIDE_BACK=1            # batchs de copy-back élargis
MESIA_PIPELINE_WAIT_MS=24    # borne l'attente pipelines par frame (défaut 500)
MESIA_CARD_BUFFERS_MIN_MB=1  # comparaisons côté carte pour les buffers >=1 Mio
MESIA_JOIN_COPIES=1          # fusionne les copies de buffers consécutives
MESIA_CARD_REHASH_RUNS=1     # une seule passe de hash sur les copies carte
```

## Prochaines étapes

- Remapping des slots pour les heaps de vues >1 M (corrige les textures nulles).
- Corriger les refus de pipelines compute `0x80070057`.
- Préchauffage offline/anticipé des pipelines depuis `early-pipelines.txt`.
- Chemin d'upload de textures batché (couper les pics de refresh de 3–4 s).
- Chemin d'éclairage ray-tracé (débloque ombres/exposition).

_Projet non affilié ni approuvé par le développeur ou l'éditeur du jeu.
Ce dépôt ne contient aucun fichier ni asset du jeu._
