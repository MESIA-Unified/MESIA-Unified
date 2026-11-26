# ANIMATION-TRANSITION-VALIDATION — fixtures animations/transitions (V5.6)

Objectif §7 : vérifier que le moteur actualise les états graphiques — pas
seulement qu'il affiche une image. Ce document consigne la couverture
existante, le protocole et les ajouts nécessaires. Ressources : originales
uniquement (fixtures GFX10 synthétiques du dépôt).

## 1. Ce qui est déjà validé (fixtures existantes)

| Propriété | Fixture | Preuve |
|---|---|---|
| Animation de géométrie | `translated_graphics.rs` (566 LOC) | VB réécrit chaque frame (sommets mobiles), 64/64 images pixel-exactes vs oracle CPU, **190 IDs d'affichage distincts** — les images progressent, ne sont ni figées ni dupliquées |
| Présentations réelles | idem | `Present(1,0)` → `S_OK` ×64, `GetFrameStatistics` confirme les IDs affichés (pas d'occlusion masquée) |
| Compteur honnête | `host::failed_and_occluded_presents_do_not_advance_the_meter` | `E_FAIL`/`DXGI_STATUS_OCCLUDED` ne comptent pas — pas de FPS artificiel |
| Traduction compute | `compute_correctness.rs`, `compute_advanced.rs`, fixture GFX10 compute (8 dispatches 1..8) | readbacks numériques exacts sur RTX 3060 |
| Conformité ALU | 17 tests VALU GFX10 | opérations scalaires/vectorielles traduites, résultats exacts |
| Fraîcheur des données in-world | soak `LATE_WAIT=2` (11 h puis 50 min) | scène 3D animée continue (personnages, véhicules, trafic), captures horodatées distinctes et cohérentes |

## 2. Écarts à couvrir (fixture `animation_transition` à écrire)

Prolonger `translated_graphics.rs` — même squelette D3D12/DXGI, oracle CPU.

| Effet à couvrir | Mécanisme de fixture | Oracle |
|---|---|---|
| Constante d'animation | `u32 time` dans un CB root, incrémentée par frame ; VS décale `x = base + a*sin(t)`, PS module la couleur | CPU recalcule `sin` et compare chaque pixel |
| Transition scène A→B | deux PSOs (deux paires VS/PS GFX10) + `blend` uniforme 0→1 sur N frames ; frame clé pour déclencher (simulée : changement de CB à l'index k) | pixels = `lerp(A,B,t)` exacts ; à t=1 : image = scène B pure (la transition **se termine**) |
| Historique d'image | texture intermédiaire : frame n copie le back buffer n-1 en texture ; PS échantillonne `0,75*current + 0,25*history` (flou temporel minimal) | oracle récursif sur les images précédentes vérifiées ; couvre "intermediate textures" + "frame history" |
| Fondu entrant/sortant | cas particulier de `blend` avec A = couleur pleine | idem transition |
| Post-traitement | PSO de passe 2 (texture→écran) avec un opérateur (gamma/luminosité) sur la texture intermédiaire | oracle CPU de l'opérateur |

Assertions du test :
- Chaque image vérifiée byte-exacte vs l'oracle (comme la baseline, pas de
  tolérance floue) ; `distinct IDs` ≥ images−2 (vsync peut répéter un
  affichage mais jamais les pixels vérifiés).
- Au point de bascule `k` : les images doivent effectivement changer (sinon
  la transition n'a pas été déclenchée).
- Après `k + N` : l'historique doit converger vers la scène B (le "flou"
  disparaît quand il doit) — borne : `|pixel_n - B| ≤ ε` après N+2 frames
  avec ε=0 puisque l'oracle est récursif exact.

## 3. Entrées et états (§8)

Le déclencheur d'entrée clavier dans la fixture est simulé par l'index de
frame `k` — la véritable chaîne `key → état → transition` est exercée en jeu
(les menus du titre répondent aux entrées ; V5.3 a documenté le passage
menu→monde automatisé). Une fixture input-level demanderait le chemin
d'entrée HLE du titre — hors scope fixture, à consigner pour Astra.

## 4. Limites honnêtes

- Le "motion blur" de la fixture = feedback texture 0,25 — couvre le
  mécanisme d'historique, pas le filtre multi-échantillons du titre.
- Le déclenchement par entrée réelle (clavier/manette→CB) n'est pas couvert
  par fixture ; il l'est par observation en jeu (menus fonctionnels).
- Artefacts connus non régressifs et hors fixture : bords de fenêtre
  pixelisés, patchs de skinning (écrits compute→vertex non rapatriés —
  documentés AUTONOMOUS-NOTES).
