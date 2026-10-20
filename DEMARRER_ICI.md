# Démarrer ici — N7RT CAN Lab 0.3, guide étudiant

Ce guide vous mène seul, du téléchargement à la clôture de votre mission. Il ne suppose aucune connaissance du bus CAN, du langage C ni des cartes STM32 : la section 5 explique le minimum. Il complète le **tutoriel PDF** (`docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf`), qui montre chaque écran en détail : quand ce guide dit « tutoriel § n », c'est ce PDF.

Ni ce guide ni l'application ne donnent les réponses : identifier les trames, les décoder et diagnostiquer les pannes, c'est la mission.

## 1. Ce qu'il vous faut

| Quoi | D'où |
|---|---|
| Un PC **Windows 10 ou 11, 64 bits**, avec un port USB libre | le vôtre |
| L'archive `n7rt-canlab-<version>-windows-x64.zip` | section 2 |
| **Python 3.11+**, **Arm GNU Toolchain**, **STM32CubeProgrammer** (gratuits) | section 4 |
| Une carte **NUCLEO-H753ZI** avec le **runtime N7RT** déjà installé, et un câble USB de données | votre encadrant : vous ne pouvez pas installer le runtime vous-même |
| Le **nom de votre mission** (APPS, WSF, SAS ou BPS) | votre encadrant. **Le choix dans l'application est définitif** : ne choisissez pas au hasard |
| Le **pack de votre groupe** (`<groupe>.n7rtpack`) et le **code du groupe** (16 caractères, `XXXX-XXXX-XXXX-XXXX`), remis par deux canaux différents | votre encadrant |
| Un accès internet | pour télécharger, installer, et ouvrir les documents de la bibliothèque de la mission |

Sans carte, vous pouvez déjà installer, lire l'énoncé, explorer le Physical Lab, écrire et compiler votre code. Programmer la carte et lancer une vérification demandent la carte.

## 2. Télécharger et vérifier

1. Ouvrez la page <https://github.com/N7-Racing-Team/n7rt-canlab-etudiant/releases/latest> (aucun compte GitHub n'est nécessaire).
2. Dans **Assets**, téléchargez `n7rt-canlab-<version>-windows-x64.zip`. Le fichier `.sha256` à côté contient son empreinte.
3. Vérifiez que le fichier n'est ni tronqué ni modifié : dans PowerShell, dans le dossier Téléchargements,

```powershell
Get-FileHash -Algorithm SHA256 .\n7rt-canlab-<version>-windows-x64.zip
```

La valeur `Hash` doit être celle du `.sha256` (à la casse près). Sinon, téléchargez de nouveau.

## 3. Décompresser et lancer

1. Clic droit sur le `.zip`, **Extraire tout…**, puis choisissez un dossier où vous pouvez écrire, par exemple `Documents\CANLab` ; pas `Program Files`. **Ne lancez jamais l'application depuis l'intérieur du .zip** : sans ses dossiers voisins, la page Code STM32 répond « Projet étudiant introuvable ».
2. Dans le dossier obtenu, gardez tout ensemble : `n7rt-can-lab.exe`, `WebView2Loader.dll`, `N7RT_CANLab_Student\`, `tools\`, `library\`, `shared\`, `docs\`.
3. Double-cliquez `n7rt-can-lab.exe`. L'exécutable n'est pas signé : Windows affiche « Windows a protégé votre ordinateur » ; cliquez **Informations complémentaires**, puis **Exécuter quand même**. Cela n'arrive qu'une fois.

La fenêtre s'ouvre en mode étudiant, au premier lancement sur la page **Code STM32**. À gauche, la barre d'activité a trois boutons : **Code STM32**, **Cockpit**, **Physical Lab**. Une fenêtre qui reste blanche signale un composant WebView2 absent : installez « Microsoft Edge WebView2 Runtime » depuis le site de Microsoft.

## 4. Installer les outils de compilation

Une seule fois par PC, comptez 20 minutes. Le pas à pas, avec les liens, les cases à cocher et les commandes de vérification, est dans **[docs/stm32-toolchain-windows.md](docs/stm32-toolchain-windows.md)**. En résumé :

1. **Python** depuis python.org, en cochant **Add python.exe to PATH** ;
2. **Arm GNU Toolchain**, cible `arm-none-eabi`, installateur Windows depuis developer.arm.com ;
3. **STM32CubeProgrammer** depuis st.com (compte myST gratuit), en acceptant les pilotes ST-LINK ;
4. dans le dossier de CAN Lab : `python -m pip install -r .\requirements-student-ide.txt` ;
5. **fermez puis relancez CAN Lab**.

Vérification d'ensemble, dans PowerShell ouvert dans le dossier de CAN Lab :

```powershell
python .\tools\stm32\student_ide.py inventory
```

`"gcc": {"available": true …}` et `"programmer": {"available": true …}` : tout est prêt.

## 5. Le minimum à comprendre

**Le bus CAN.** Dans une voiture, les calculateurs (ECU) échangent des **trames** sur un bus partagé. Une trame porte un **identifiant** (un nombre écrit en hexadécimal, `0x…`) qui dit de quoi elle parle, une longueur (**DLC**, nombre d'octets de données) et jusqu'à 8 octets de **données** (64 en CAN FD). Chaque ECU émet ses trames à intervalle régulier : la **période**. Une grandeur physique (une position, une pression…) est codée dans quelques bits des données : pour la retrouver, il faut savoir **quels bits**, dans **quel ordre** (INTEL ou MOTOROLA), avec quel **signe**, puis appliquer `valeur = brut × facteur + offset`. Certaines trames portent aussi un **compteur** qui avance à chaque envoi et un **CRC**, une somme de contrôle qui détecte une trame abîmée. C'est ce que vous découvrez en observant le bus et en lisant les documents de la mission.

**La carte.** La NUCLEO-H753ZI porte un microcontrôleur STM32 et une sonde de programmation **ST-LINK**. Le **runtime N7RT**, installé par l'encadrant, reçoit les trames du véhicule simulé et appelle **votre code**. Programmer la carte (« flasher ») n'écrit que la zone de votre code ; le runtime est protégé et contrôlé avant et après.

**Votre code C.** Vous ne modifiez que deux fichiers, `student_decoder.c` et `student_decoder.h`. Le runtime appelle trois fonctions :

| Fonction | Quand | Ce que vous y faites |
|---|---|---|
| `student_init()` | au début de chaque session | remettre vos variables à zéro |
| `student_on_frame(frame)` | à chaque trame reçue | lire `frame->id`, `frame->len`, `frame->data[0…]`, décoder |
| `student_on_tick(now_us)` | périodiquement | surveiller le temps (une trame qui n'arrive plus) |

Pour rendre un résultat, `n7rt_publish(slot, valeur)` publie une valeur dans un **slot** (un numéro de sortie : l'énoncé de la mission dit quel slot porte quelle grandeur) ; `n7rt_reject(slot)` signale qu'il n'y a pas de valeur valide ; `n7rt_log("texte")` écrit une ligne dans la console de l'application (120 caractères maximum, 20 lignes par seconde). Le gabarit fourni compile et fonctionne : il publie la longueur et le premier octet de chaque trame, ce qui permet de tester toute la chaîne avant d'écrire une ligne. L'API complète est dans `N7RT_CANLab_Student\n7rt\include\n7rt.h`.

Pour lire des bits en C : `frame->data[2]` est l'octet 2 ; `(x >> 3) & 0x0F` garde les 4 bits de `x` à partir du bit 3 ; `(uint16_t)(a | (b << 8))` assemble deux octets. Le tutoriel § 5.1 donne la convention de numérotation des bits de la fiche.

**Les étapes.** E0 : décoder et documenter le fonctionnement normal. E1 à E3 : trois pannes à diagnostiquer, mesurer et réparer. E4 : une qualification automatique de votre code. E5 : la clôture. L'application juge chaque étape seule, sans dire ce qui est faux.

## 6. Brancher la carte et faire un premier essai

1. Branchez la NUCLEO par le connecteur USB de son **ST-LINK, CN1**, au milieu du petit côté de la carte (pas l'autre connecteur USB).
2. Dans la barre du haut, choisissez son port dans **CARTE** (la sonde ST-LINK apparaît avec son numéro de série), puis **Connecter**. Attendez **Prête**.
3. Si l'état est **Pas de code étudiant**, la carte n'a pas encore de programme à vous : c'est normal la première fois. Allez dans **Code STM32**, cliquez **Compiler + flasher** avec le gabarit tel quel. L'application libère la carte, la programme, puis la reconnecte.
4. Dans **Code STM32**, le panneau de la carte doit dire **La carte exécute la compilation actuelle.**
5. Dans le **Cockpit**, cliquez **Start** : le tableau « Trames reçues par le nœud étudiant » défile, et « Résultats étudiants » montre les valeurs publiées par le gabarit. **Stop** arrête la session.

Votre chaîne complète fonctionne. Règle à retenir : **arrêtez la session (Stop) avant de programmer la carte**.

Une nouvelle session (Start) n'efface pas le tableau : depuis la 0.3.1, les trames d'une session précédente restent affichées **en gris**, avec « lignes grisées : session précédente » en tête du tableau. Ne relevez vos mesures que sur les lignes normales.

## 7. Ouvrir la mission avec le pack et le code du groupe

Dans le **Cockpit**, panneau latéral (tutoriel § 2) :

1. Choisissez **la mission indiquée par votre encadrant**, **Choisir cette mission**, puis **Confirmer**. Ce choix est **définitif sur ce PC** ; une erreur se corrige seulement par l'encadrant.
2. **Importer le pack du groupe…** et sélectionnez votre fichier `.n7rtpack`. L'application en garde une copie : à faire une seule fois.
3. Saisissez le **code du groupe** (tirets, espaces et minuscules acceptés), puis **Ouvrir**. Le code n'est jamais enregistré : vous le saisissez à chaque ouverture. Gardez-le en lieu sûr.

Le banc de la mission se charge dans le Physical Lab, la **Bibliothèque** se remplit, et le panneau de progression apparaît : E0 est « EN COURS ». Lisez l'énoncé : résumé, « Exigences fonctionnelles » et « Sorties à publier » (le slot de chaque sortie).

Ne partagez ni le pack ni le code : chaque groupe a sa propre variante du véhicule, et les réponses d'un autre groupe sont fausses pour le vôtre.

## 8. Les étapes E0 à E5

**E0, essai nominal** (tutoriel § 5).
1. **Start**, puis dans le Physical Lab bougez le curseur de la grandeur que mesure votre ECU (Inspecteur, à droite) et repérez dans le Cockpit l'identifiant dont les **Données** changent. **Filtre ID (hex)** isole un identifiant ; un clic sur une trame affiche son détail ; l'écart entre les **t (ms)** de deux trames du même identifiant donne la période.
2. Cherchez dans la **Bibliothèque** les documents qui décrivent vos capteurs et la protection des trames (le marquage écrit sur chaque capteur du Physical Lab aide à trouver la bonne fiche technique).
3. Écrivez votre décodeur dans **Code STM32**, **Compiler**, corrigez les **Problèmes**, **Stop**, **Flasher**. Vérifiez vos valeurs dans « Résultats étudiants ».
4. **Ouvrir la fiche** (onglet **Fiche**, sous les trames) : remplissez les sept sections, puis **Enregistrer la fiche**. Rien ne s'enregistre tout seul.
5. **Lancer la vérification**. Ne débranchez pas la carte et ne fermez pas l'application pendant l'essai. Verdict « Vérification réussie » ou « Échec à corriger », avec deux lignes (fiche, essai) et aucun autre détail.

Ne modifiez pas les valeurs de la rubrique « Paramètres » des composants du Physical Lab : chaque vérification compare votre banc au banc de référence. Les curseurs et interrupteurs sont libres.

**E1, E2, E3, incidents** (tutoriel § 6). « Le véhicule revient d'essai avec une anomalie. » Observez le bus, vos slots et le banc ; dans la fiche d'incident, choisissez la cible, la catégorie et l'état observé : l'état impose la **mesure clé**. Mesurez au multimètre du Physical Lab ou sur le bus, **session démarrée (Start) depuis au moins 3 s**, puis **Consigner la mesure** et **Enregistrer la fiche**. Réparez (le circuit, votre code, ou les deux), puis **Vérifier la réparation**. Réparé avant d'avoir mesuré : **Reproduire l'anomalie**.

**E4, qualification** (tutoriel § 7). **Lancer la qualification**, carte branchée : huit compétences CP-1 à CP-8, PASS ou FAIL. Les huit en PASS clôturent la mission.

**E5, clôture** (tutoriel § 8). **Ouvrir la fiche finale** : le dossier `Documents\N7RT CAN Lab\fiches\<mission>-<groupe>-<date>\` contient `fiche.html`, `fiche.json` et `scellés\`. Remettez le dossier entier si votre encadrant le demande.

Entre deux vérifications d'une même étape : **2 minutes** (E0 à E3), **5 minutes** (E4). Une vérification interrompue (carte débranchée, application fermée) ne compte pas.

## 9. Sauvegarder et reprendre

| Quoi | Où | Comment le garder |
|---|---|---|
| Votre code (`student_decoder.c`, `.h`) | enregistré par **Enregistrer** ou **Compiler**, dans `%APPDATA%\fr.n7rt.canlab\student-ide\` | copiez régulièrement ces deux fichiers ailleurs (clé USB, dépôt Git privé du groupe) |
| La progression, la fiche, le pack importé | `%APPDATA%\fr.n7rt.canlab\student\`, sur ce PC et ce compte Windows | ne rien modifier à la main ; travaillez toujours sur le même PC |
| La fiche finale (E5) | `Documents\N7RT CAN Lab\fiches\` (dans OneDrive si vos Documents y sont redirigés) | remettez-la à l'encadrant |

Pour ouvrir `%APPDATA%\fr.n7rt.canlab` : touche Windows + R, collez le chemin, Entrée.

- **Fermer** : entre deux essais, quand vous voulez. Si la fiche a des modifications non enregistrées, l'application propose **Enregistrer et fermer**, **Fermer sans enregistrer** ou **Revenir à la mission**.
- **Reprendre** : relancez l'application, Cockpit, saisissez le code du groupe, **Ouvrir** : tout ce qui était enregistré est là, anomalie en cours comprise.
- **Changer de PC** : la progression vit sur le PC, pas dans le pack. Un groupe qui change de PC repart de E0 avec le même pack : demandez d'abord à votre encadrant. Votre code, lui, se recopie (première ligne du tableau).
- **Mettre à jour CAN Lab** (nouvelle version 0.3.x) : décompressez la nouvelle archive dans un **nouveau** dossier et lancez-la ; la progression, le pack et votre code sont dans `%APPDATA%`, pas dans le dossier de l'application, et sont repris.

## 10. Erreurs fréquentes et dépannage

| Ce que vous voyez | Que faire |
|---|---|
| « Python introuvable… », « … introuvable », « L'outil de compilation (Python) s'est arrêté sans réponse… » | [docs/stm32-toolchain-windows.md](docs/stm32-toolchain-windows.md), section 6 : chaque message y a sa cause et sa solution |
| « Projet étudiant introuvable : placez le SDK et les outils à côté de l'application. » | Section 3 : décompressez toute l'archive, lancez l'exe depuis le dossier décompressé |
| La liste **CARTE** affiche « Aucun port série détecté » | Câble sur **CN1** ? câble de données (pas de charge seule) ? autre port USB ; puis « Rechercher les cartes » |
| « Port occupé » | Une autre application tient le port (STM32CubeIDE, moniteur série, seconde fenêtre de CAN Lab) : fermez-la |
| « Carte incompatible », ou « Le runtime de la carte diffère du SDK » | La carte n'a pas le bon runtime : rendez-la à l'encadrant |
| **Start** grisé, état « Pas de code étudiant » | Section 6, étape 3 |
| Erreurs rouges dans **Problèmes** après **Compiler** | Ce sont des erreurs de votre C : fichier et ligne indiqués ; corrigez, enregistrez, recompilez |
| « Arrêtez la session avant de charger votre programme. » | **Stop** dans la barre du haut, puis **Flasher** |
| « Complétez la fiche de cette étape avant la vérification. Champs à renseigner : … » | Remplissez les champs nommés, **Enregistrer la fiche** |
| « Prochaine vérification possible dans … » | Attendez ; refaites vos mesures en attendant |
| « Aucune trame récente sur le bus : lancez le véhicule, puis consignez la mesure. » | **Start** (barre du haut ; le bouton RUN du Physical Lab ne suffit pas), attendez 3 s, consignez |
| « Le banc de la mission n'est plus ouvert dans le Physical Lab : … » | **Fermer** sous la mission (Cockpit), code du groupe, **Ouvrir** |
| Code du groupe refusé | Vérifiez les 16 caractères (confusion 0/O, 1/I) et que le pack importé est bien celui de votre groupe |
| « Progression illisible sur ce poste : … » | Prévenez l'encadrant ; ne supprimez rien |
| Fenêtre noire ou figée | Mémoire saturée : fermez les autres applications, relancez CAN Lab (la progression est reprise) |
| Les documents de la Bibliothèque ne s'ouvrent pas | Ils s'ouvrent sur le site de leur éditeur : vérifiez la connexion internet |

Problèmes connus et contournements : [docs/known-issues-0.3.md](docs/known-issues-0.3.md).

## 11. Ce qui a changé depuis le tutoriel PDF (version 0.3.1)

Le tutoriel PDF décrit la version 0.3.0. En 0.3.1 :

- les outils installés aux emplacements par défaut (Arm GNU Toolchain dans `Program Files (x86)`, STM32CubeProgrammer dans `Program Files`) sont trouvés sans configurer le `PATH` ;
- un outil manquant est signalé en français, avec le document à suivre, qui est maintenant fourni : `docs/stm32-toolchain-windows.md` ;
- un Python absent ou trop ancien a son propre message, au lieu de « Outil de compilation indisponible » ou « Réponse de l'outil illisible » ;
- les trames des sessions précédentes sont grisées dans le Cockpit ;
- les trois boutons de la fenêtre « Fiche non enregistrée » (dont **Enregistrer et fermer**) sont entièrement lisibles.

## 12. Où trouver quoi

| Document | Contenu |
|---|---|
| `docs/N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf` | Le tutoriel complet, écran par écran |
| [docs/demarrage-5-minutes.md](docs/demarrage-5-minutes.md) | L'essentiel sur une page |
| [docs/stm32-toolchain-windows.md](docs/stm32-toolchain-windows.md) | Installation et diagnostic des outils |
| [docs/inapp-student-ide-tutorial.md](docs/inapp-student-ide-tutorial.md) | La page Code STM32 en détail |
| `N7RT_CANLab_Student\n7rt\include\n7rt.h` | L'API de votre code C |
| [docs/known-issues-0.3.md](docs/known-issues-0.3.md) | Problèmes connus |
| [docs/README_CANLab_0.3.md](docs/README_CANLab_0.3.md) | Index de tous les documents |
