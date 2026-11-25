# Handoff — MESIA : passer de ~1 fps à 3-5 fps constants

## Contexte

MESIA = couche de compatibilité Rust/Windows qui exécute un jeu Xbox Series X (GTA6, `game_scarlett_master.exe`) en traduisant ses appels D3D12.X vers D3D12 PC + ses shaders wave64 RDNA2 vers DXIL wave32 (NVIDIA RTX 3060).

- **Code de travail** : `C:\Users\utilisateur\Desktop\mesia-main` (NON git — source de vérité)
- **Miroir git** : `C:\Users\utilisateur\Desktop\mesia-upload` → pushé sur github.com/MESIA-Unified (à jour jusqu'à `409ba29`). Copier les fichiers modifiés de `mesia-main` vers `mesia-upload`, puis commit+push.
- **Notes d'état** : `mesia-main/AUTONOMOUS-NOTES.md` (lire en entier — historique des décisions)
- **Ce doc** : `mesia-main/HANDOFF-FPS.md`
- Build : `cargo build --release -p mesia` (dans mesia-main) ; exe = `target\release\mesia.exe`
- Tests : `cargo test -p mesia-gpu` (et autres crates)

## Lancer / mesurer

- `powershell scripts/play.ps1 -Game "C:\Users\utilisateur\Desktop\GTA6.Xbox.v1.0.2.1-CYBERLEEK\game_scarlett_master.exe" [-Fast]` — `-Game` = l'EXE, pas le dossier. `-Fast` = profil fluidité (COPY_MODE=none, GUARD_HOLES=0, UNWATCHED_FRAMES=8).
- Log : `%TEMP%\mesia.log` (ANSI à striper : `sed 's/\x1b\[[0-9;]*m//g'`).
- Screenshot même fenêtre occultée : `scripts/shot.ps1 -PrintWindow` → `%TEMP%\mesia-shot.png`.
- Input : `scripts/drive.ps1 -Cmd key|keys|drag`.
- Timing : `MESIA_GPU_TIMING=1` avant play.ps1 → lignes `card's clock per frame` dans le log. Lecture : `nothing of ours queued X ms (card ~Y% busy)` = travail GPU ; `while the submitting thread was working` = temps CPU où le GPU attend = goulot CPU.
- Env utiles : `MESIA_D3D12_DRED=1`, `MESIA_WAVE_WHY=1` (pourquoi un shader refuse), `MESIA_PIPELINE_WORKERS=12`.

## État actuel

- Boot→monde : ~1-2 min à cache chaud. Cinématique in-engine **~720-830 ms/frame (~1 fps), ~4700-5000 draws/frame**. Menu/loading : 30 fps (scène légère).
- Frame = **~360-390 ms GPU** (card ~47-52% busy) + **~350-590 ms CPU** (card idle behind submitting thread). Les DEUX moitiés doivent chuter pour 3-5 fps (200-333 ms).
- Stabilité : 4 fix Phase 1 commités (voir AUTONOMOUS-NOTES). Suspect TDR n°1 = `s_barrier` dans régions divergentes (mesia-shader `lower.rs` ~4674) — groupe attend des threads sortis → hang NVIDIA. Aucun device-removed observé sur les derniers runs (soak en cours).

## Les 3 goulots CPU mesurés (audit F1, agent d1551c8b)

1. **Watch/copy non-incrémental** — `crates/mesia-gpu/src/watch.rs`, `host.rs` (`run_deferred`, `Batch::flush`) : ~10 GB scannés à chaque soumission. Rendre incrémental (journal par page déjà existe côté `watch::wrote`). Flags d'isolation existants : `MESIA_ASK_ONCE` (défaut 1), `MESIA_GUARD_HOLES`, `MESIA_COPY_MODE` (guarded|plain|none — none = zéro copies, textures gelées).
2. **`relay_wait` ~200 ms/soumission** — le soumetteur attend les dépendances du relay worker (`host.rs` ~2346, `relay()`/`relay_exact()` ~1867-1881). Réduire l'étendue des attentes ou pipeliner plus finement.
3. **`tables()`/`bind`/`heap_buffers` ×5000 draws** — hash mémoire titre par table, `dynamic()` scan linéaire sous mutex, mirror par slot (`draw.rs`, `descriptor/`).

Leviers GPU (phase suivante) : moins de syncs/échanges dans le DXIL émis (`mesia-shader/src/emit/`, `lower.rs`) ; `s_barrier` fence_only carve-out existe (`MESIA_GROUP_SYNC` 0/1/2 — 1 = races visibles prouvées, 2 = défaut sûr). `MESIA_WAVE_WIDE=0` actif (widen = known-bad).

## Contraintes

- Un seul test GPU à la fois (RTX 3060). Pas de modif TDR/registre. Commits atomiques dans mesia-upload. Pas de stubs silencieux. Qualité visuelle : le user accepte « moche » (textures gelées OK) tant que ça bouge.
- Cache shader clé par BLAKE3 de l'exe → TOUT rebuild = re-traduction complète (~7 min de tempête, sauvegardes incrémentales OK). Tester le code en `cargo test` autant que possible avant un run jeu.
- `mesia.exe` a aussi `translate`, `inspect`, `census`, `shader-info` (analyse hors-jeu).

## Plan d'attaque proposé

1. Mesurer une frame monde stable : extraire du log `card's clock` + `slow host list`/`slow list` + compteurs `copystat` (lignes `whole`, `copies`, `watch`).
2. Attaquer le goulot #1 (watch incrémental) — c'est le plus gros et le mieux documenté.
3. Re-mesurer même scène ; puis #2 (relay_wait), puis #3 (binding).
4. Côté GPU : profiler quels coûts dominent (beaucoup de `s_barrier`? exchanges?) via `MESIA_WAVE_WHY` + stats du lower.
5. Palier visé : ≤333 ms/frame constant, même scène.
