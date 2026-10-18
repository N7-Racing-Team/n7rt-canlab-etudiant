# Recette Windows des quatre missions

> Historique de la release 0.2 : ce document ne décrit pas la progression 0.3 (étapes E0 à E5, verdicts dans l'application). Pour la 0.3, suivez le [tutoriel étudiant](N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf).

Cette recette exige une NUCLEO-H753ZI portant le runtime N7RT épinglé, enregistrée dans le registre local des cartes, une connexion USB stable et le candidat Windows accompagné de son manifeste. Elle a été exécutée les 11 et 12 octobre 2026 ; les résultats figurent dans [le rapport R1](missions-four-r1.md). Avant chaque campagne, contrôler la tension de la cible (environ 3,3 V) : une alimentation instable a provoqué des redémarrages pendant la recette.

## Préparer les deux postes

Sur le poste encadrant autorisé, extraire l'archive encadrant et configurer son répertoire restreint selon [la distribution](distribution.md). Le dépôt restreint et le coffre restent sur ce poste. Aucun accès hérité de l'organisation étudiante n'est utilisé.

Sur un profil Windows étudiant distinct, extraire uniquement l'archive étudiante. Importer le fichier de groupe remis par l'encadrant et son code. Le poste étudiant ne reçoit ni checkout encadrant, ni corrigé, ni clé de génération. Utiliser le `n7rt-tool.exe` de l'archive encadrant sur le poste autorisé ; son répertoire doit être dans le PATH pour l'invoquer par son nom, ou l'invoquer par son chemin absolu. Les codes et clés ne doivent apparaître ni dans les captures ni dans les comptes rendus.

Après publication, les archives et leurs empreintes du tag publié constituent la distribution de référence. Un candidat local et son arbre de travail modifié doivent rester identifiés comme tels jusqu'à validation et publication autorisées.

## Contrôler le mode étudiant

1. Dans un profil neuf, constater que les quatre missions sont proposées. Choisir une seule mission, importer son pack, puis l'ouvrir avec le code du groupe.
2. Fermer et redémarrer l'application. Vérifier que le même choix reste verrouillé. Les trois autres choix et l'ouverture directe d'un autre pack doivent être refusés.
3. Ouvrir la bibliothèque de la mission sans connexion encadrant. Vérifier que les documents publics sont accessibles et qu'aucune trame n'est annotée avec sa fonction ou son décodage.
4. Modifier `student_decoder.c` et `student_decoder.h` dans l'IDE intégré, compiler, flasher, puis vérifier la reconnexion et la nouvelle empreinte du programme. Répéter après une modification volontaire du programme.
5. Lancer la vérification avec le template public : l'essai complet doit être enregistré, puis jugé FAIL sur le poste encadrant. Une déconnexion ou un essai incomplet doit annuler l'essai, sans produire PASS.

Le reset étudiant se fait par la procédure de réinitialisation du profil étudiant décrite dans [la distribution](distribution.md), sans transporter le dépôt restreint sur ce poste. Un reset du banc encadrant ne déverrouille pas le choix étudiant.

## Rejouer les quatre missions en mode encadrant

Pour chaque ligne, ouvrir la mission, utiliser l'IDE intégré pour charger le corrigé privé depuis le poste autorisé, compiler et flasher la carte. Exécuter le nominal et conserver le verdict, l'identité de la carte, l'empreinte du programme et le manifeste du candidat. PASS est requis avec le corrigé ; FAIL est requis avec le template public compilé et flashé dans le même IDE. Aucun résultat d'un autre programme ne peut servir de preuve.

| Mission | Manipulations du banc à vérifier | Résultats attendus à observer sur la STM32 |
| --- | --- | --- |
| APPS | Course de pédale, coupure d'un connecteur, variation LV et remise en état | Les huit sorties A suivent la course ou leur qualification ; les défauts entraînent les rejets et états de sécurité prévus. |
| WSF | Vitesse du véhicule, braquage, déconnexion gauche/droite, entrefer, alimentation et guérison | Les huit sorties W différencient les roues, l'arrêt, la plausibilité et les diagnostics ; un défaut de roue ne produit pas une vitesse nominale inventée. |
| SAS | Angle et vitesse de manœuvre dans les butées, coupure SENT, coupure alimentation et remise en état | Les cinq sorties S suivent l'angle et la vitesse effectivement transmis ; les défauts entraînent la qualification ou les rejets prévus. |
| BPS | Paliers de pression hydraulique, contact de frein, connecteurs avant/arrière, alimentation et remise en état | Les six sorties B suivent les pressions mesurées, le freinage, la qualification et la cohérence. |

Après le nominal, utiliser « Réinitialiser le banc » : le banc et sa source repartent au début. Ouvrir une autre mission sans quitter le mode encadrant, revenir à la précédente et répéter. Vérifier les quatre transitions et plusieurs répétitions. Le compte rendu doit distinguer le trafic véhicule, les RESULT réellement reçus de la carte et le verdict privé.

## Rejouer la recette GUI automatisée

Le harness `stm32lab app-test` joue les quatre missions dans l'application réelle avec la même implémentation, sur la carte enregistrée. Pour chaque mission `<m>` parmi `apps`, `wsf`, `sas` et `bps` :

| Scénario | Édition | Décodeur `N7RT_E2E_DECODER_C` | Verdict attendu |
| --- | --- | --- | --- |
| `ide-mission-<m>` | encadrant | corrigé privé de la mission | `--expect-verdict pass` (verdict nominal de l'application) |
| `ide-mission-<m>` | encadrant | template public | `--expect-verdict fail` |
| `ide-student-<m>` | étudiante, avec `--tool` = `n7rt-tool` encadrant | corrigé privé de la mission | `--expect-verdict pass` (fichier d'essai jugé par l'outil) |
| `ide-student-<m>` | étudiante, avec `--tool` = `n7rt-tool` encadrant | template public | `--expect-verdict fail` |

Commande type, depuis le dépôt étudiant : `python tools/stm32/stm32lab.py app-test C --scenario ide-mission-wsf --expect-verdict pass --exe <n7rt-can-lab.exe de l'archive encadrant>`.

L'environnement du harness fournit `N7RT_INSTRUCTOR_DIR`, `N7RT_E2E_INSTRUCTOR_CODE` et `N7RT_E2E_DECODER_C`. Les scénarios étudiants ajoutent `N7RT_E2E_PACK` et `N7RT_E2E_GROUP_CODE`. Le pack d'essai de référence d'un groupe se crée sur le poste encadrant avec `n7rt-tool test-pack <dossier encadrant> --mission <m> --group TEST --cohort <cohorte d'essai>`. Les codes ne sont jamais écrits dans les rapports ni affichés dans les comptes rendus.

Chaque parcours exige :
- compilation, flash et reconnexion par l'IDE intégré ;
- ouverture de la mission demandée, bus véhicule livré sans perte ni NACK, au moins 17 identifiants distincts, `TICK_RESULT` reçus ;
- affichage identique aux trames réellement envoyées, aucune erreur N7RT-Link (trame, rejet, trou de séquence) ;
- aucun redémarrage de la carte entre le début de session et la fin de l'essai nominal, le redémarrage dû au flash précédant la session ;
- vue du banc dans le Physical Lab, essai nominal et verdict attendu ;
- en mode étudiant : choix verrouillé, deux autres missions refusées, aucun verdict montré, fichier d'essai jugé par l'outil encadrant ;
- trois captures réelles par parcours encadrant (bus, banc, verdict), deux par parcours étudiant (mission verrouillée, essai enregistré).

Les rapports et captures sont écrits sous `missions/<m>/reports/app-test/` du dépôt encadrant, jamais dans le dépôt étudiant. Terminer par un parcours avec le template public afin de laisser la carte sur ce programme.

## Terminer la recette

Reflasher le template public et reconnecter la carte avant son retour à un étudiant. Fermer le mode encadrant et utiliser le profil étudiant séparé pour vérifier une dernière fois son verrou. Contrôler aussi le cockpit historique, le Physical Lab libre, l'édition des deux fichiers, les erreurs de compilation et la reconnexion après flash.

Consigner séparément les résultats matériels, la régression logicielle, les audits des deux archives, la reproductibilité et le test mémoire prolongé du candidat final. Une case non exécutée reste « à faire » ; une validation logicielle du runtime C ne remplace pas la recette NUCLEO.
