# Démarrer ici — CAN Lab 0.3, guide étudiant

Ce guide vous conduit seul, du téléchargement de CAN Lab jusqu'à la remise de votre fiche finale. Il ne suppose aucune connaissance de Git, du bus CAN, du langage C ni des cartes STM32 : chaque notion est expliquée au moment où vous en avez besoin.

Il complète le **tutoriel PDF**, `docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf`, qui reste la référence détaillée, écran par écran, avec ses figures annotées. Ce guide vous dit **dans quel ordre agir** et **que faire quand ça bloque** ; à chaque étape, un renvoi « PDF § n, p. n » mène à la section du tutoriel qui montre l'écran. Le PDF décrit la version 0.3.0 : les différences des versions suivantes sont au [chapitre 13](#13-différences-avec-le-tutoriel-pdf).

Ni ce guide ni l'application ne donnent les réponses de votre mission. Tous les exemples de valeurs (identifiants, bits, mesures) sont **fictifs** et signalés comme tels.

**Comment lire un chapitre.** Chaque chapitre donne, dans cet ordre : **À faire** (les clics indispensables, numérotés), **Vous devez voir** (le résultat qui prouve que l'étape a réussi) et **Si ça ne marche pas** (les erreurs fréquentes, leur cause et la marche à suivre). Ne passez pas au chapitre suivant tant que le résultat attendu n'est pas à l'écran.

## Sommaire

0. [Avant de commencer](#0-avant-de-commencer)
1. [Télécharger et extraire l'archive](#1-télécharger-et-extraire-larchive)
2. [Installer les outils de compilation](#2-installer-les-outils-de-compilation)
3. [Brancher et connecter la carte NUCLEO](#3-brancher-et-connecter-la-carte-nucleo)
4. [Écrire, compiler et flasher votre code](#4-écrire-compiler-et-flasher-votre-code)
5. [Ouvrir votre mission](#5-ouvrir-votre-mission)
6. [RUN du Physical Lab et Start du bus : deux choses différentes](#6-run-du-physical-lab-et-start-du-bus--deux-choses-différentes)
7. [E0, l'essai nominal](#7-e0-lessai-nominal)
8. [E1 à E3, les incidents](#8-e1-à-e3-les-incidents)
9. [E4 et E5, qualification et clôture](#9-e4-et-e5-qualification-et-clôture)
10. [Dépannage par symptôme](#10-dépannage-par-symptôme)
11. [Ce que disent les verdicts, et ce qu'ils ne disent pas](#11-ce-que-disent-les-verdicts-et-ce-quils-ne-disent-pas)
12. [Sauvegarder, reprendre, changer de PC](#12-sauvegarder-reprendre-changer-de-pc)
13. [Différences avec le tutoriel PDF](#13-différences-avec-le-tutoriel-pdf)

## 0. Avant de commencer

| Il vous faut | D'où cela vient |
|---|---|
| Un PC sous **Windows 10 ou 11, 64 bits**, un port USB libre, un accès internet | vous |
| La carte **NUCLEO-H753ZI** de votre poste, **avec le runtime N7RT déjà installé**, et un câble USB de données | votre encadrant ; vous ne pouvez pas installer le runtime vous-même |
| Le **nom de votre mission** : APPS, WSF, SAS ou BPS | votre encadrant. Le choix dans l'application est **définitif** : ne choisissez rien avant de le connaître |
| Le **pack de votre groupe** : un fichier `.n7rtpack` | votre encadrant |
| Le **code de votre groupe** : 16 caractères, `XXXX-XXXX-XXXX-XXXX`, remis à part | votre encadrant |
| Les **valeurs de supervision** de la fiche (quatre nombres) et les consignes éventuelles | le message de lancement de l'E0 de votre encadrant ; elles ne sont ni dans l'application ni dans la bibliothèque |

Vocabulaire minimal, que la suite précise :

| Mot | Sens |
|---|---|
| **Carte** | la NUCLEO-H753ZI : un microcontrôleur STM32 qui exécute votre programme, et sa sonde **ST-LINK** qui permet de le programmer par USB |
| **Runtime N7RT** | le programme installé par l'encadrant sur la carte : il reçoit les trames du véhicule simulé et appelle votre code |
| **Trame** | un message du bus CAN : un **identifiant** (de quoi elle parle), une longueur (**DLC**) et jusqu'à 8 **octets de données** |
| **Session** | l'échange en cours entre l'application et la carte, démarré par **Start**, arrêté par **Stop** |
| **Slot** | une sortie numérotée de votre programme ; l'énoncé dit quelle grandeur va dans quel slot |
| **Flasher** | écrire votre programme compilé dans la mémoire de la carte |
| **Fiche** | le document que vous remplissez dans l'application : votre décodage (E0), puis un diagnostic par incident (E1 à E3) |

## 1. Télécharger et extraire l'archive

Le dépôt GitHub contient le code source de l'application ; **vous n'en avez pas besoin**. Vous téléchargez un seul fichier prêt à l'emploi, sans compte GitHub.

**À faire**

1. Ouvrez <https://github.com/N7-Racing-Team/n7rt-canlab-etudiant/releases/latest>. C'est la page **Releases** du dépôt (depuis la page d'accueil du dépôt : lien **Releases**, à droite). La version la plus récente porte l'étiquette **Latest**.
2. Descendez jusqu'à la rubrique **Assets** de cette version. Elle contient quatre lignes. Téléchargez **`n7rt-canlab-0.3.2-windows-x64.zip`**, puis **`n7rt-canlab-0.3.2-windows-x64.zip.sha256`**. **Ne prenez pas « Source code (zip) » ni « Source code (tar.gz) »** : ce sont les sources, sans l'application compilée.
3. Si votre navigateur dit que le fichier « n'est pas fréquemment téléchargé », choisissez **Conserver**.
4. Vérifiez l'empreinte : elle prouve que le fichier n'est ni tronqué ni modifié. Ouvrez PowerShell dans votre dossier Téléchargements (menu Démarrer, tapez `PowerShell`, Entrée), puis copiez-collez ces trois lignes :

```powershell
cd $HOME\Downloads
$attendu = (Get-Content .\n7rt-canlab-0.3.2-windows-x64.zip.sha256).Split(" ")[0]
(Get-FileHash .\n7rt-canlab-0.3.2-windows-x64.zip -Algorithm SHA256).Hash -eq $attendu
```

5. Clic droit sur le `.zip`, **Extraire tout…**. La fenêtre propose un dossier dans Téléchargements : remplacez-le (bouton **Parcourir…**, ou en tapant le chemin) par un dossier à vous, par exemple `C:\Users\VOTRE_NOM\Documents\CANLab`, puis **Extraire**. Pas dans `Program Files`, ni dans Téléchargements, ni dans un dossier temporaire : le nettoyage automatique de Windows (Assistant Stockage) peut y supprimer des fichiers de l'archive, qui portent tous la date du 01/01/1980. Ce dossier, celui qui contient `n7rt-can-lab.exe`, est appelé « le dossier de CAN Lab » dans la suite.
6. Ouvrez le dossier obtenu : il contient `n7rt-can-lab.exe`, `DEMARRER_ICI.html`, `docs`, `tools`, `library`, `N7RT_CANLab_Student`… **Ne déplacez jamais l'exécutable seul** et ne lancez jamais l'application depuis l'intérieur du `.zip` : elle a besoin de tous ses dossiers voisins.
7. Double-cliquez `n7rt-can-lab.exe`. L'exécutable n'est pas signé : si Windows affiche « Windows a protégé votre ordinateur », cliquez **Informations complémentaires**, puis **Exécuter quand même** (une seule fois).

**Vous devez voir**

- À l'étape 4, PowerShell répond **`True`**.
- À l'étape 7, la fenêtre « N7RT CAN Lab » s'ouvre ; le titre de la barre du haut affiche la version (`v0.3.2`). Au premier lancement, elle s'ouvre sur la page **Code STM32** ; la barre d'activité, tout à gauche, a trois boutons : **Code STM32**, **Cockpit**, **Physical Lab**.

**Si ça ne marche pas**

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| PowerShell répond `False` | téléchargement abîmé, ou fichiers d'une autre version | supprimez les deux fichiers, téléchargez-les de nouveau |
| « Impossible de trouver le chemin… » | le fichier n'est pas dans Téléchargements, ou son nom diffère (autre version, « (1) » ajouté par le navigateur) | dans l'Explorateur, renommez le fichier, ou remplacez le nom dans les commandes |
| La page Code STM32 affiche « Projet étudiant introuvable : placez le SDK et les outils à côté de l'application. » | application lancée depuis le `.zip`, ou exécutable déplacé seul | refaites l'étape 5, lancez l'exécutable depuis le dossier extrait |
| **Compiler** répond « [Errno 2] No such file or directory: … », ou des fichiers de l'archive ont disparu | archive extraite dans Téléchargements ou un dossier temporaire, que le nettoyage automatique de Windows a vidé en partie, ou fichiers déplacés | supprimez ce dossier, extrayez de nouveau toute l'archive dans `Documents` (étape 5) |
| La fenêtre reste blanche | composant d'affichage WebView2 absent, ou une autre fenêtre de CAN Lab déjà ouverte | fermez toutes les fenêtres de CAN Lab et relancez ; si elle reste blanche, installez « Microsoft Edge WebView2 Runtime » depuis le site de Microsoft |

Référence : [PDF §2.1, p. 4](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=4) (contenu de l'archive) et [§2.4, p. 5](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=5) (premier lancement).

## 2. Installer les outils de compilation

Le Cockpit, le Physical Lab et la fiche fonctionnent déjà. Pour **compiler** votre programme et **le flasher** sur la carte, il faut trois outils gratuits, installés une fois par PC, comptez 20 à 30 minutes :

| Outil | Rôle |
|---|---|
| **Python** 3.11 ou plus récent, avec le module **pyserial** | exécute le petit programme (`tools\stm32\student_ide.py`) par lequel CAN Lab pilote les deux outils suivants |
| **Arm GNU Toolchain**, cible `arm-none-eabi` | le compilateur C du microcontrôleur : `arm-none-eabi-gcc` |
| **STM32CubeProgrammer** | le programmateur de la carte : `STM32_Programmer_CLI` ; il installe aussi le pilote USB du ST-LINK |

**STM32CubeIDE n'est pas obligatoire** : la page Code STM32 de CAN Lab le remplace. Si vous l'avez déjà, ses outils intégrés sont trouvés aussi ; ne programmez jamais la carte directement depuis STM32CubeIDE.

### 2.1 Python et pyserial

**À faire**

1. Ouvrez <https://www.python.org/downloads/windows/>, choisissez une version **3.12 ou 3.13** et téléchargez **« Windows installer (64-bit) »**.
2. Lancez-le. **Au premier écran, cochez « Add python.exe to PATH »** (en bas de la fenêtre), puis cliquez **Install Now**. Les droits administrateur ne sont pas nécessaires.
3. Fermez toutes les fenêtres PowerShell, ouvrez-en une nouvelle et vérifiez :

```powershell
python --version
where.exe python
```

4. Placez-vous dans le dossier de CAN Lab et installez pyserial. La première ligne suppose le dossier de l'exemple du chapitre 1 ; sinon, remplacez le chemin entre guillemets par le vôtre (dans l'Explorateur, clic droit sur le dossier, **Copier en tant que chemin d'accès**, puis collez-le après `cd `) :

```powershell
cd "$HOME\Documents\CANLab"
python -m pip install -r .\requirements-student-ide.txt
python -c "import serial; print('pyserial', serial.__version__)"
```

**Vous devez voir** : `Python 3.12.x` (ou 3.13.x) ; `where.exe` affiche en **première** ligne un chemin qui finit par `\Python312\python.exe` (ou `\Python313\python.exe`) ; la dernière commande affiche `pyserial 3.5` ou plus récent.

### 2.2 Arm GNU Toolchain

**À faire**

1. Ouvrez <https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads> (site officiel d'Arm).
2. Dans la rubrique **Windows (mingw-w64-x86_64) hosted cross toolchains**, partie **AArch32 bare-metal target (arm-none-eabi)**, téléchargez le fichier **`.exe`**, par exemple `arm-gnu-toolchain-14.3.rel1-mingw-w64-x86_64-arm-none-eabi.exe`. Ne prenez ni `aarch64-none-elf`, ni `arm-none-linux-gnueabihf`, ni une version pour macOS ou Linux.
3. Lancez-le (Windows demande les droits administrateur) et **gardez le dossier proposé**, de la forme `C:\Program Files (x86)\Arm GNU Toolchain arm-none-eabi\14.3 rel1\`.
4. Au dernier écran, **cochez « Add path to environment variable »**, puis **Finish**.
5. Fermez et rouvrez PowerShell, puis vérifiez :

```powershell
where.exe arm-none-eabi-gcc
arm-none-eabi-gcc --version
```

**Vous devez voir** : un chemin qui finit par `\bin\arm-none-eabi-gcc.exe`, puis une première ligne de la forme `arm-none-eabi-gcc.exe (Arm GNU Toolchain 14.3.Rel1 …) 14.3.1 …`. Toute version 11 ou plus récente convient ; la 14.3 a été vérifiée avec CAN Lab.

### 2.3 STM32CubeProgrammer

**À faire**

1. Ouvrez <https://www.st.com/en/development-tools/stm32cubeprog.html> (site officiel de STMicroelectronics), cliquez **Get Software**, puis la ligne **STM32CubePrg-W64** (Windows 64 bits). ST demande de créer un compte myST gratuit ou de donner une adresse e-mail pour recevoir le lien. Si votre encadrant fournit l'installateur, prenez le sien.
2. Décompressez le fichier téléchargé, lancez **`SetupSTM32CubeProgrammer_win64.exe`** (droits administrateur) et **gardez le dossier proposé** : `C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\`.
3. **Acceptez l'installation des pilotes ST-LINK** quand l'installateur la propose : sans eux, Windows ne reconnaît pas la carte.
4. Cet installateur n'ajoute rien au PATH : ajoutez-y le dossier `C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin` (section 2.4), puis, dans un nouveau PowerShell :

```powershell
where.exe STM32_Programmer_CLI
STM32_Programmer_CLI --version
```

**Vous devez voir** : un chemin qui finit par `\bin\STM32_Programmer_CLI.exe`, puis une ligne `STM32CubeProgrammer version: 2.x.y` (2.23.0 a été vérifiée).

### 2.4 Comprendre et régler le PATH

Quand vous tapez `python` ou `arm-none-eabi-gcc`, Windows ne fouille pas tout le disque : il cherche un fichier de ce nom **dans une liste de dossiers**, la variable d'environnement **PATH**, dans l'ordre de la liste. Un programme installé dans un dossier absent de cette liste « n'existe pas » pour PowerShell, même s'il est bien sur le disque. C'est pour cela que les installateurs proposent une case « Add … to PATH ».

Le PATH est une **liste partagée** par tous vos programmes. On y **ajoute** un dossier ; on ne remplace jamais son contenu.

**À faire : ajouter un dossier au PATH, par les menus (méthode conseillée)**

1. Touche Windows, tapez `variables d'environnement`, ouvrez **Modifier les variables d'environnement pour votre compte**.
2. Dans le cadre du haut, « Variables utilisateur », sélectionnez la ligne **Path**, puis **Modifier…**. La fenêtre qui s'ouvre montre **un dossier par ligne**.
3. Cliquez **Nouveau**, collez le dossier, par exemple `C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin`, puis Entrée. Ne modifiez ni ne supprimez les autres lignes.
4. **OK** dans cette fenêtre, puis **OK** dans la précédente.

S'il n'y a pas de ligne Path dans le cadre du haut, cliquez **Nouvelle…** sous ce cadre : nom `Path`, valeur le dossier, **OK**.

**À faire : la même chose en PowerShell** (le dossier est **ajouté** à la fin de votre PATH, sans droits administrateur ; remplacez-le par le vôtre) :

```powershell
$dossier = "C:\Program Files\STMicroelectronics\STM32Cube\STM32CubeProgrammer\bin"
$actuel = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$actuel;$dossier", "User")
```

> **Attention** : n'utilisez jamais `setx PATH "%PATH%;…"`. Cette commande tronque le PATH à 1024 caractères et recopie le PATH du système dans le vôtre : des programmes cessent de fonctionner.

**Pourquoi rouvrir PowerShell, puis CAN Lab.** Un programme reçoit une copie du PATH **au moment où il démarre** ; il ne voit pas les modifications faites ensuite. Après chaque installation ou modification du PATH : fermez **toutes** les fenêtres PowerShell et ouvrez-en une nouvelle pour vérifier, puis **fermez et relancez CAN Lab**.

**Vérifier avec `where.exe`.** Dans PowerShell, tapez bien `where.exe`, avec `.exe` : `where` seul est une autre commande. `where.exe` affiche **tous** les dossiers du PATH qui contiennent l'outil, dans l'ordre où Windows les essaie ; c'est la **première ligne** qui sera utilisée. Aucune ligne et le message « Impossible de trouver des fichiers… » : le dossier de l'outil n'est pas dans le PATH.

À savoir : depuis la version 0.3.1, CAN Lab trouve aussi le compilateur et STM32CubeProgrammer s'ils sont installés **dans leur dossier par défaut**, même hors du PATH. Le PATH reste nécessaire pour Python, et il rend les vérifications ci-dessus possibles.

### 2.5 Vérification d'ensemble

**À faire** : dans PowerShell, placé dans le dossier extrait de CAN Lab (comme en 2.1, étape 4), tapez :

```powershell
python .\tools\stm32\student_ide.py inventory
```

**Vous devez voir** une ligne qui commence par `{"ok": true`, avec `"gcc": {"available": true, …}` et `"programmer": {"available": true, …}`. C'est exactement la recherche que fait CAN Lab avant **Compiler** et **Flasher**. Si la carte est branchée, `"probes"` cite sa sonde, avec `"board": "NUCLEO-H753ZI"`.

**Si ça ne marche pas**

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| `python --version` ouvre le Microsoft Store, ou répond « Python est introuvable » | Python absent du PATH : c'est un raccourci de Windows qui répond | réinstallez Python en cochant « Add python.exe to PATH » ; ou désactivez **Paramètres → Applications → Paramètres avancés des applications → Alias d'exécution d'application**, lignes « python.exe » et « python3.exe » |
| `Python 3.10` ou plus ancien | une ancienne version passe en premier dans le PATH | `where.exe python` montre laquelle ; installez 3.12 ou 3.13 et placez-la en tête (fenêtre Path, bouton **Monter**) |
| `"gcc": {"available": false` | compilateur absent, ou installé dans un dossier choisi par vous et absent du PATH | section 2.2 ; ou ajoutez son dossier `bin` au PATH (2.4) |
| `"programmer": {"available": false` | STM32CubeProgrammer absent | section 2.3 |
| `"error": "No module named 'serial'"` | pyserial absent pour ce Python | refaites l'étape 4 de la section 2.1 |
| `"probes": []`, carte branchée | câble de charge seule, autre connecteur que CN1, ou pilote ST-LINK absent | chapitre 3 ; réinstallez STM32CubeProgrammer en acceptant les pilotes |
| Pas de droits administrateur (poste géré) | les installateurs d'Arm et de ST les demandent | demandez l'installation au service informatique ou à votre encadrant |

Pas à pas complet, avec la table des messages de Code STM32 : [docs/stm32-toolchain-windows.md](docs/stm32-toolchain-windows.md). Référence : [PDF §2.2, p. 4](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=4).

## 3. Brancher et connecter la carte NUCLEO

**À faire**

1. Branchez la NUCLEO-H753ZI par le connecteur USB de son **ST-LINK, CN1**, au milieu du petit côté de la carte, et **pas** par l'autre connecteur USB. Utilisez un câble de données (un câble de charge seule ne transmet rien).
2. Identifiez le port COM de la carte : clic droit sur le bouton Démarrer, **Gestionnaire de périphériques**, rubrique **Ports (COM et LPT)** : la ligne **STMicroelectronics STLink Virtual COM Port (COMn)** donne son numéro.
3. Dans CAN Lab, regardez la barre du haut, liste **CARTE**. Avec une seule carte branchée, l'application s'y connecte d'elle-même. Sinon, choisissez la ligne `COMn · STLINK-V3E · …` (le port de l'étape 2, puis le numéro de série de la sonde), puis cliquez **Connecter**. Le bouton aux deux flèches circulaires, à droite de la liste, relit la liste des ports.
4. Attendez quelques secondes.

**Vous devez voir** : le voyant d'état, en haut à droite, passe par « Poignée de main », puis **PRÊTE** (en vert). Le message sous la barre d'outils dit « Carte prête : runtime … vérifié, code étudiant chargé (CRC …). » Dans le Cockpit, la section **Carte** du panneau latéral affiche « Intégrité : défi SHA-256 vérifié » en vert.

**États de la carte et que faire**

| Voyant | Ce qui se passe | Que faire |
|---|---|---|
| « Déconnecté » | aucune liaison ouverte | choisissez la carte, **Connecter** |
| « Carte absente » | la carte choisie n'est pas détectée | câble sur CN1 ? autre port USB ; l'application réessaie seule |
| « Port occupé » | un autre programme a ouvert le port : console série, STM32CubeIDE, **seconde fenêtre de CAN Lab** | fermez-le ; la connexion reprend seule |
| « Poignée de main » qui dure | le runtime ne répond pas | débranchez et rebranchez la carte |
| « Pas de code étudiant » | runtime vérifié, mais aucun programme étudiant valide : **Start** reste grisé | normal pour une carte neuve : chapitre 4, **Compiler + flasher** avec le gabarit |
| « Code étudiant désactivé » | votre code a provoqué une faute | corrigez, recompilez, reflashez ; **Start** le réactive aussi |
| « Carte incompatible », ou « NON vérifié » à la ligne Intégrité | le runtime de la carte ne correspond pas à l'application | rendez la carte à l'encadrant : vous ne pouvez pas le réinstaller |
| « Liaison perdue » | câble débranché | rebranchez : la reconnexion est automatique |
| Liste CARTE : « Aucun port série détecté » | carte non reconnue par Windows | câble, connecteur CN1, pilote ST-LINK (2.3), puis le bouton aux deux flèches |

Référence : [PDF §2.3 à §2.6, p. 5 à 7](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=5).

## 4. Écrire, compiler et flasher votre code

Vous ne modifiez que deux fichiers, `student_decoder.c` et `student_decoder.h`, dans la page **Code STM32** (premier bouton de la barre d'activité). Ils sont enregistrés dans le profil de l'application sur ce PC, pas dans le dossier extrait. Le runtime appelle trois fonctions de votre fichier `.c` :

| Fonction | Quand le runtime l'appelle | Ce que vous y faites |
|---|---|---|
| `student_init()` | au démarrage de la carte et à **chaque Start** | remettre vos variables à zéro |
| `student_on_frame(frame)` | à **chaque trame** reçue | lire `frame->id`, `frame->len`, `frame->data[0]`, `frame->data[1]`… : filtrer, contrôler, décoder, mémoriser |
| `student_on_tick(now_us)` | à **chaque tick** (10 ms par défaut) | publier chaque slot : `n7rt_publish(slot, valeur)` ou `n7rt_reject(slot)` |

Le gabarit fourni ne décode **aucune** mission : il publie la longueur de chaque trame dans le slot 0 et son premier octet dans le slot 1. Il sert à vérifier que toute la chaîne fonctionne.

### 4.1 Le cycle : modifier, enregistrer, compiler, flasher, contrôler

**À faire**

1. Cliquez le fichier dans l'explorateur, à gauche, et modifiez-le dans l'éditeur. Un point à côté du nom signale une modification non enregistrée.
2. **Enregistrer** (ou Ctrl+S).
3. **Compiler**. Lisez l'onglet **Console**, sous l'éditeur.
4. Dans le Cockpit, la carte doit être **Prête** et la session **arrêtée** (**Stop**) : on ne flashe jamais pendant une session.
5. **Flasher** (ou directement **Compiler + flasher**). Ne débranchez pas la carte : la ligne d'état affiche « Programmation de la zone étudiante… », puis « Reconnexion et vérification de la carte… » ; la carte redémarre et l'application se reconnecte seule.
6. Dans le panneau de droite, comparez **CRC compilé** (rubrique Compilation) et **CRC étudiant** (rubrique Carte connectée).

**Vous devez voir** : après l'étape 3, la console finit par `CRC étudiant 0x… · … octets de flash · … octets de RAM` et **Flasher** devient actif. Après l'étape 6, les deux CRC sont **égaux** et la phrase **« La carte exécute la compilation actuelle. »** s'affiche. Le même CRC apparaît dans la barre d'état, en bas de la fenêtre.

Le CRC est l'empreinte de votre programme : le même code redonne le même CRC, la moindre modification le change. Deux CRC différents signifient que la carte n'exécute pas ce que vous avez sous les yeux. Ce CRC n'a rien à voir avec le CRC qui protège certaines trames de votre mission.

### 4.2 Lire une erreur de compilation

**À faire** (exercice sans risque) : ajoutez la ligne `#error N7RT_TEST_DIAGNOSTIC` à la fin de `student_decoder.c`, cliquez **Compiler**, ouvrez l'onglet **Problèmes**, cliquez la ligne d'erreur. Puis retirez la ligne, enregistrez et recompilez.

**Vous devez voir** : le bandeau rouge « Compilation échouée. Corrigez les problèmes indiqués. » ; dans **Problèmes**, une ligne de la forme `student_decoder.c:33:2 · error · #error N7RT_TEST_DIAGNOSTIC` (fichier, ligne, colonne, gravité, message) ; un clic place le curseur sur la ligne en cause. Corrigez toujours **la première erreur d'abord** : les suivantes en découlent souvent.

Un bandeau rouge qui **nomme un outil** (« Compilateur arm-none-eabi-gcc introuvable… », « Python introuvable… ») ne concerne pas votre code : c'est l'installation, chapitre 2.

### 4.3 Test de démonstration, sans mission

Ce test vérifie la chaîne complète (PC → carte → votre code → PC) **avant** d'ouvrir votre mission. Faites-le mission **non ouverte** : au tout début, ou après **Fermer** dans la section Mission. Sans mission, le bus est celui de l'**ECU virtuel** du SDK, une source de démonstration qui n'appartient à aucune mission.

**À faire**

1. Page Code STM32 : avec le gabarit tel quel, **Compiler + flasher**, puis vérifiez « La carte exécute la compilation actuelle. »
2. Passez au **Cockpit** (deuxième bouton de la barre d'activité). La section **Stimuli — ECU virtuel** du panneau latéral règle cette source : Potentiomètre 128, Identifiant `0x123`, DLC 8, Cadence 10 trames/s.
3. Cliquez **Start** dans la barre du haut.
4. Lisez le tableau **Trames reçues par le nœud étudiant**, puis cliquez une ligne pour l'afficher dans **Trame sélectionnée**.
5. Déplacez le curseur **Potentiomètre**, puis cochez **Bouton appuyé**, et regardez ce qui change.
6. Cliquez **Stop**.

**Vous devez voir**, pour chaque trame :

| Colonne | Valeur | Ce que c'est |
|---|---|---|
| ID | `0x123` | l'identifiant de la trame, en hexadécimal : il dit « de quoi parle la trame » |
| DLC | `8` | le nombre d'octets de données |
| Données | `80 00 14 03 04 05 06 07` | les 8 octets, `data[0]` à gauche, en hexadécimal : deux chiffres par octet, de `00` à `FF` (0 à 255) |
| Résultat | `OK` | la carte a livré la trame à votre code et renvoyé sa réponse |
| Publications | `s0=8 · s1=128` | ce que votre code a publié en traitant cette trame |

Dans cette source de démonstration, `data[0]` est le potentiomètre (`80` en hexadécimal vaut 128), `data[1]` le bouton (`00` relâché, `01` appuyé), `data[2]` un compteur qui augmente à chaque trame, la suite des octets de remplissage. Le gabarit publie `frame->len` dans **s0** (8) et `frame->data[0]` dans **s1** (128) : à l'étape 5, **s1** suit le potentiomètre, et `data[1]` passe à `01` quand le bouton est coché. Le panneau **Résultats étudiants**, à droite, garde la dernière valeur de chaque slot ; sa colonne Source dit « trame … ».

**Exercice facultatif (exemple fictif d'entraînement, à retirer avant votre mission)** : dans `student_on_frame`, après le bloc `if … else` existant, ajoutez :

```c
  if (frame->len > 1u) {
    n7rt_publish(2u, (float)frame->data[1]);   /* entraînement : slot 2 = octet 1 de la trame */
  }
```

Compilez, flashez, contrôlez le CRC, **Start** : **s2** vaut 0, puis 1 quand **Bouton appuyé** est coché. Le CRC a changé, puisque le code a changé.

Pour convertir de l'hexadécimal : la Calculatrice de Windows, mode **Programmeur**.

### 4.4 Si ça ne marche pas

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| **Flasher** grisé | survolez-le : la raison s'affiche (« Compilez les modifications avant de flasher. », « Arrêtez la session… », « La carte doit être prête avant le flash. ») | compilez ; **Stop** ; attendez **Prête** |
| « Le code sur la carte diffère de la source actuelle. » | la carte exécute une autre version que votre source | **Compiler + flasher**, puis recomparez les CRC |
| « Connectez la carte pour vérifier le code chargé. » alors que la carte est Prête | après un redémarrage de CAN Lab, aucune compilation n'a encore eu lieu : il n'y a rien à comparer | cliquez **Compiler** (la carte n'est pas reprogrammée) : la comparaison s'affiche |
| « Programmation ou vérification échouée. Consultez la console. », ou un message ST-LINK | carte débranchée pendant le flash, port tenu par un autre programme | lisez la Console ; débranchez et rebranchez la carte, attendez **Prête**, recommencez |
| Le bandeau nomme un outil ou Python | installation incomplète | chapitre 2 |
| **s1** ne suit pas le potentiomètre au test de démonstration | la carte n'exécute pas le gabarit, ou la session n'est pas démarrée | comparez les CRC (4.1) ; cliquez **Start** |
| Restaurer le point de départ | **Restaurer le template** remplace vos deux fichiers par le gabarit | copiez d'abord votre code ailleurs ; puis recompilez et reflashez |

Références : [PDF §10.1 à §10.5, p. 47 à 51](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=47) (page de code, boutons, erreur, flash et CRC, gabarit), [§8.1 à §8.3, p. 33 à 35](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=33) (session, tableau, résultats).

## 5. Ouvrir votre mission

Tout se passe dans le **Cockpit**, panneau latéral, section **Mission**. Trois temps : choisir la mission (une fois pour toutes sur ce PC), importer le pack (une fois), saisir le code du groupe (**à chaque ouverture**).

**À faire, la première fois**

1. Dans la liste des missions, sélectionnez **celle que votre encadrant vous a attribuée** : « APPS — pédale d'accélérateur », « WSF — vitesse des roues », « SAS — angle du volant » ou « BPS — pression de freinage ».
2. **Choisir cette mission**, lisez l'avertissement, puis **Confirmer**. Ce choix est **définitif sur ce PC** : seul l'encadrant peut l'effacer.
3. **Importer le pack du groupe…** et sélectionnez votre fichier `.n7rtpack`. L'application en garde une copie : vous ne l'importez qu'une fois.
4. Tapez le **code du groupe** dans **Code du groupe** (tirets, espaces et minuscules acceptés ; l'icône en forme d'œil affiche les caractères), puis **Ouvrir** ou Entrée.

**Vous devez voir** : la ligne « Pack » affiche le nom de votre groupe et sa cohorte ; puis, après **Ouvrir**, un panneau de mission en tête du panneau latéral, titré « Mission … — groupe … », avec la progression (**E0 Essai nominal : EN COURS**, les autres « À FAIRE », la ligne « Fiche : En cours de rédaction »), la consigne, **Ouvrir la fiche** et **Lancer la vérification**. La section **Stimuli — ECU virtuel** disparaît, la liste **Tick** se grise, la barre d'état affiche l'étape en cours et le Physical Lab charge le banc de votre mission.

**À faire, après un redémarrage du PC ou de CAN Lab**

Au redémarrage, la mission est **fermée**, pas perdue. Les lignes « Mission » et « Pack » sont remplies, le champ Code du groupe est vide, et la section **Stimuli — ECU virtuel** est revenue : tant que vous n'avez pas rouvert la mission, **Start** fait tourner le bus de démonstration (trames `0x123`), pas le véhicule de votre mission.

1. Vérifiez que la carte est **Prête** (chapitre 3).
2. Tapez le code du groupe, **Ouvrir**.
3. Contrôlez les signes d'une mission ouverte (paragraphe « Vous devez voir » ci-dessus) : votre progression, votre fiche, votre banc et l'anomalie d'un incident en cours sont repris tels qu'ils étaient enregistrés.
4. Code STM32 : **Compiler**, puis contrôlez « La carte exécute la compilation actuelle. » (chapitre 4.4).

**Si ça ne marche pas**

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| « Code de groupe incorrect. » | faute de frappe, confusion 0/O ou 1/I, code d'un autre groupe | retapez les 16 caractères tels que remis ; si le code est perdu, demandez-le à l'encadrant |
| **Ouvrir** grisé | survolez-le : « Saisissez le code du groupe. », « Importez d'abord le pack du groupe. » | faites ce qu'il dit |
| « Ce fichier n'est pas un pack de groupe N7RT. » | vous avez choisi un autre fichier | sélectionnez le `.n7rtpack` |
| « Ce pack est destiné à une autre mission. » | pack d'une autre mission que celle confirmée | vérifiez avec l'encadrant le fichier et la mission |
| « Pack du groupe illisible… », « Fichier trop volumineux pour un pack de groupe. » | fichier abîmé | demandez un nouveau pack à l'encadrant |
| « La mission de cette session est déjà choisie : … » | une autre mission a déjà été confirmée sur ce PC | demandez à l'encadrant |
| « Progression illisible sur ce poste : … » | données locales endommagées | prévenez l'encadrant ; ne supprimez rien |
| Vous ne voyez que des trames `0x123` | la mission n'est pas ouverte : c'est l'ECU virtuel | saisissez le code du groupe, **Ouvrir** |

Ne communiquez ni votre pack ni votre code à un autre groupe : chaque groupe a sa propre variante du véhicule, et les réponses d'un autre groupe sont fausses pour le vôtre. Le bouton **Mode encadrant** est réservé à l'encadrant.

Références : [PDF §3 et §3.1, p. 8 à 11](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=8), [§5, p. 15](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=15) (énoncé).

## 6. RUN du Physical Lab et Start du bus : deux choses différentes

Deux paires de boutons se ressemblent et ne font pas la même chose.

| | **Start / Stop** | **RUN / STOP / RESET** |
|---|---|---|
| Où | barre du haut, identique dans les trois vues | barre d'outils du schéma, dans le **Physical Lab** |
| Ce qu'ils pilotent | la **session** avec la carte : les trames partent vers la carte, ses résultats reviennent | la **simulation du circuit** du Physical Lab : le temps simulé du schéma |
| Effet sur votre code | **Start** rappelle `student_init()` : vos variables reprennent leur valeur initiale | aucun |
| Nécessaires pour | voir des trames et des résultats, consigner une mesure, toute vérification | rien en mission : la session fait avancer le banc |

**En mission, c'est Start qui fait rouler le véhicule.** Au **Start**, l'application arrête et remet à zéro la simulation du banc (vos réglages, curseurs et interrupteurs sont conservés), puis fait avancer le temps du banc **au rythme du bus** : l'ECU de votre mission lit ses capteurs sur le banc et émet ses trames vers votre carte. Pendant la session, RUN, STOP et RESET sont refusés : « Horloge du banc pilotée par la séance CAN : arrêtez la séance avant cette commande. » Le bouton RUN seul ne fait pas rouler le véhicule : sans **Start**, aucune trame, aucun résultat, aucune consignation.

**Mesurez pendant une session.** Le multimètre lit la moyenne des 100 dernières trames de simulation du banc : en mission, relevez une tension **session démarrée**, pendant que le véhicule roule, dans les conditions que vous voulez observer.

**Après Stop, les trames affichées sont un historique.** Le tableau garde les dernières trames reçues et « Résultats étudiants » garde la dernière valeur de chaque slot, mais plus rien n'arrive. Ne relevez rien sur un historique figé : relancez **Start**. Au **Start** suivant, une nouvelle session commence ; les trames de la session précédente restent un moment dans le tableau, **grisées**, et la mention « lignes grisées : session précédente » s'affiche au-dessus du tableau. Quand la fenêtre est large, la colonne **Session** montre aussi l'identifiant de session de chaque trame. Ne mesurez que sur les lignes normales.

**Le banc doit rester celui de la mission.** Dans le Physical Lab, **Nouveau**, **Ouvrir** et **Charger** (rubrique Fichier) remplacent le banc de la mission par un autre document. Consigner, reproduire l'anomalie et vérifier sont alors refusés : « Le banc de la mission n'est plus ouvert dans le Physical Lab : … ». Pour retrouver votre banc tel que vous l'aviez laissé : Cockpit, section Mission, **Fermer**, puis code du groupe et **Ouvrir**. Le bandeau « Banc de la mission — mode mission : les grandeurs internes ne sont pas affichées. », au-dessus du schéma, confirme que vous êtes sur le bon banc.

Références : [PDF §8.1, p. 33](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=33) (Start et Stop), [§9.2, p. 38](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=38) (RUN, STOP, RESET), [§9.6, p. 43](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=43) (revenir au banc de la mission).

## 7. E0, l'essai nominal

Consigne : identifier les trames de votre ECU sur le bus, les décoder, programmer la carte et documenter votre décodage dans la partie nominale de la fiche. Mission ouverte, carte **Prête**.

**À faire**

1. **Lire l'énoncé.** Panneau de la mission, sous la progression (faites défiler) : le résumé (le rôle de votre carte, ce qu'elle reçoit et publie), les **Exigences fonctionnelles** (seuils, délais, conduite en cas de défaut) et les **Sorties à publier**, une ligne par sortie : `Slot n — code · libellé (unité)`. Notez chaque numéro de slot et son unité.
2. **Identifier l'ECU.** Le résumé dit quel calculateur émet les trames que vous décodez. **Ouvrir le banc** (sous les Sorties à publier) affiche le banc dans le Physical Lab : avec l'outil **Sélection**, cliquez l'ECU et chaque capteur ; l'**Inspecteur**, à droite, donne leur désignateur, leur modèle et leurs broches.
3. **Explorer le banc sans le modifier.** Lisez les rubriques « Paramètres » sans les changer : chaque vérification compare votre banc au banc de référence. Les curseurs, interrupteurs et commandes (rubrique « Commandes ») sont libres. Relevez le **marquage** écrit sur le symbole de chaque capteur : il vous servira à trouver sa fiche technique.
4. **Mesurer au multimètre.** **Start** (chapitre 6), puis dans le Physical Lab, rubrique Multimètre : **Fonction** « V continu », **COM** (sonde noire) sur la masse de l'ECU, **VΩ** (sonde rouge) sur la broche à mesurer. Déplacez une commande et observez la tension.
5. **Isoler les trames.** Session démarrée, déplacez la commande qui fait varier la grandeur de votre ECU, et repérez dans le Cockpit l'identifiant dont les **Données** changent. Tapez-le dans **Filtre ID (hex)** pour ne garder que lui. Cliquez une trame : **Trame sélectionnée** donne son détail. Pour faire apparaître la colonne **t (ms)**, agrandissez la fenêtre ou masquez le panneau latéral (boutons en haut à droite) : l'écart entre les **t (ms)** de deux trames successives donne la période ; moyennez sur plusieurs trames. Repérez aussi les champs qui ne bougent pas avec la grandeur : compteur, statut, protection.
6. **Chercher dans la bibliothèque.** Panneau de la mission, **Bibliothèque** : chaque document s'ouvre d'un clic (dans votre navigateur ; connexion internet nécessaire). Cherchez (Ctrl+F) le marquage relevé à l'étape 3 : certaines fiches décrivent un autre capteur que celui du banc. Pour la protection des trames, servez-vous des spécifications ; pour les seuils et délais, des Exigences fonctionnelles, puis du règlement.
7. **Remplir les sept sections de la fiche.** **Ouvrir la fiche** : elle s'ouvre dans l'onglet **Fiche** du dock du bas. Index 01 à 07 : Documents utilisés, Trames, Signaux, Statuts, Supervision de la trame de données, Décisions de conception, Justification. **Ajouter …** crée un bloc. Les quatre valeurs de **Supervision** sont celles du message de lancement de votre encadrant. Les formats exacts de chaque champ sont dans le [PDF §7.3, p. 25 à 30](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=25), avec un exemple fictif complet.
8. **Enregistrer la fiche.** Rien ne s'enregistre tout seul. Enregistrez souvent, même incomplète.
9. **Coder.** Dans `student_on_frame` : ignorez les trames qui ne vous concernent pas, contrôlez protection, compteur et statut, décodez (valeur brute × facteur + offset), mémorisez la valeur, sa validité et l'instant. Dans `student_on_tick` : **à chaque tick, une entrée par slot de la mission**, `n7rt_publish` ou `n7rt_reject`, selon vos décisions DD-1 à DD-4 et la supervision. Méthodes générales et fragments fictifs : [PDF §10.7, p. 53 à 56](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=53).
10. **Compiler, Stop, flasher, contrôler le CRC** (chapitre 4.1).
11. **Tester.** **Start** ; dans **Résultats étudiants**, chaque slot de la mission affiche son code, son nom et sa valeur avec son unité, et sa source (au survol) doit être **« tick … »**. Faites varier les commandes du banc et vérifiez que vos valeurs suivent. Utilisez `n7rt_log` et l'onglet **Console étudiant** pour suivre votre code (une ligne par bloc de trames, jamais une par trame).
12. **Lancer la vérification.** Le bouton enregistre d'abord la fiche ouverte. Pendant l'essai, ne débranchez pas la carte et ne fermez pas l'application.

**Vous devez voir** : pendant l'essai, « Essai en cours — … s. Ne débranchez pas la carte et ne fermez pas l'application. », bus et résultats masqués. Puis soit « Vérification réussie. Étape validée le … » suivi de « Incident 1 ouvert. Le véhicule revient d'essai avec une anomalie. », soit « Échec à corriger. » avec deux lignes : « Fiche nominale : conforme / non conforme » et « Essai nominal : réussi / non réussi ».

**Si ça ne marche pas**

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| **Lancer la vérification** grisé, avec « Complétez la fiche de cette étape… Champs à renseigner : … » | la liste porte sur la fiche **enregistrée** | remplissez les champs nommés, puis **Enregistrer la fiche** |
| « Corrigez les champs signalés dans la fiche. » | un champ a une forme refusée (encadré rouge) | **Enregistrer la fiche** amène au premier champ fautif ; lisez le message sous le champ |
| « Connectez la carte et vérifiez que votre programme est chargé. » | carte non Prête | chapitre 3, puis chapitre 4 |
| Un slot n'apparaît jamais dans Résultats étudiants | votre code ne publie pas ce numéro | vérifiez le numéro dans le code et dans la fiche |
| La source d'un slot est « trame … » | vous publiez dans `student_on_frame` : la vérification ne le lit pas | publiez dans `student_on_tick` |
| « Fiche nominale : non conforme » | au moins un champ faux | reprenez chaque champ depuis sa source (bus, documents, énoncé, supervision reçue) ; contrôlez la numérotation des bits |
| « Essai nominal : non réussi » | le code ne fait pas ce que dit la fiche, ou la carte n'exécute pas votre dernière compilation | CRC (4.1) ; un slot écrit à chaque tick, dans la bonne unité ; testez vos cas limites |
| « Prochaine vérification possible dans … » | 2 minutes entre deux vérifications | reprenez vos mesures en attendant |

Références : [PDF §5, p. 15](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=15) (énoncé, bibliothèque), [§7, p. 22](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=22) (fiche), [§9.3 et §9.4, p. 39](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=39) (inspecteur, multimètre), [§11, p. 57](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=57) (vérifier une étape).

## 8. E1 à E3, les incidents

Après E0, « le véhicule revient d'essai avec une anomalie ». L'application ne dit jamais ce qui est en cause. Une seule anomalie est active à la fois ; elle survit à la fermeture de l'application.

**À faire, pour chaque incident**

1. **Observer.** Session démarrée : le bus (des trames manquent-elles ? le compteur avance-t-il ? la protection est-elle conforme ? le statut ?), vos Résultats étudiants, le banc et le multimètre.
2. **Formuler un diagnostic.** **Ouvrir la fiche** : la fiche de l'incident a cinq blocs. **Cible** : d'abord son type (« Conducteur (ses deux broches) », « Composant (son désignateur) » ou « Identifiant (trame du bus) »), puis la cible elle-même. **Catégorie** : la famille de la cause ; sa définition s'affiche sous la liste. **État observé** : ce que vous constatez ; les états se lisent **dans l'ordre de la liste**, retenez le premier dont la définition s'applique.
3. **Lire la grandeur imposée.** L'état observé impose la grandeur de la **mesure clé** : « Grandeur imposée : … » s'affiche sous la liste, avec les unités acceptées.
4. **Mesurer pendant l'anomalie.** **Start**, attendez au moins **3 secondes**, relevez la grandeur imposée (au multimètre, ou sur le bus avec Filtre ID et la colonne t (ms)) pendant que l'anomalie se manifeste, **sans toucher aux curseurs**.
5. **Consigner, avant toute réparation.** **Consigner la mesure** (dans la fiche, section Mesure clé, ou sous le multimètre du Physical Lab : même bouton). L'application scelle les trames des dernières secondes, l'état du banc et la lecture du multimètre.
6. **Saisir** la **Valeur** (un nombre, sans unité, virgule ou point) et l'**Unité**, puis **Enregistrer la fiche**.
7. **Réparer** : le circuit (remplacer un fil, rétablir un réglage), votre code, ou les deux. Pour remplacer un fil : outil Sélection, cliquez le fil, notez ses extrémités dans l'Inspecteur, touche **Suppr** ; puis outil **Fil** (touche W), cliquez la première broche, un point libre à chaque coude, puis la seconde broche ([PDF §9.5, p. 40](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=40)). Si vous changez le code : compilez, flashez, contrôlez le CRC.
8. **Vérifier la réparation.**
9. **Réparé trop tôt ?** Si vous avez réparé avant de consigner, ou si votre mesure ne montre plus l'anomalie : **Reproduire l'anomalie** (sous la consigne), confirmez **Reproduire**, puis remesurez, consignez, réparez de nouveau et vérifiez. Vos modifications du circuit sont conservées.

**Vous devez voir** : après l'étape 5, « Mesure consignée le … » ; après l'étape 8, « Vérification réussie. Étape validée le … » et l'incident suivant s'ouvre, ou « Échec à corriger. » avec « Diagnostic (fiche) : conforme / non conforme » et « Réparation (essai caché) : vérifiée / non vérifiée ».

**Si ça ne marche pas**

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| « Aucune trame récente sur le bus : lancez le véhicule, puis consignez la mesure. » | pas de session (RUN ne suffit pas) | **Start**, attendez 3 s, consignez |
| « La mesure ne peut être consignée que pendant l'incident en cours, hors vérification. » | vérification en cours, ou pas d'incident ouvert | attendez la fin de l'essai |
| « Le banc de la mission n'est plus ouvert dans le Physical Lab : … » | Nouveau, Ouvrir ou Charger ont remplacé le banc | chapitre 6 : **Fermer**, code, **Ouvrir** |
| **Vérifier la réparation** grisé, « Mesure clé : consigner la mesure » | rien n'est consigné | étapes 4 et 5 |
| « Diagnostic (fiche) : non conforme » | cible, catégorie, état ou mesure faux, ou mesure prise sans l'anomalie | relisez les définitions ; vérifiez les broches ; remesurez pendant l'anomalie (étape 9) |
| « Réparation (essai caché) : non vérifiée » | l'anomalie n'a pas disparu, un paramètre ou une liaison diffère du banc de référence, ou votre code ne traite pas la situation | refaites la mesure après réparation ; rétablissez les valeurs d'origine ; testez votre code |
| Un fil refusé (« Ce fil traverserait le corps de … ») | le tracé croise un composant, une broche ou un fil | recommencez en cliquant un point libre à chaque coude |

Références : [PDF §7.4, p. 30](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=30) (fiche d'incident, catégories, états, grandeurs), [§9.7 et §9.8, p. 43 et 44](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=43) (consigner, reproduire), [§11.3, p. 59](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=59) (lire le verdict).

## 9. E4 et E5, qualification et clôture

**À faire, E4**

1. Programmez la **version finale** de votre décodeur et contrôlez le CRC (chapitre 4.1).
2. **Lancer la qualification**, carte branchée, application ouverte. Attendez la fin (« Qualification en cours — … s. »).
3. Lisez les **huit lignes de compétence**, chacune PASS ou FAIL :

| Ligne | Compétence |
|---|---|
| CP-1 | Recevoir et filtrer les trames de l'ECU |
| CP-2 | Décoder les valeurs nominales |
| CP-3 | Respecter les limites et les valeurs invalides |
| CP-4 | Contrôler l'intégrité des messages |
| CP-5 | Superviser fraîcheur et séquence |
| CP-6 | Traiter un défaut signalé par la source |
| CP-7 | Appliquer la logique de sécurité de la mission |
| CP-8 | Reprendre après un défaut |

4. Une ligne en **FAIL** suffit pour que E4 passe à « ÉCHEC À CORRIGER ». L'intitulé désigne une **famille de situations** (trames à ignorer, valeurs invalides, protection, fraîcheur et séquence, défaut annoncé par la source, logique de sécurité de l'énoncé, retour à la normale) : reproduisez-la sur le banc et le bus, observez vos résultats et votre console, corrigez le code, **compilez, flashez, contrôlez le CRC**, puis relancez après le délai de **5 minutes**. Les huit compétences sont réévaluées à chaque qualification.

**Vous devez voir** : les huit en PASS, puis « Mission clôturée. » et « La fiche finale est exportée dans le dossier Documents. » ; l'étape passe à **E5 · Clôture**, la ligne Fiche affiche « Exportée le … ».

**À faire, E5**

1. **Ouvrir la fiche finale** : `fiche.html` s'ouvre dans votre navigateur.
2. Retrouvez le dossier exporté : `Documents\N7RT CAN Lab\fiches\`, sous-dossier nommé `mission-groupe-AAAAMMJJ-HHMM` (mission, groupe, instant de la clôture). Si vos Documents sont synchronisés par OneDrive, ce dossier est dans OneDrive.
3. **Ce que vous remettez** : **ce sous-dossier entier, sans rien y modifier** : `fiche.html` (la fiche lisible), `fiche.json` (les mêmes données ; son empreinte SHA-256 figure au pied de `fiche.html`) et le dossier `scellés`. Pour l'envoyer : clic droit sur le sous-dossier, **Compresser dans un fichier ZIP** (Windows 11) ou **Envoyer vers → Dossier compressé** (Windows 10). Remettez-le comme votre encadrant l'indique. Votre code source n'est à joindre que s'il le demande (chapitre 12).

La fiche finale contient vos parties de fiche, vos mesures consignées, les réparations enregistrées, les compétences, **toutes** vos tentatives, vos reproductions d'anomalie et le temps actif. Après la clôture, la fiche est en lecture seule : « La mission est clôturée : la fiche n'est plus modifiable. »

**Si ça ne marche pas**

| Ce que vous voyez | Cause | Que faire |
|---|---|---|
| **Lancer la qualification** grisé | carte non Prête, ou délai de 5 minutes | lisez le message sous le bouton |
| Qualification interrompue | carte débranchée, application fermée | elle n'est pas comptée : relancez-la |
| « Exportation de la fiche finale impossible », bouton « Réessayer l'export » | dossier Documents introuvable ou non inscriptible, disque plein | libérez de la place ou corrigez les droits, **Réessayer l'export** ; sinon prévenez l'encadrant |

Références : [PDF §12, p. 62](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=62) (qualification), [§13, p. 66](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=66) (clôture et export).

## 10. Dépannage par symptôme

| Symptôme | Diagnostic | Marche à suivre |
|---|---|---|
| **Compilateur absent** : « Compilateur arm-none-eabi-gcc introuvable… » | Arm GNU Toolchain non installé, ou installé hors de son dossier par défaut et hors du PATH | 2.2 et 2.4, puis `python .\tools\stm32\student_ide.py inventory` (2.5) |
| **Programmeur absent** : « STM32_Programmer_CLI introuvable… » | STM32CubeProgrammer non installé | 2.3, puis 2.5 |
| **Python absent** : « Python introuvable… » | Python absent du PATH de CAN Lab, ou seul l'alias du Microsoft Store répond | 2.1 ; fermez et relancez CAN Lab |
| « [Errno 2] No such file or directory: … » à la compilation | des fichiers de l'archive manquent (dossier temporaire ou Téléchargements nettoyé par Windows, fichiers déplacés) | extrayez de nouveau toute l'archive dans `Documents` (chapitre 1, étape 5) |
| « L'outil de compilation (Python) s'est arrêté sans réponse… » | Python démarre mais le helper ne répond pas (Python incomplet, dossier `tools` absent) | commande `inventory` (2.5) ; vérifiez que l'archive est entièrement extraite |
| **Port occupé** | un autre programme tient le port série, souvent une **seconde fenêtre de CAN Lab** | fermez-le ; la connexion reprend seule |
| **Carte incompatible** | runtime de la carte différent de celui de l'application | rendez la carte à l'encadrant |
| **CRC différent** : « Le code sur la carte diffère de la source actuelle. » | la carte n'exécute pas votre dernière source | **Compiler + flasher**, puis comparez CRC compilé et CRC étudiant |
| **Bouton grisé** (Start, Flasher, Ouvrir, vérification) | une condition manque | survolez le bouton ou lisez le message dessous : il dit laquelle |
| **Pack refusé** | autre fichier, autre mission, fichier abîmé | chapitre 5, tableau |
| **Code incorrect** : « Code de groupe incorrect. » | faute de frappe ou code d'un autre groupe | chapitre 5 |
| **Fiche non enregistrée** : « Modifications non enregistrées » à côté du bouton | rien ne s'enregistre tout seul | **Enregistrer la fiche** ; à la fermeture, choisissez **Enregistrer et fermer** |
| « Certains champs ne sont pas au bon format… » | un champ mal formé bloque tout l'enregistrement | corrigez le champ encadré en rouge, puis enregistrez |
| **Banc remplacé** : « Le banc de la mission n'est plus ouvert dans le Physical Lab : … » | Nouveau, Ouvrir ou Charger utilisés en mission | **Fermer** la mission, code, **Ouvrir** (chapitre 6) |
| **Session vide** : aucune trame, « Aucune trame pour l'instant : démarrez une session (Start)… » | pas de session, ou filtre trop restrictif | **Start** ; videz **Filtre ID (hex)** |
| Seulement des trames `0x123` | la mission n'est pas ouverte : bus de démonstration | code du groupe, **Ouvrir** (chapitre 5) |
| **Vérification échouée** : « Échec à corriger. » | voir les lignes du verdict | chapitre 11 ; reprenez la démarche : observer, faire une hypothèse, mesurer, corriger une seule chose, vérifier |
| « Vérification interrompue (…). Elle n'est pas comptée. » | carte débranchée ou application fermée pendant l'essai | rebranchez, relancez |
| « Code étudiant désactivé » | votre code a fait une faute (accès hors d'un tableau, boucle infinie) | corrigez, recompilez, reflashez ; Start le réactive |
| Fenêtre blanche, ou noire et figée | seconde fenêtre de CAN Lab ouverte, WebView2 absent, mémoire saturée | fermez toutes les fenêtres de CAN Lab et les applications inutiles, relancez |

Tableau complet du tutoriel : [PDF §14, p. 68](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=68) et [§15, p. 69](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=69).

## 11. Ce que disent les verdicts, et ce qu'ils ne disent pas

- **« OK » dans la colonne Résultat d'une trame** veut dire : la carte a livré la trame à votre code et a renvoyé sa réponse. Cela ne dit **rien** de la protection de bout en bout de la trame (son CRC et son compteur) : la contrôler est le travail de votre code. Une trame « OK » peut porter une protection non conforme.
- **« Enregistré »** veut dire : la fiche est sauvegardée sur ce PC. Ce n'est **pas** « Validé » : seule une vérification réussie fait passer une étape à **VALIDÉ**. Le formulaire contrôle la **forme** des champs (un identifiant hexadécimal, un entier dans sa plage), jamais leur justesse.
- **Une valeur dans Résultats étudiants** ne compte que si elle est publiée à chaque tick (source « tick … ») ; une valeur publiée dans `student_on_frame` s'affiche mais n'est pas lue par la vérification.
- **« La carte exécute la compilation actuelle. »** prouve que la carte exécute votre dernière source, pas que cette source est juste.
- **Après un échec, aucune réponse détaillée n'est donnée** : « Le détail n'est pas communiqué. Reprenez vos mesures et vos essais. » Un verdict désigne une **partie** du travail (fiche, essai, diagnostic, réparation, compétence), jamais le champ, la trame, l'instant ou la valeur en cause. Changer des valeurs au hasard coûte 2 minutes (5 en E4) par essai et n'apprend rien.
- Le texte libre (justifications, pages, noms des signaux) n'entre jamais dans un verdict ; il sert à votre encadrant.
- Toutes les tentatives, réussies ou non, figurent dans la fiche finale.

Référence : [PDF §11.3, p. 59](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=59) et [§14, p. 68](docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf#page=68).

## 12. Sauvegarder, reprendre, changer de PC

| Quoi | Où, sur ce PC | Comment le protéger |
|---|---|---|
| Votre code (`student_decoder.c`, `student_decoder.h`) | `%APPDATA%\fr.n7rt.canlab\student-ide\` | copiez régulièrement ces deux fichiers ailleurs (clé USB, dépôt privé du groupe) |
| La progression, la fiche, le pack importé | `%APPDATA%\fr.n7rt.canlab\student\` | ne rien modifier à la main ; travaillez toujours sur le même PC et le même compte Windows |
| La fiche finale (E5) | `Documents\N7RT CAN Lab\fiches\` | remettez le sous-dossier entier (chapitre 9) |

Pour ouvrir `%APPDATA%\fr.n7rt.canlab` : touches Windows + R, collez le chemin, Entrée.

- **Fermer** : entre deux essais, quand vous voulez (jamais pendant une vérification). Si la fiche ou le code ont des modifications non enregistrées, une fenêtre propose **Enregistrer et fermer**, **Fermer sans enregistrer**, ou de revenir.
- **Reprendre** : chapitre 5, « après un redémarrage ».
- **Changer de PC** : la progression vit sur le PC, pas dans le pack. Un groupe qui change de PC repart de E0 avec le même pack : demandez d'abord à votre encadrant. Votre code, lui, se recopie.
- **Mettre à jour CAN Lab** (nouvelle version 0.3.x) : extrayez la nouvelle archive dans un **nouveau** dossier et lancez-la ; la progression, le pack et votre code sont dans `%APPDATA%` et sont repris.

## 13. Différences avec le tutoriel PDF

Le tutoriel PDF décrit la version 0.3.0. Depuis, la version 0.3.1 a corrigé l'application et la version 0.3.2 ajoute ce guide :

| Le PDF dit (0.3.0) | Depuis 0.3.1 |
|---|---|
| Archive `n7rt-canlab-0.3.0-windows-x64.zip` | `n7rt-canlab-0.3.2-windows-x64.zip`, avec ce guide à sa racine (`DEMARRER_ICI.md` et `DEMARRER_ICI.html`) |
| Outils trouvés dans `C:\ST` et le PATH | trouvés aussi dans leurs dossiers d'installation par défaut (`Program Files`) |
| « Outil de compilation indisponible : installez les prérequis du projet étudiant. », `arm-none-eabi-gcc not found`, `STM32_Programmer_CLI not found` | « Python introuvable… », « Compilateur arm-none-eabi-gcc introuvable… », « STM32_Programmer_CLI introuvable… », chacun avec l'outil à installer |
| « Le document cité par ces messages n'est pas fourni » | il est fourni : `docs/stm32-toolchain-windows.md` (et sa copie HTML) |
| Compilation impossible faute d'outil : bandeau « Compilation échouée. Corrigez les problèmes indiqués. » | le bandeau nomme l'outil manquant |
| Fenêtre « Fiche non enregistrée » : seul « et fermer » lisible | les trois boutons sont lisibles |
| Les trames des sessions précédentes ne se distinguent que par la colonne Session | elles sont grisées, avec une légende au-dessus du tableau |

Tout le reste du PDF s'applique tel quel.

**Où trouver quoi**

| Document | Contenu |
|---|---|
| `docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf` | le tutoriel complet, écran par écran |
| [docs/stm32-toolchain-windows.md](docs/stm32-toolchain-windows.md) | installation et diagnostic des outils, messages de Code STM32 |
| [docs/demarrage-5-minutes.md](docs/demarrage-5-minutes.md) | l'essentiel sur une page |
| [docs/known-issues-0.3.md](docs/known-issues-0.3.md) | problèmes connus et contournements |
| `N7RT_CANLab_Student\n7rt\include\n7rt.h` | l'API de votre code C |
