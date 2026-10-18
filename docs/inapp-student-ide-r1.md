# Mini-IDE étudiant intégré — rapport R1

Date : 2026-10-08. État : logiciel 0.2 validé et reproductible ; parcours IDE matériel et quatre essais APPS conformes aux attentes avec les binaires publiés, après reconnexion de la NUCLEO. Cette validation couvre le mini-IDE et les quatre cas APPS, pas quatre missions ni les budgets mémoire/CPU. Cette mise à jour des preuves complète la release publiée ; elle ne remplace pas ses archives originales.

`OUT:R1` désigne ce rapport et ses preuves. Aucun autre livrable nommé R1 ni protocole de validation supplémentaire n'est défini dans le texte de reprise fourni. Le tutoriel utilisateur est [inapp-student-ide-tutorial.md](inapp-student-ide-tutorial.md).

La [synthèse publique vérifiée de Board C](inapp-student-ide-board-c-verification-20261008.json) regroupe les résultats des cinq parcours, leurs comptes et contrôles, les empreintes publiques et les limites mémoire. Elle ne contient ni code de groupe, contenu de pack, source privée, identifiant CAN, valeur du bus ni chemin de profil.

Le test mémoire prolongé du candidat de clôture est suivi séparément dans [la revue release 0.2 R1](release-0.2-closure-r1.md). Les résultats courts et totaux de tests ci-dessous restent ceux de la campagne du binaire publié ; ils ne sont pas remplacés par ceux du candidat.

## Périmètre

Le CAN Lab reçoit une page dédiée au code étudiant, à côté du Cockpit et du Physical Lab. Le parcours attendu est : ouvrir le gabarit, modifier `student_decoder.c` ou `.h`, sauvegarder, compiler, lire les diagnostics, programmer la zone étudiante sur une carte explicitement choisie, puis retrouver la session dans le Cockpit. STM32CubeIDE reste utilisable avec le même SDK.

Le SDK public fournit les trois hooks `student_init`, `student_on_frame`, `student_on_tick` et les services `n7rt_publish`, `n7rt_reject`, `n7rt_now_us`, `n7rt_log`. Le runtime, la glue et le script de liaison restent des entrées protégées. Référence : [SDK étudiant v0](n7rt-student-sdk-v0.md).

Le backend expose `ide_state`, `ide_read`, `ide_save`, `ide_restore`, `ide_build`, `ide_flash`. Les deux fichiers éditables vivent dans le profil local ; les gabarits embarqués restent immuables. Toute sauvegarde qui change le code invalide la compilation précédente : programmer une image d'une ancienne révision doit être refusé.

L'éditeur utilise CodeMirror 6 : historique, recherche/remplacement, pliage, sélections multiples, complétion des seuls services publics et marqueurs des diagnostics GCC. Les commandes visibles sont **Enregistrer**, **Compiler**, **Flasher**, **Compiler + flasher**, **Restaurer le template** ; les sorties se consultent dans **Console** et **Problèmes**. Les sources survivent au relancement, mais l'état de build doit être recréé par **Compiler**.

La release publiée le 8 octobre rend APPS jouable et laisse WSF, SAS et BPS indisponibles. Les quatre parcours de cette campagne sont quatre essais APPS. Le candidat du 11 octobre intègre les quatre missions ; ses résultats sont suivis dans [son rapport R1](missions-four-r1.md), avec une nouvelle recette matérielle à faire. Aucune solution de référence, valeur de pack, liste d'identifiants véhicule ou clé encadrant n'est livrée dans le mini-IDE public ni dans ce rapport. Les validations encadrant chargent leur source privée uniquement à la demande de l'opérateur et n'en publient pas le contenu.

## Installation depuis l'archive

- Python 3.11 ou plus récent et `pyserial>=3.5,<4.0` sont nécessaires au helper local. Le ZIP fournit `requirements-student-ide.txt` à sa racine.
- Un compilateur `arm-none-eabi-gcc` et les outils de sa chaîne ARM doivent être installés. Le helper doit annoncer les outils découverts et l'action à faire quand un outil manque.
- STM32CubeProgrammer fournit le programmateur ST-LINK utilisé pour la carte. La carte se choisit par sonde et UID, jamais uniquement par un numéro COM.
- La release doit inclure le SDK public et le helper `tools/stm32/` avec tous ses imports locaux. Une archive extraite hors du dépôt doit suffire pour trouver ces ressources ; ni les sources privées du runtime ni l'installation de développement du mainteneur ne servent de repli.

La compilation utilise le runtime précompilé et contrôle l'image produite. La programmation n'écrit que la zone étudiante `0x08040000…0x08100000` et vérifie le runtime avant et après. Ces protections limitent ce que l'outil écrit. Elles ne rendent pas le programme C étudiant inoffensif à l'exécution : le runtime actuel ne fournit pas une sandbox matérielle contre un code malveillant.

## VERIFY FULL : matrice de validation

Un résultat PASS s'applique à la commande ou à l'exécutable identifié dans sa preuve. Les résultats marqués « candidat » ne constituent pas un PASS matériel du dernier binaire. Une fonctionnalité refusée ou une mission indisponible reste décrite comme telle.

Le parcours matériel IDE a été rejoué avec l'exécutable étudiant publié, de SHA-256 `5377f6336335fa45f736ec4eb1740b8b562f8286eee6ef00035b1f8cc9c6b6d3`, après que l'utilisateur a rebranché la NUCLEO et fermé l'instance précédente du CAN Lab. Le rapport `app_ide_c_20261008-125431.json` et ses six captures remplacent la provenance mixte du premier tutoriel. Les quatre cas APPS ont ensuite été exécutés et vérifiés séparément ; les deux refus attendus sont des contrôles négatifs réussis, pas des échecs de la validation.

| Contrôle | Résultat attendu | Preuve | État |
|---|---|---|---|
| Navigation | La page code s'ouvre et les deux autres vues restent accessibles | Rapports E2E IDE/Physical Lab et tests UI | PASS logiciel ; captures Physical Lab finales refusées par QA |
| Projet initial | Le gabarit public et ses deux fichiers éditables sont disponibles | Rapport E2E IDE et capture 01 | PASS |
| Sauvegarde | Les deux fichiers sont sauvegardés et les modifications sont signalées | Tests backend et E2E `both-authorised-files-and-combined-button` | PASS |
| Fermeture native | Annuler protège les modifications ; enregistrer puis fermer conserve la source ; une opération active suspend la fermeture | 11 tests UI et deux demandes de fermeture native du binaire publié | PASS : annulations réelles, autres choix couverts par tests UI |
| Édition | Coloration, numéros de ligne et positions GCC restent cohérents avec le texte | Tests UI, GCC réel et captures 02/04 | PASS |
| Compilation nominale | Le gabarit public compile ; runtime et ABI sont intacts | E2E `build` du binaire publié et tests helper | PASS matériel IDE et tests logiciels |
| Erreur C | Une erreur volontaire affiche le fichier et la ligne ; aucune image invalide n'est flashable | E2E `diagnostics-and-stale-refusal` du binaire publié et capture 04 | PASS |
| Erreurs des outils | Les erreurs du helper sont lisibles et ne révèlent pas de chemins privés | Tests helper/backend | Couverture unitaire ; aucun essai UI spécifique avec outils désinstallés |
| Chemins | La release extraite hors du dépôt fonctionne ; seuls les deux fichiers autorisés du profil sont éditables ; les entrées SDK sont protégées | E2E depuis l'archive extraite et tests helper/backend | PASS |
| Révision | Modifier puis sauvegarder après un build invalide l'image ; un flash exige une nouvelle compilation | Tests backend/helper et E2E `diagnostics-and-stale-refusal` | PASS logiciel et binaire publié |
| Flash contrôlé | Carte connectée et runtime vérifié ; session/essai actifs refusés ; écriture limitée au code étudiant ; runtime identique avant et après | Tests des garde-fous et E2E `flash-reconnect-ui` du binaire publié | PASS matériel IDE |
| Reconnexion | Le port est libéré pour le programmateur puis reconnecté ; UID, runtime et CRC étudiant correspondent | E2E `flash-reconnect-ui` du binaire publié et capture 06 | PASS matériel IDE |
| Cas APPS positif | V0 avec décodeur de référence : réussi côté encadrant | Rapport privé `125526` et jugement du fichier enregistré | PASS : édition publique sans verdict UI ; jugement encadrant réussi, code 0 |
| Variante APPS | Même décodeur de référence sur une variante : non réussi | Rapport privé `125619` et jugement du fichier enregistré | PASS du contrôle négatif : essai non réussi, code 1 attendu |
| Édition encadrant | Essai de référence : réussi avec l'oracle ; aucun verdict dans l'édition étudiant | Rapport privé `125718` et contrôle de séparation | PASS : état terminé et verdict réussi dans l'édition encadrant |
| Gabarit public | L'intégration fonctionne mais le gabarit n'est pas une solution APPS : non réussi | Rapport privé `125806` et jugement du fichier enregistré | PASS du contrôle négatif : gabarit non réussi, code 1 attendu |
| Autres missions | WSF, SAS, BPS restent annoncées indisponibles | Suites Rust `mission` et `mission_modes` ; catalogue public | PASS logiciel : indisponibilité confirmée ; aucune mission supplémentaire jouable |
| Régressions | Tests TypeScript, Python, Rust public et Rust avec oracle passent | Résumés de suites | PASS logiciel : UI 253, Rust public 371, Rust oracle 393 ; Python détaillé plus bas |
| Release autonome | Archive extraite hors du dépôt, navigation et build local exécutés depuis cette archive | E2E IDE du binaire publié depuis son archive extraite | PASS : édition, compilation, diagnostics, flash et reconnexion |
| Confidentialité | Audit arbre et archive étudiant ; aucun oracle dans le binaire ; captures relues | Audit final original au commit `734e769` et contrôle du marqueur d'édition | PASS : 343 fichiers, 60 entrées ZIP ; aucune nouvelle exception ; nouvelles captures relues |
| Intégrité | SHA-256, CRC des ZIP, fichiers des manifestes et commits propres concordent | Archives originales publiées : étudiant `734e769`, encadrant `ba530a5` | PASS : 59/60 fichiers manifestés, CRC et empreintes ; archives originales conservées |
| Reproductibilité | Deux builds indépendants de sources applicatives identiques, avec mêmes outils et date, puis emballage au même commit documentaire donnent les mêmes exécutables et archives | Compilations A/B `9436a9d`/`59a2de3`, emballages finaux propres `734e769` | PASS : exécutables et ZIP originaux A/B identiques ; supplément de preuves distribué séparément |

## Review indépendante

La lecture du frontend, du backend et du helper a couvert les opérations concurrentes, le rechargement, les chemins, les diagnostics et la séparation étudiant/encadrant. Les corrections identifiées incluent : résolution du SDK depuis une archive extraite, signatures de complétion conformes à l'ABI, absence d'avertissement de fermeture avant le premier chargement, rejet des snapshots de polling périmés, contrôle identique de la cible pour les deux commandes de flash, invalidation après restauration partiellement échouée et reprise après un temporaire abandonné.

Les sources C ne sont ni interprétées comme du HTML ni utilisées pour construire une commande shell. Les commandes du helper ont des arguments fixes, le compilateur ne reçoit pas les variables d'injection de chemins ou les codes encadrant, et les diagnostics affichés utilisent des noms de fichier autorisés. Le helper contrôle les sections ELF, la zone de chargement, l'image runtime et les empreintes des sources avant programmation.

### Exécution des tests Tauri sous Windows

Les binaires de tests de la bibliothèque Tauri ne reçoivent pas automatiquement le manifeste de l'exécutable de l'application. Avec la dépendance `common-controls-v6`, un binaire de tests sans ce manifeste peut s'arrêter dans le chargeur Windows avec `0xC0000139`, avant d'exécuter une assertion. La campagne utilise un runner qui compile les tests puis ajoute le manifeste standard aux seuls binaires de tests générés. Il ne modifie ni la configuration Windows ni les exécutables livrés.

Depuis la racine de la source combinée :

```powershell
python app/scripts/test-workspace.py --public --jobs 1 --build-date 2026-10-08
python app/scripts/test-workspace.py --oracle --jobs 1 --build-date 2026-10-08
```

Depuis le dépôt étudiant exporté, l'oracle est celui du dépôt encadrant voisin ; ajouter `--oracle-dir ../n7rt-canlab-missions/oracle/n7rt-vehicle`. Les rapports sont `app/target/test-workspace-public.json` et `app/target/test-workspace-oracle.json`. Pour cibler les assertions du mini-IDE, ajouter `--package n7rt-can-lab -- ide::tests --nocapture`.

La campagne finale avec oracle a compté 393 tests réussis, aucun échec et cinq tests ignorés dans 43 suites (`app/target/test-workspace-oracle.json`), dont les 14 tests backend du mini-IDE. Le nouveau test de reconnexion à la même carte protège la session active. La première campagne avait révélé un échec de ce test ; la source a été corrigée puis la campagne complète a été rejouée.

Les cinq tests Rust ignorés par défaut sont deux benchmarks, un soak long de circuits, un générateur de fichiers de démonstration et un export de traces de référence privées. Ils n'ont pas été exécutés par cette campagne. Les deux générateurs écrivent des artefacts et ne constituent pas des assertions manquantes du parcours IDE.

## Captures et tutoriel

Les six captures montrent la page code et le gabarit, l'édition des deux fichiers, la compilation réussie, le diagnostic de compilation, la carte prête à programmer et la reconnexion vérifiée dans Code STM32. Toutes proviennent du binaire étudiant publié `5377…`, exécuté avec la carte. La série publique utilise du code générique. Les codes de groupe, clés, packs en clair et détails du juge restent hors des captures.

Chaque écran du tutoriel indique l'action, le changement visible et la suite possible. Une image ne remplace pas la preuve de compilation ou de programmation. Les six images ont été relues indépendamment : texte lisible, gabarit public, aucun code de groupe ni élément de solution privée.

La série utilise `docs/img/inapp-student-ide/01-project.png`, `02-edit.png`, `03-build.png`, `04-diagnostics.png`, `05-flash.png` et `06-reconnected.png`.

| Capture publiée | Exécutable | Provenance |
|---|---|---|
| 01-project à 06-reconnected | Binaire étudiant publié, SHA-256 `5377f6336335fa45f736ec4eb1740b8b562f8286eee6ef00035b1f8cc9c6b6d3` | `app_ide_c_20261008-125431.json` ; parcours complet réussi avec la carte rebranchée |

Les six nouvelles images ont été relues indépendamment : fenêtres complètes de 1702 × 1114 pixels, code générique et diagnostics lisibles, aucun contenu de solution privée. Elles remplacent les images de provenance mixte du tutoriel initial. Les captures minimisées de la prévisualisation sans carte restent rejetées et ne sont pas publiées.

Provenance : `app_ide_c_20261008-125431.json`, scénario `ide`, exécuté depuis l'archive publiée extraite dans un dossier temporaire indépendant, avec le dossier de l'exécutable comme répertoire courant. SHA-256 de `n7rt-can-lab.exe` : `5377f6336335fa45f736ec4eb1740b8b562f8286eee6ef00035b1f8cc9c6b6d3`. L'E2E a duré 28,9 s, avec code de sortie 0, six contrôles IDE et quatre contrôles du harness réussis. Les quinze étapes fonctionnelles ont réussi ; cinq comparaisons interface/carte ont vérifié 200 lignes sur 200, 503 réponses ont été contrôlées à la fin, sans erreur de framing, rejet ni trou de séquence.

Le parcours du binaire publié couvre les annulations de fermeture native, la compilation, l'erreur GCC volontaire et le refus d'image périmée, la programmation puis la reconnexion, ainsi que les deux fichiers autorisés et **Compiler + flasher**. La condition « captures du binaire final » est désormais satisfaite pour les six écrans IDE. Les quatre essais APPS constituent la campagne distincte détaillée ci-dessous.

La capture 05 précède le flash et indique que le code de la carte diffère de la compilation actuelle. La capture 06 suit les vrais boutons **Flasher** puis **Compiler + flasher** : le CRC de la carte correspond au build et l'interface confirme que la carte exécute la compilation actuelle. Le pic de mémoire privée de l'ensemble application/WebView2, 311,1 Mio, appartient à ce parcours court ; il ne constitue pas un test de budget mémoire sous charge.

## Preuves et limites matérielles

Le test public de départ publie la longueur de la trame et, lorsqu'il existe, son premier octet. Il vérifie l'intégration et ne résout aucune mission. C'est le seul code nominal générique disponible publiquement ; aucune fixture de solution WSF, SAS ou BPS n'est inventée.

Avant la déconnexion demandée à 10:02, un début de parcours APPS privé avait été arrêté par une comparaison erronée du libellé dans le harness, avant le jugement attendu. Ce premier essai ne prouve aucun verdict. L'utilisateur a ensuite rebranché la NUCLEO et fermé l'instance CAN Lab précédente ; le parcours IDE générique et les quatre cas APPS ont été repris avec les binaires publiés et ont satisfait leurs assertions.

### Quatre essais APPS, quatre attentes distinctes

| Cas | Rapport privé | Durée ; contrôles IDE / étapes / assertions | Résultat constaté |
|---|---|---|---|
| Décodeur de référence sur V0 dans l'édition étudiant | `app_ide-student-apps_c_20261008-125526.json` | 20,1 s ; 4 / 12 / 11 | Essai enregistré sans verdict UI ; jugement encadrant `pass`, code 0 |
| Même décodeur sur une variante de groupe | `app_ide-student-apps_c_20261008-125619.json` | 19,6 s ; 4 / 12 / 11 | Essai enregistré sans verdict UI ; jugement `fail`, code 1 attendu |
| Référence dans l'édition encadrant | `app_ide-mission-apps_c_20261008-125718.json` | 23,3 s ; 4 / 11 / 10 | Essai terminé ; verdict encadrant `pass` |
| Gabarit public sur V0 | `app_ide-student-apps_c_20261008-125806.json` | 20,0 s ; 4 / 12 / 11 | Essai enregistré sans verdict UI ; jugement `fail`, code 1 attendu |

Les quatre rapports sont globalement réussis, avec sortie application 0 et toutes les assertions conformes. Les fichiers d'essai étudiants enregistrés contiennent les ticks nominaux complets ; les comparaisons interface/bus vérifient 200 lignes sur 200, sans perte, NACK, rejet, erreur de framing ni trou de séquence. Les UID, CRC du build, CRC reconnecté et métadonnées de l'essai concordent, avec runtime vérifié. Les essais étudiants utilisent le binaire publié `5377…` ; l'essai encadrant utilise celui de SHA-256 `3814e92409391c0b05694b24d4c13f36c66f77df27c4af2ae1805eb8b598148d`. Les rapports bruts, fichiers d'essai, sources privées et captures APPS restent dans l'espace encadrant ; ce rapport ne publie que les résultats et leur provenance.

Après les essais, Board C a été laissée au gabarit public, CRC `0x5CBC80B3`, avec runtime vérifié. WSF, SAS et BPS sont indisponibles dans ces binaires publiés : ces quatre essais ne valident pas le candidat des quatre missions. Ils ne mesurent pas non plus les budgets mémoire/CPU ou une éventuelle fuite sur une exécution longue.

Le rapport Physical Lab `app_phys_20261008-082641.json` décrit une validation antérieure : scénario fonctionnel réussi, trois contrôles réussis, cinq captures. Il ne prouve pas le nouveau mini-IDE. Les audits et archives antérieurs à ce changement ne valident pas non plus le nouveau helper ni la nouvelle page.

Le Physical Lab a aussi été rejoué sur le dernier binaire `5377…` sans carte (`app_phys_20261008-102314.json`) : seize étapes fonctionnelles et code de sortie 0 réussis. Les cinq captures de ce run ne montraient qu'une barre de titre minimisée et ont été refusées lors de la QA ; le booléen « captures » du harness précédent ne suffit donc pas à déclarer ce run entièrement PASS. Les cinq images complètes du run candidat `50c9…` (`app_phys_20261008-095543.json`) restent une preuve historique valide. Le contrôle du harness a été renforcé pour refuser les fenêtres minimisées et les géométries insuffisantes, en lecture seule.

La release 0.1 précédant le mini-IDE a été publiée après deux compilations en clones propres donnant les mêmes empreintes. Le mini-IDE 0.2 modifie les sources compilées ; il dispose désormais de sa propre comparaison de deux compilations indépendantes, décrite ci-dessous, sans déduire ce résultat de la 0.1.

### Audit et intégrité des archives 0.2

L'audit final des archives originales a contrôlé 343 fichiers de l'arbre Git et les 60 entrées de l'archive A, issue du commit propre `734e769`. Il ne trouve aucun élément privé. Les 45 correspondances autorisées sont des faux positifs déjà examinés ; aucune nouvelle exception ni règle affaiblie n'a été ajoutée. Le binaire public ne contient pas le marqueur de l'oracle. Les six nouvelles captures ont été relues séparément avec la provenance indiquée plus haut.

Les archives originales A et C passent le contrôle de tous leurs CRC, des 59 et 60 fichiers décrits par leurs manifestes, de leurs fichiers `.sha256` et des cinq empreintes du SDK. Le manifeste du SDK correspond au runtime public épinglé. C contient bien le marqueur oracle dans ses deux exécutables ; son application et son outil correspondent aux artefacts encadrant construits. Les 57 ressources publiques communes sont identiques octet par octet entre A et C. Les sources déclarées par les manifestes sont propres : étudiant `734e769`, encadrant `ba530a5`.

Les archives publiées restent immuables : étudiant `n7rt-canlab-0.2.0-windows-x64.zip`, SHA-256 `080763c1ba0b7470091141317c1a2adbdc0945306795013acb677d6cfa7259f5` ; encadrant `n7rt-canlab-0.2.0-instructor-tools-windows-x64.zip`, SHA-256 `6dcf0de0d953b2815701fd152cebd85613ddc724ce3ea90cdc73e6b63eb3a0d6`. Le supplément daté `n7rt-canlab-0.2.0-board-c-verification-20261008.zip` distribue ces preuves matérielles et les captures actualisées séparément, sans recompilation, remplacement d'exécutable ni déplacement du tag original. Son empreinte figure dans son fichier `.sha256`, pas dans ce rapport.

La compilation C a été lancée avec le commit encadrant `fe250de` et le commit étudiant `9436a9d`. Pendant cette compilation, seuls des documents, README et le harness Python ont évolué ; aucun fichier applicatif ni source compilée de l'oracle n'a changé. Les commits `ba530a5`/`734e769` identifient son emballage publié et ne sont pas présentés comme les commits de démarrage de la compilation.

Le binaire A a été compilé au commit `9436a9d`, à la date fixée du 2026-10-08, puis emballé avec les documents du commit `59a2de3`. Le build indépendant B a réellement recompilé le commit propre `59a2de3` dans un clone et une cible de compilation isolés, avec sortie 0 après 22 min 08 s. `git diff 9436a9d 59a2de3 -- app` est vide ; les 243 fichiers applicatifs comparés entre la source et l'export, dont 196 sources et configurations compilées, sont identiques. Le diff complet entre les deux commits ne concerne que les documents, captures et le harness Python non compilé.

Les deux exécutables sont identiques octet par octet, de SHA-256 `5377f6336335fa45f736ec4eb1740b8b562f8286eee6ef00035b1f8cc9c6b6d3`. Les deux ZIP étudiant, d'abord comparés au commit propre `59a2de3`, puis emballés et vérifiés au même commit final propre `734e769`, sont également identiques : 60 entrées, CRC, empreintes des fichiers et runtime épinglé vérifiés. La preuve porte ainsi sur deux vraies compilations indépendantes, puis sur leur distribution ; elle ne repose pas sur deux réemballages du même exécutable. Les rapports du mainteneur sont `ide-repro-compiled-20261008.json`, `ide-release-final-verification-20261008.json` et `ide-final-archive-integrity-20261008.json`. Les présentes preuves matérielles sont distribuées dans un supplément distinct ; les archives originales et leurs exécutables restent identiques.

## Résultats logiciels confirmés

Ces résultats portent sur les sources du 2026-10-08. La provenance des exécutables et archives est décrite ci-dessus ; ces suites ne remplacent pas les essais matériels reportés.

| Campagne | Commande depuis la racine source | Résultat |
|---|---|---|
| Python configuré du workspace, relancé après le contrôle des captures | `python -B -m pytest -q -rs -p no:cacheprovider` | 316 passés, 2 ignorés, 0 échec ; 118,95 s |
| Python complet du dépôt étudiant exporté, relancé seul après compilation | `python -B -m pytest -q -rs -p no:cacheprovider` | 259 passés, 5 ignorés, 0 échec ; 61,57 s ; suite UI comprise |
| Helper final, relancé après les changements de diagnostics UTF-8 | `python -B -m pytest -q tools/stm32/tests/test_student_ide.py -p no:cacheprovider` | 35 passés, 0 échec ; 4,41 s |
| Garde de collecte des fichiers privés | `python -B -m pytest -q restricted/tools/tests/test_audit_student.py -p no:cacheprovider` | 6 passés, 0 échec |
| Python du dépôt encadrant exporté | `python -B -m pytest tools/tests tools/n7rt_link/tests -q -rs -p no:cacheprovider` | 25 passés, aucun échec ni test ignoré ; audit et runtime C hôte |
| Packaging étudiant et encadrant autonome | `python -B -m pytest tools/release/tests -q` | 26 passés, 0 échec ; extraction indépendante et compilation GCC réelle couvertes |
| Helper, SDK et parseurs de programmation | `python -B -m pytest tools/stm32/tests/test_student_ide.py tools/stm32/tests/test_n7rt_sdk.py tools/stm32/tests/test_stm32lab.py -q` | 56 passés, 1 ignoré, 0 échec |
| Workspace Rust avec oracle, dont le backend du mini-IDE | `python app/scripts/test-workspace.py --oracle --jobs 1 --build-date 2026-10-08` | 393 passés, 0 échec, 5 ignorés dans 43 suites ; les 14 tests IDE ont réussi |
| Workspace Rust public | `python app/scripts/test-workspace.py --public --jobs 1 --build-date 2026-10-08` | 371 passés, 0 échec, 4 ignorés dans 43 suites |
| Fermeture native et positions GCC | Tests UI de la vraie classe et du modèle | 11 tests de fermeture et 6 tests du modèle réussis ; `npm run check` réussi |
| UI complète | `npx vitest run` | 253 tests réussis dans 25 fichiers ; contrôles UI et TypeScript réussis |
| Audit de l'arbre et de l'archive étudiant A | `python -B tools/audit_student.py ../n7rt-canlab ../n7rt-canlab/dist/n7rt-canlab-0.2.0-windows-x64.zip --redact` depuis le dépôt encadrant | 343 fichiers et 60 entrées ZIP contrôlés, aucun finding ; 45 faux positifs déjà relus autorisés, aucune nouvelle exception |

Les deux tests Python ignorés sont l'exécution Rust facultative et le build lourd CubeIDE. Pour cette exécution Python, `N7RT_RUN_CARGO=0` et `N7RT_RUN_CUBEIDE=0` ont été imposés dans l'environnement du processus ; les tests Rust sont exécutés séparément par leur runner. Le test privé d'audit est hors des chemins configurés par `pyproject.toml` et a donc été lancé explicitement, sans être ajouté au total 316.

Le dépôt étudiant exporté ignore en plus le module de packaging encadrant, qui requiert la source privée combinée, un test qui exige l'image runtime privée et un adaptateur `pc/dashboard` non distribué. Ces trois absences sont prévues par la séparation des dépôts. Les suites source et exportée n'ont donc pas les mêmes chemins de collecte ; leurs totaux ne s'additionnent pas.

La tentative exportée pendant la compilation encadrant avait obtenu 258 tests Python réussis, 252 tests UI réussis et un timeout du test de propriété de la liste virtuelle : 5842 ms pour une limite de 5000 ms, sans assertion fausse. Aucun seuil ni test n'a été changé ; la suite complète a ensuite été rejouée seule, après la fin de la compilation, avec 259 tests Python réussis et la suite UI entièrement réussie.

Un premier scénario E2E de la vraie édition publique avait validé les étapes IDE mais échoué dans la suite fonctionnelle sur une reconnexion d'une carte déjà prête. La source a été corrigée et le scénario complet a été rejoué. Les nouvelles captures décrites plus haut proviennent toutes du binaire publié ; ce premier rapport reste historique et n'est pas utilisé comme PASS global.
