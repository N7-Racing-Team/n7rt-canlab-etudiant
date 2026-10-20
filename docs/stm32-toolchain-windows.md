# Outils de compilation et de programmation sous Windows (CAN Lab 0.3)

La page **Code STM32** de CAN Lab compile votre décodeur et programme la carte NUCLEO-H753ZI avec trois outils que vous installez vous-même, une seule fois :

| Outil | Rôle | Version vérifiée |
|---|---|---|
| **Python** 3.11 ou plus récent | lance le petit programme (le « helper », `tools\stm32\student_ide.py`) qui pilote la compilation et la programmation | 3.12.10 |
| **Arm GNU Toolchain** (cible `arm-none-eabi`) | compilateur C pour le microcontrôleur de la carte : `arm-none-eabi-gcc` | 14.3.rel1 |
| **STM32CubeProgrammer** | programme la carte par sa sonde ST-LINK : `STM32_Programmer_CLI` ; installe aussi le pilote USB du ST-LINK | 2.23.0 |

Sans eux, l'application s'ouvre et le Cockpit, le Physical Lab et la fiche fonctionnent : seuls **Compiler** et **Flasher** sont refusés, avec un message qui nomme l'outil manquant et renvoie à ce document.

Toutes les commandes ci-dessous se tapent dans **PowerShell**. Pour l'ouvrir dans le dossier de CAN Lab : dans l'Explorateur, ouvrez le dossier décompressé (celui qui contient `n7rt-can-lab.exe`), clic droit dans une zone vide, **Ouvrir dans le Terminal** (Windows 11), ou tapez `powershell` dans la barre d'adresse puis Entrée (Windows 10).

## 1. Python

1. Téléchargez l'installateur Windows 64 bits sur <https://www.python.org/downloads/windows/> (« Windows installer (64-bit) », version 3.11, 3.12 ou plus récente).
2. Au premier écran de l'installateur, **cochez « Add python.exe to PATH »**, puis **Install Now**. Les droits administrateur ne sont pas nécessaires.
3. **Fermez CAN Lab s'il est ouvert, puis relancez-le** : une application ne voit un nouveau programme du `PATH` qu'après son redémarrage.
4. Dans un nouveau PowerShell, vérifiez :

```powershell
python --version
```

Réponse attendue : `Python 3.11.x` ou plus récent. Si PowerShell ouvre le Microsoft Store ou répond « Python est introuvable », c'est l'alias Windows qui répond à la place de Python : Python n'est pas installé, ou pas dans le `PATH`. Réinstallez en cochant la case de l'étape 2 (bouton **Modify**, puis **Add Python to environment variables**), ou désactivez l'alias : **Paramètres → Applications → Paramètres avancés des applications → Alias d'exécution d'application**, décochez les deux lignes « Programme d'installation d'application python.exe / python3.exe ».

5. Toujours dans le dossier de CAN Lab, installez la dépendance du helper (elle sert au diagnostic de la section 5) :

```powershell
python -m pip install -r .\requirements-student-ide.txt
```

## 2. Arm GNU Toolchain (`arm-none-eabi-gcc`)

1. Ouvrez <https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads>.
2. Dans la rubrique **Windows (mingw-w64-x86_64) hosted cross toolchains**, partie **AArch32 bare-metal target (arm-none-eabi)**, téléchargez l'installateur `.exe` (par exemple `arm-gnu-toolchain-14.3.rel1-mingw-w64-x86_64-arm-none-eabi.exe`). Ne prenez pas une chaîne `aarch64-none-elf` ni `arm-none-linux-gnueabihf` : ce ne sont pas les bonnes cibles.
3. Lancez-le et gardez le dossier proposé : `C:\Program Files (x86)\Arm GNU Toolchain arm-none-eabi\<version>\`. L'installation demande les droits administrateur.
4. Au dernier écran, cochez **Add path to environment variable** (recommandé, pas obligatoire : voir la section 4).

Vérification :

```powershell
arm-none-eabi-gcc --version
```

Réponse attendue : une première ligne `arm-none-eabi-gcc.exe (Arm GNU Toolchain 14.3.Rel1 …) 14.3.1 …` ou une autre version 11 ou plus récente.

Sans droits administrateur : téléchargez à la place l'archive `.zip` de la même ligne, décompressez-la dans un dossier à vous (par exemple `C:\Users\<vous>\arm-gnu-toolchain\`) et ajoutez son sous-dossier `bin` au `PATH` (section 4) : un dossier choisi par vous n'est trouvé que par le `PATH`.

## 3. STM32CubeProgrammer (`STM32_Programmer_CLI`)

1. Ouvrez <https://www.st.com/en/development-tools/stm32cubeprog.html>, **Get Software**, et prenez la version Windows 64 bits (`STM32CubePrg-W64`). Le site de ST demande un compte myST gratuit, ou une adresse e-mail à laquelle il envoie le lien. Si votre encadrant vous fournit l'installateur, utilisez celui-là.
2. Décompressez l'archive téléchargée, lancez `SetupSTM32CubeProgrammer_win64.exe` (droits administrateur), gardez le dossier proposé : `C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\`.
3. Acceptez l'installation des **pilotes ST-LINK** quand l'installateur la propose : sans eux, Windows ne reconnaît pas la sonde de la carte.

Vérification (le chemin entre guillemets marche même sans `PATH`) :

```powershell
& "C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin\STM32_Programmer_CLI.exe" --version
```

Réponse attendue : une ligne `STM32CubeProgrammer version: 2.x.y`.

Si vous avez déjà **STM32CubeIDE** (dans `C:\ST\STM32CubeIDE_<version>\`) ou **STM32CubeCLT** (`C:\ST\STM32CubeCLT_<version>\`), ils contiennent un compilateur et un `STM32_Programmer_CLI` que CAN Lab trouve aussi : rien d'autre à installer.

## 4. Où CAN Lab cherche les outils, et le `PATH`

Depuis la version 0.3.1, le helper trouve les outils installés aux **emplacements par défaut** sans aucune configuration. Pour chaque outil, il prend le premier trouvé dans cet ordre :

1. une variable d'environnement qui donne le chemin exact de l'exécutable : `STM32LAB_GCC` (pour `arm-none-eabi-gcc.exe`), `STM32LAB_PROGRAMMER` (pour `STM32_Programmer_CLI.exe`) ;
2. les outils STM32Cube de VS Code (`%LOCALAPPDATA%\stm32cube\bundles`) ;
3. `C:\ST\STM32CubeCLT_*`, puis les outils intégrés à `C:\ST\STM32CubeIDE_*` ;
4. le `PATH` de CAN Lab, puis le `PATH` tel qu'il est enregistré dans Windows (une modification du `PATH` est donc vue sans relancer CAN Lab) ;
5. les dossiers par défaut des installateurs : `Program Files` et `Program Files (x86)`, `Arm GNU Toolchain arm-none-eabi\<version>\bin` (la version la plus récente) et `STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin`.

Le `PATH` n'est donc utile que pour un outil installé ailleurs (archive `.zip`, dossier personnalisé). Pour y ajouter un dossier :

- **par les menus** : touche Windows, tapez « variables d'environnement », ouvrez **Modifier les variables d'environnement pour votre compte**, sélectionnez **Path** dans le cadre du haut, **Modifier**, **Nouveau**, collez le dossier `bin` de l'outil (par exemple `C:\Program Files (x86)\Arm GNU Toolchain arm-none-eabi\14.3 rel1\bin`), puis **OK** dans les deux fenêtres ;
- **par PowerShell** (remplacez le dossier par le vôtre ; la commande ajoute le dossier au `PATH` de votre compte, sans droits administrateur) :

```powershell
$bin = "C:\Users\<vous>\arm-gnu-toolchain\bin"
[Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";" + $bin, "User")
```

Ouvrez ensuite un **nouveau** PowerShell pour vérifier : une fenêtre déjà ouverte garde l'ancien `PATH`. Python, lui, est lancé par CAN Lab avec le `PATH` du démarrage de l'application : après avoir installé Python ou modifié son `PATH`, **relancez CAN Lab**.

## 5. Diagnostic

Dans PowerShell, depuis le dossier de CAN Lab. Chaque commande dit où est l'outil que Windows trouve, ou rien s'il ne le trouve pas :

```powershell
python --version
where.exe python
where.exe arm-none-eabi-gcc
where.exe STM32_Programmer_CLI
```

(`where.exe`, avec `.exe` : dans PowerShell, `where` seul est une autre commande.)

Ce que **CAN Lab** trouve réellement, avec la même recherche que **Compiler** et **Flasher** (section 4) :

```powershell
python .\tools\stm32\student_ide.py inventory
```

La réponse est une ligne JSON. `"gcc": {"available": true, "path": "…arm-none-eabi-gcc.exe"}` et `"programmer": {"available": true, "path": "…STM32_Programmer_CLI.exe"}` : les deux outils sont trouvés. `"available": false` : installez l'outil (sections 2 et 3). Carte branchée, la liste `"probes"` montre la sonde ST-LINK, avec `"board": "NUCLEO-H753ZI"` ; une liste vide, carte branchée : câble USB de charge seule, autre port USB, ou pilote ST-LINK absent (réinstallez STM32CubeProgrammer en acceptant les pilotes). `"error": "No module named 'serial'"` : refaites la dernière commande de la section 1.

La sonde vue par STM32CubeProgrammer lui-même :

```powershell
& "C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin\STM32_Programmer_CLI.exe" -l st
```

Réponse attendue, carte branchée : un bloc `ST-Link Probe 0 :` avec `ST-LINK SN`, `ST-LINK FW` et `Board Name : NUCLEO-H753ZI`. Fermez CAN Lab avant cette commande si la carte y est connectée.

## 6. Messages de Code STM32

| Message | Cause | Que faire |
|---|---|---|
| « Python introuvable : installez Python 3.11 ou plus récent… » | `python` n'est pas dans le `PATH` de CAN Lab, ou seul l'alias du Microsoft Store répond | Section 1, puis **fermez et relancez CAN Lab** |
| « Python 3.x trop ancien… » | la commande `python` lance une version antérieure à 3.11 | Installez une version récente (section 1) ; `where.exe python` montre laquelle est trouvée en premier |
| « Compilateur arm-none-eabi-gcc introuvable… » | aucun compilateur aux emplacements de la section 4 | Section 2, puis **Compiler** de nouveau |
| « STM32_Programmer_CLI introuvable… » | aucun STM32CubeProgrammer aux emplacements de la section 4 | Section 3, puis **Flasher** de nouveau |
| « L'outil de compilation (Python) s'est arrêté sans réponse… » | Python a démarré mais le helper n'a pas pu répondre (installation de Python incomplète, archive mal décompressée) | Commande `inventory` de la section 5 : son message d'erreur dit ce qui manque. Vérifiez que `tools\stm32\` est bien à côté de `n7rt-can-lab.exe` |
| « Projet étudiant introuvable : placez le SDK et les outils à côté de l'application. » | l'exécutable a été lancé depuis l'archive sans la décompresser, ou déplacé seul | Décompressez **toute** l'archive (**Extraire tout**) et lancez `n7rt-can-lab.exe` depuis le dossier obtenu |
| « ST-LINK inaccessible ou délai dépassé ; reconnectez la carte. » | sonde débranchée, utilisée par un autre programme (STM32CubeProgrammer ou STM32CubeIDE ouvert), ou pilote absent | Fermez les autres outils ST, rebranchez la carte, vérifiez avec `-l st` (section 5) |
| « Le runtime de la carte diffère du SDK ; seul un encadrant peut le réinstaller. » | la carte n'a pas le runtime N7RT de cette version | Rendez la carte à votre encadrant : vous ne pouvez pas installer le runtime vous-même |

Les erreurs de **votre code C** (point-virgule manquant, variable inconnue…) ne sont pas des problèmes d'installation : elles s'affichent dans **Problèmes** avec le fichier et la ligne.
