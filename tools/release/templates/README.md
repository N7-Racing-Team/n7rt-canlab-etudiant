# N7RT CAN Lab — dépôt étudiant

Laboratoire CAN de la N7 Racing Team : une application Windows (Cockpit, Physical Lab, Code STM32) pilote une carte NUCLEO-H753ZI sur laquelle tourne votre décodeur `student_decoder.c`. La page **Code STM32** permet d'écrire, compiler et programmer vos deux fichiers étudiants. La carte reçoit les trames d'un véhicule simulé ; vous les décodez et publiez les grandeurs demandées par la mission.

Ce dépôt construit l'**édition publique** de l'application : elle n'affiche aucun verdict. Votre encadrant juge vos essais avec l'édition encadrant.

| Dossier | Contenu |
|---|---|
| `app/` | Code source de l'application (Rust + Tauri 2, interface TypeScript) et de `n7rt-tool` ; le build public de cet outil refuse de juger, l'oracle reste dans le dépôt encadrant |
| `firmware/n7rt_student/` | Projet STM32CubeIDE `N7RT_CANLab_Student` : runtime N7RT précompilé (`n7rt/`), gabarit à compléter (`student/`) |
| `library/` | Bibliothèque par liens : manifestes et `links.json` (titre, éditeur, révision, adresse chez l'éditeur) ; aucun PDF |
| `shared/` | Énoncés des missions, schémas, vecteurs du protocole N7RT-Link |
| `docs/` | Documentation publique : architecture, protocole, SDK, Physical Lab, distribution |
| `tools/` | Helper de Code STM32, outils du banc (`stm32/stm32lab.py`), codec public N7RT-Link, contrôle de la bibliothèque, release |

## 1. Installer la release

1. Télécharger `n7rt-canlab-<version>-windows-x64.zip` (page Releases du dépôt, ou fourni par l'encadrant). Si son empreinte est publiée (fichier `.sha256` à côté de l'archive), la comparer à celle que donne PowerShell : `Get-FileHash -Algorithm SHA256 .\n7rt-canlab-<version>-windows-x64.zip`.
2. Décompresser l'archive dans un dossier où vous avez les droits d'écriture (pas dans `Program Files`).
3. Lancer `n7rt-can-lab.exe`. L'exécutable n'est pas signé : à la première ouverture, Windows SmartScreen demande une confirmation (« Informations complémentaires » puis « Exécuter quand même »). WebView2 est fourni par Windows 10 et 11.

`LISEZMOI.txt` dans l'archive reprend ces étapes. Conservez tous ses dossiers à côté de l'exécutable : `tools/stm32/` et le SDK servent à Code STM32 ; `library/` fournit la liste des documents de la mission. L'application ouvre chaque document à l'adresse de son éditeur (accès internet nécessaire). Copie locale facultative : [library/README.md](library/README.md). `CODEMIRROR-LICENSES.txt` conserve les licences des composants embarqués de l'éditeur ; celui-ci ne télécharge aucune dépendance à l'exécution.

## 2. Écrire, compiler et programmer avec Code STM32

Prérequis : **Python 3.11 ou plus récent**, disponible par la commande `python`, **pyserial**, la chaîne GNU ARM contenant `arm-none-eabi-gcc` et **STM32CubeProgrammer**. Python et les outils ST s'installent séparément : aucun interpréteur Python ni compilateur n'est embarqué dans la release. Le helper découvre les outils dans les installations STM32Cube, `C:\ST` et le `PATH` ; un outil manquant produit un message de refus avant la compilation ou la programmation.

Dans le dossier de l'archive extraite :

```powershell
python -m pip install -r .\requirements-student-ide.txt
```

Depuis ce dépôt, le fichier de dépendances est `tools/stm32/requirements-student-ide.txt`.

1. Ouvrir **Code STM32**, puis `student_decoder.c` ou `student_decoder.h` : ce sont les deux seuls fichiers éditables. Le gabarit et les entrées protégées du SDK restent fournis avec l'application ; votre travail est sauvegardé dans le profil local.
2. Modifier le code, puis **Enregistrer**. L'API est décrite dans `N7RT_CANLab_Student/n7rt/include/n7rt.h` de l'archive, ou dans le projet sous `firmware/n7rt_student/` de ce dépôt.
3. **Compiler**, puis corriger les diagnostics de la vue **Problèmes**. Une sauvegarde qui change le source invalide la compilation précédente.
4. Dans le **Cockpit**, choisir et connecter une NUCLEO-H753ZI branchée sur ST-LINK (CN1). Attendre la vérification du runtime et arrêter la session avant de programmer.
5. Revenir à **Code STM32** et **Flasher** l'image validée, ou **Compiler + flasher**. L'application libère le port série, programme uniquement la zone étudiante, contrôle le runtime et reconnecte la même carte. Reprendre ensuite les essais dans le Cockpit.

Tutoriel : [docs/inapp-student-ide-tutorial.md](docs/inapp-student-ide-tutorial.md). Rapport de validation : [docs/inapp-student-ide-r1.md](docs/inapp-student-ide-r1.md). Ces contrôles protègent le téléversement ; le programme C sur le microcontrôleur n'est pas une sandbox de sécurité.

La [revue de clôture 0.2 R1](docs/release-0.2-closure-r1.md) suit les corrections et candidats locaux du 8 octobre, leur séparation étudiant/encadrant, les vérifications de distribution et le test mémoire prolongé. Son état décrit le candidat ; les preuves matérielles du rapport IDE concernent le binaire publié.

**Secours encadrant : STM32CubeIDE 2.0.0.** Le même projet `N7RT_CANLab_Student` peut être importé dans CubeIDE pour compiler ou déboguer. L'encadrant utilise le parcours protégé `stm32lab.py flash <carte> --student` pour programmer le code étudiant, après compilation ; une programmation directe par CubeIDE n'est pas ce parcours protégé. Détails du SDK : [docs/n7rt-student-sdk-v0.md](docs/n7rt-student-sdk-v0.md).

## 3. Choisir la mission

Au premier lancement, choisissez APPS, WSF, SAS ou BPS (« Choisir cette mission », puis « Confirmer »). Le choix est définitif pour la session et conservé après redémarrage : l'application n'ouvre ensuite que cette mission, et un pack de groupe n'ouvre que la mission pour laquelle il a été généré. Pour changer de session, demandez à votre encadrant. Consultez `docs/missions-four-r1.md` pour les résultats de validation et `docs/missions-four-manual-acceptance.md` pour la recette matérielle.

## 4. Importer le pack du groupe

L'encadrant remet à chaque groupe un fichier `<groupe>.n7rtpack` (par exemple `G01.n7rtpack`) et, séparément, un **code de groupe** : 16 caractères en quatre blocs, `XXXX-XXXX-XXXX-XXXX` (espaces, tirets et minuscules acceptés à la saisie). Le pack est chiffré : sans le code, il est illisible.

1. Importer le pack une fois (« Importer le pack du groupe… ») : l'application en garde une copie dans ses données, sur ce poste.
2. À chaque ouverture de la mission, saisir le code du groupe. Le code et le contenu déchiffré restent en mémoire : ils ne sont jamais écrits sur le disque.

Ne partagez ni le pack ni le code : chaque groupe a sa propre variante du véhicule.

## 5. Enregistrer l'essai nominal et l'envoyer

« Enregistrer l'essai nominal » joue l'essai caché sur votre carte et écrit `essai-<mission>-<groupe>-<aaaammjj-hhmmss>.n7rtrun` (heure UTC) dans `N7RT CAN Lab\essais`, sous votre dossier Documents de Windows. Si Documents est redirigé vers OneDrive, ce dossier est dans OneDrive ; le bouton « Ouvrir le dossier », affiché après l'essai, l'ouvre. Ne débranchez pas la carte et ne fermez pas l'application pendant l'essai.

L'édition publique ne donne pas de verdict : envoyez ce fichier à votre encadrant, qui le juge avec l'édition encadrant. Le fichier enregistre aussi l'identifiant unique de la carte, la version du runtime et le CRC de votre code ; l'essai décisif se fait en direct devant votre encadrant, sur la carte du groupe.

## 6. Construire depuis les sources

Prérequis : Rust stable (rustup), Node 22 (22.12 ou plus récent) ou Node 24, Python 3.11 ou plus récent. La release est construite avec Node 24 : versions exactes des outils dans le champ `toolchain` du `manifest.json` de l'archive. Windows : Visual Studio Build Tools 2022, ou la chaîne GNU sans droits administrateur décrite dans [app/README.md](app/README.md) (« Windows sans MSVC »).

```powershell
python tools/release/build_student.py --build-date 2026-10-08 --jobs 1
```

Le script vérifie la chaîne d'outils, lance `npm ci`, `npm run build` puis `cargo build --release --locked` en édition publique (sans oracle), et écrit dans `dist/` l'archive déterministe `n7rt-canlab-<version>-windows-x64.zip` et son `.sha256` : entrées triées, dates fixes, `manifest.json` avec l'empreinte SHA-256 de chaque fichier, les versions des outils et le commit source (`source` ; `dirty` signale des modifications locales). Construire depuis un clone sans modification locale. `--build-date` : la date de la release à reproduire (champ `build_date` de son `manifest.json`). Même commit, même chaîne d'outils, même date : mêmes octets. Les outils encadrant (édition encadrant, `n7rt-tool` avec l'oracle) ne se construisent pas ici et ne sont jamais publiés avec cette release.

Tests, depuis la racine du dépôt (Windows sans MSVC : `. .\app\scripts\windows-gnu-env.ps1` d'abord, dans la même session) :

```powershell
cd app
npm ci
npm run check
npx vitest run
python scripts/test-workspace.py --public --jobs 1 --build-date 2026-10-08
cd ..
python -m pip install ".[dev]"
python -m pytest
```

Développement de l'application : [app/README.md](app/README.md).

Banc en ligne de commande (optionnel) : `python tools/stm32/stm32lab.py build --student`, `flash <carte> --student`, `smoke-test <carte> --student` (carte déclarée dans `tools/stm32/boards.local.json`, même format que `boards.json`).

## 7. Ce qui n'est pas dans ce dépôt

| Absent | Pourquoi |
|---|---|
| Packs de référence, matrices, vecteurs dorés des missions, décodeurs de référence, règles de jugement | Ce sont les réponses des missions. Ils vivent dans le dépôt encadrant ; vous recevez seulement le pack chiffré de votre groupe. |
| Oracle (logique de référence côté récepteur, jugement des essais) | Compilé seulement dans l'édition encadrant. L'édition publique n'en contient que des substituts et ne donne aucun verdict. |
| Clé maîtresse, empreinte du code encadrant | Dans le coffre local réservé aux encadrants, hors Git ([distribution](docs/distribution.md)). L'exécutable de l'édition publique ne contient ni clé, ni code, ni oracle. |
| Source du runtime N7RT | Le runtime est distribué en binaire validé (`firmware/n7rt_student/runtime-release.json`) ; l'application vérifie son empreinte à chaque connexion. |
| Spécification véhicule, recherche | Dans le dépôt encadrant. |
| PDF des documents techniques | Bibliothèque par liens : chaque document est cité avec l'adresse de son éditeur ([library/README.md](library/README.md)). |

Principe et formats : [docs/distribution.md](docs/distribution.md). Les fichiers `*.n7rtpack`, `*.n7rtrun` et les copies locales `library/**/*.pdf` sont exclus du dépôt par `.gitignore` : ne les ajoutez pas.

Depuis la publication 0.2.0, ce dépôt versionné est la source de vérité de l'application et de sa distribution publique. Proposez les corrections ici par patch ou pull request, puis soumettez-les à la revue du mainteneur. Les anciens exports de l'espace combiné servent seulement à une migration vers une cible neuve non versionnée ; ils refusent d'écraser un clone Git. Les corrections locales de clôture restent un candidat non publié jusqu'à leur validation et à l'autorisation de commit/push ([distribution](docs/distribution.md#12-source-de-vérité-après-publication)).
