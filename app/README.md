# N7RT CAN Lab — application (Tauri 2)

Cockpit Virtual CAN : l'application détecte la NUCLEO-H753ZI, établit N7RT-Link v0, pilote une session, envoie des `VCAN_FRAME` générées par un ECU virtuel et affiche en direct ce que `student_decoder.c` publie sur la vraie carte.

La page **Code STM32** intègre un éditeur CodeMirror 6 pour `student_decoder.c/.h`, avec historique, recherche/remplacement, pliage, complétion de l'API publique et diagnostics GCC. Elle enregistre le travail dans le profil local, compile contre le SDK précompilé et programme la seule zone étudiante. Parcours illustré : [tutoriel](../docs/inapp-student-ide-tutorial.md) ; preuves : [rapport R1](../docs/inapp-student-ide-r1.md).

Ce dépôt compile l'**édition publique** : modes étudiant et encadrant, sans oracle ni verdict ; le runner `scripts/test-workspace.py --public` y exécute les tests publics. L'édition encadrant, qui contient l'oracle, se construit à partir du dépôt encadrant. Principe : [docs/distribution.md](../docs/distribution.md).

Validation et mesures : [docs/n7rt-can-lab-app-v0.md](../docs/n7rt-can-lab-app-v0.md). Protocole : [docs/n7rt-link-v0.md](../docs/n7rt-link-v0.md).

## Organisation

```text
app/
  crates/n7rt-link/     codec N7RT-Link v0 (COBS, CRC-32, messages, récepteur), sans dépendance ;
                        tests publics de propriétés ; conformité exacte dans le dépôt encadrant
  crates/n7rt-lab/      cœur sans UI : découverte des ports, moteur de liaison (thread link),
                        générateur Virtual CAN (thread sim), métriques, vérification e2e, fausse carte,
                        missions (modes étudiant et encadrant, bibliothèque, essai nominal)
  crates/n7rt-circuit/  Physical Lab : schéma (composants, fils, réseaux, annuler/rétablir, persistance)
  crates/n7rt-solver/   Physical Lab : solveur de circuits linéaires déterministe
  crates/n7rt-vehicle/  bus véhicule synthétique déterministe, paramètres lus dans le pack de mission ;
                        sans oracle dans l'édition publique
  crates/n7rt-seal/     empreinte du code encadrant (Argon2id), packs de groupe scellés (AES-256-GCM)
  crates/n7rt-tool/     outil console encadrant n7rt-tool (code, packs de groupe, jugement des essais ;
                        commande judge refusée dans l'édition publique)
  src-tauri/            coquille Tauri : commandes typées, mini-IDE, mode --e2e
  ui/                   TypeScript + Vite, sans framework ; store + vues rendues par requestAnimationFrame,
                        CodeMirror 6 pour l'éditeur C intégré
  scripts/              check-ui.mjs (garde anti-innerHTML), windows-gnu-env.ps1,
                        test-workspace.py (tests Rust et manifeste des tests Tauri Windows)
  tools/zig-shims/      windres/dlltool/ld pour Windows sans MSVC (voir plus bas)
```

Sources de vérité réutilisées, jamais recopiées :
- le runtime épinglé `firmware/n7rt_student/runtime-release.json` ;
- son image, extraite au build du SDK (`n7rt/lib/n7rt_runtime.o`) pour vérifier le défi SHA-256 de `HELLO_ACK` ;
- le contrat [N7RT-Link v0](../docs/n7rt-link-v0.md), ses formats et constantes ; les fixtures exacts ne sont pas distribués ici.

## Prérequis

| OS | Prérequis |
|---|---|
| Tous | Rust stable (rustup), Node 22 (22.12 ou plus récent) ou Node 24 (Node 24 pour reproduire la release), `npm ci` dans `app/` ; Python 3.11 ou plus récent pour la suite du projet |
| Windows (standard) | Visual Studio Build Tools 2022 (C++), WebView2 (présent sur Windows 10/11) |
| Windows sans droits admin | Voir « Windows sans MSVC » |
| macOS 13+ | Xcode Command Line Tools |
| Linux (Ubuntu 22.04/24.04) | `libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libudev-dev pkg-config build-essential` ; compte dans le groupe `dialout` ; règle udev `ID_MM_DEVICE_IGNORE=1` pour le VID 0483 |

Pour utiliser **Code STM32**, Python 3.11+ et `pyserial` sont nécessaires au helper (`python -m pip install -r tools/stm32/requirements-student-ide.txt`, depuis la racine du dépôt). GCC ARM compile le code étudiant ; STM32CubeProgrammer est nécessaire au flash. Ces outils restent installés sur le poste et ne sont pas inclus dans le ZIP. Le ZIP inclut les helpers et le SDK : il fonctionne extrait hors de ce dépôt.

## Commandes

```powershell
cd app
npm ci                            # dépendances exactes de package-lock.json
npm run check                     # tsc + gardes UI et Canvas (pas d'innerHTML, pas d'inline script/style)
npx vitest run                    # vitest : canvas, panneaux, unités, fixtures des DTO (= npm test)
python scripts/test-workspace.py --public --jobs 1 --build-date 2026-10-08  # tests Rust publics, src-tauri compris
npm run tauri dev                 # développement (Vite + rechargement)
npm run tauri build -- --no-bundle   # exécutable autonome : target/release/n7rt-can-lab(.exe)
npm run tauri build               # installeurs : NSIS (Windows), .deb/AppImage (Linux), .dmg (macOS)
```

Suite du projet, depuis la racine du dépôt (contrôles des rapports du banc sur rapports synthétiques, gardes UI, `npm test` quand `app/node_modules` existe) :

```powershell
python -m pip install ".[dev]"    # dépendances et outils de test (pytest, psutil, pillow, ruff)
python -m pytest                  # toute la suite Python
python -m pytest -q tools/stm32/tests/test_n7rt_app.py
$env:N7RT_RUN_CARGO = "1"; python -m pytest -q tools/stm32/tests/test_n7rt_app.py   # + tests Rust des quatre crates
```

Sous Windows sans MSVC, `cargo test` et la variante `N7RT_RUN_CARGO` demandent d'abord `. .\app\scripts\windows-gnu-env.ps1` dans la même session.

Le runner ci-dessus prépare lui-même l'environnement GNU. Sous Windows, il ajoute le manifeste standard Common Controls v6 de Tauri aux seuls binaires de tests qui en ont besoin : sans ce manifeste, le chargeur peut s'arrêter avec `0xC0000139` avant les assertions. Aucun réglage Windows ni binaire de release n'est modifié. Le résumé est écrit dans `app/target/test-workspace-public.json` (chemin relatif à la racine du dépôt).

Depuis la racine du workspace combiné, les tests avec oracle se lancent ainsi :

```powershell
python app/scripts/test-workspace.py --oracle --jobs 1 --build-date 2026-10-08
```

Depuis le dépôt étudiant avec le dépôt encadrant voisin, ajouter au besoin `--oracle-dir ../n7rt-canlab-missions/oracle/n7rt-vehicle`. Le rapport correspondant est `app/target/test-workspace-oracle.json`. Un ciblage backend se fait avec `--package n7rt-can-lab -- ide::tests --nocapture`.

Banc, carte réelle déclarée dans `tools/stm32/boards.json` ou `tools/stm32/boards.local.json` :

```powershell
python tools/stm32/stm32lab.py app-test <carte>      # scénario fonctionnel coordonné + charge 600 trames/s 120 s
python tools/stm32/stm32lab.py app-test <carte> --scenario perf --seconds 600 --release
```

Physical Lab, sans carte (ni ST-LINK, ni SWD, ni port série) : l'application rejoue ses scénarios `--e2e phys` (test manuel, attendus du §9) et `--e2e phys-perf` (budgets du §8.6) de [docs/physical-lab-v1.md](../docs/physical-lab-v1.md), puis le banc imprime une ligne PASS ou FAIL par budget :

```powershell
python tools/stm32/stm32lab.py app-test --phys                                 # phys + phys-perf
python tools/stm32/stm32lab.py app-test --phys --scenario phys-perf --release
```

Rapport de chaque exécution : `data/logs/app_<scénario>_<carte>_<date>.json`, ou `app_<scénario>_<date>.json` sans carte (vérifications, échantillons CPU et mémoire par processus), captures des étapes `shot:` à côté (`<rapport>_<nom>.png`).

Test manuel de 3 minutes du Physical Lab : [docs/physical-lab-v1.md §13](../docs/physical-lab-v1.md).

## Windows sans MSVC

La chaîne vérifiée sur le banc n'a ni Visual Studio ni MSYS2. Elle est entièrement au niveau utilisateur :
1. **Rust.** `rustup-init --default-host x86_64-pc-windows-gnu --profile minimal --no-modify-path`. Le linker MinGW est fourni par la toolchain.
2. **zig 0.15.2** dans `%LOCALAPPDATA%\n7rt-toolchain\zig-0.15.2` (par exemple, extrait de la roue PyPI `ziglang==0.15.2`). Il sert à `zig rc` et `zig dlltool`.
3. **`tools/zig-shims`.** `cargo build --release`, puis copier l'exécutable en `n7rt-toolchain\bin\windres.exe`, `bin\dlltool.exe` et `lld\ld.exe`.
4. Dans chaque session : `. .\scripts\windows-gnu-env.ps1`. Il ajoute `bin` au PATH et fixe `RUSTFLAGS=-C link-arg=-B…/lld/`.

**Pourquoi c'est nécessaire.**
- **Ressources** (icône, manifeste) : `tauri-build` les compile avec `windres`.
- **Imports raw-dylib** (crate `windows-link`) : rustc les génère avec `dlltool`, qui exige un assembleur.
- **Édition de liens** : GNU ld désordonne la table d'imports quand ces bibliothèques « courtes » côtoient celles de MinGW. Constaté : le script de build de `tauri` plantait sur une entrée `GetModuleHandleA` non résolue. L'édition de liens passe donc par `rust-lld`.

Avec Visual Studio Build Tools, rien de tout cela n'est utile.

## Règles de l'UI

Design system N7RT, à lire avant tout nouvel écran : [ui/DESIGN.md](ui/DESIGN.md).
- Thème sombre type IDE, violet N7RT réservé à la hiérarchie et aux actions.
- Aucun coin arrondi, aucune ombre, aucune animation continue.
- La garde `npm run check` refuse rayons, ombres, `@keyframes`, transitions de plus de 150 ms et `innerHTML`.

- **Données dynamiques** (carte, ports, logs étudiants) : uniquement `textContent` et setters d'attributs. `scripts/check-ui.mjs` fait échouer `npm run check` et la suite du projet sur `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`, `eval`… La CSP interdit les scripts inline et les styles non autorisés ; les styles internes de CodeMirror utilisent le nonce fourni par Tauri et le contrat décrit dans `ui/DESIGN.md`.
- **Aucune solution de décodage dans l'UI.** Elle montre :
  - les trames brutes et ce que le code étudiant a publié ;
  - les stimuli, qui sont les entrées d'un ECU virtuel. Leur encodage d'entraînement public est documenté dans `crates/n7rt-lab/src/sim.rs` et n'est jamais affiché.
- **Flux de données** : l'UI tire les changements (`lab_drain`) toutes les 50 ms.
  - Le cœur ne pousse pas d'événements : avec `emit`, le processus WebView2 grossissait d'environ 1,8 Mo/s à 600 trames/s.
  - Les vues ne touchent le DOM qu'au `requestAnimationFrame`, et seulement pour ce qui a changé.
  - Le tableau garde 200 lignes : les nouvelles sont insérées en tête, les plus anciennes retirées.
- **Évolution.** Une nouvelle vue (le Physical Lab en Canvas2D) implémente `View` (`ui/src/views/cockpit.ts`) et lit le même `Store`.
