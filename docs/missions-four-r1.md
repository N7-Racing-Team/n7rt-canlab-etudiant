# R1 — quatre missions, release 0.2

> Historique de la release 0.2 : ce document ne décrit pas la progression 0.3 (étapes E0 à E5, verdicts dans l'application). Pour la 0.3, suivez le [tutoriel étudiant](N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf).

État au 12 octobre 2026 : candidat final5 de la release 0.2, ni tagué ni publié en release. APPS, WSF, SAS et BPS sont jouables de bout en bout et recettées sur NUCLEO, par la liaison série puis dans l'application réelle, en modes encadrant et étudiant.

## Critères d'acceptation

| Critère | État | Preuve |
| --- | --- | --- |
| WSF, SAS et BPS ont banc Physical Lab, ECU source, manipulations, trafic véhicule, résultats STM32 et oracle privé | **PASS** | Bancs, sources ECU et consommateurs privés des quatre missions ; scénarios nominaux et cas de défaut dans les suites Rust publique et privée ; recette sur carte ci-dessous. |
| Chaîne Physical Lab → ECU → bus CAN → N7RT-Link → NUCLEO → `student_decoder` → RESULT → verdict | **PASS sur NUCLEO** | Board C : exports des vrais bancs envoyés à la carte par N7RT-Link, juge privé. Board D : parcours complets dans l'application. |
| IDE intégré : modifier, compiler et flasher `student_decoder.c/.h` pour les quatre | **PASS** | Compilation, flash et reconnexion par l'IDE intégré de l'application, pour chaque mission, avec le corrigé puis avec le template. |
| Bibliothèques publiques de chaque mission sans rôle ni réponse | **PASS** | Bibliothèque par liens et manifestes sans rôle ; audits source et archives sans finding. |
| Mode étudiant : choix unique verrouillé, conservé au redémarrage | **PASS** | Tests des quatre choix et du redémarrage ; dans l'application, pour chaque mission : choix verrouillé, deux autres missions refusées, aucun verdict montré à l'étudiant. |
| Mode encadrant : ouvrir, rejouer et réinitialiser les quatre, sans privilège dans la distribution étudiante | **PASS** | Contrôleur testé avec les quatre sources actives ; marqueur oracle absent de l'exécutable étudiant ; fichiers d'essai étudiants jugés seulement par l'outil encadrant. |
| Corrigé privé PASS et template public FAIL pour chaque mission | **PASS** | En C natif, sur NUCLEO par la liaison série, et dans l'application en modes encadrant et étudiant. |
| Régression complète | **PASS fonctionnel ; budgets mémoire et d'images hors limite** | Suites, smoke, essai court et soak ci-dessous. |
| Release Windows testable et protocole de recette | **PASS** | Builds final5, archives étudiantes A/B identiques, audits sans finding ; [protocole de recette](missions-four-manual-acceptance.md). |

## Recette sur carte

**Board C, liaison série, 11 octobre.** Pour chaque mission, le corrigé privé puis le template public sont compilés et flashés par le chemin de l'IDE intégré. Les exports des vrais bancs (Physical Lab, ECU source, bus véhicule) sont ensuite envoyés à la carte en temps réel par N7RT-Link, et les `RESULT` reçus sont jugés par le juge privé.

- Cinq campagnes complètes concordent, soit 40 exécutions : quatre corrigés PASS et quatre templates FAIL à chaque fois.
- Chaque exécution reçoit 4 340 `RESULT` et 900 `TICK_RESULT`, dans l'ordre et une seule fois.
- Les compteurs de perte du runtime sont nuls, et les paquets de la carte sont ré-encodés à l'identique par le codec de référence.
- Aucun NACK, aucun FAULT.

**Board D, application réelle, 12 octobre.** Cette nouvelle NUCLEO-H753ZI a reçu le runtime 0.1.0 épinglé, dont l'image a été vérifiée par empreinte et par le défi SHA-256 de la carte. Le harness `stm32lab app-test` joue les quatre missions avec une seule implémentation paramétrée par mission (`[ide-]mission-<id>`, `[ide-]student-<id>`).

| Mission | Encadrant, corrigé | Encadrant, template | Étudiant, corrigé | Étudiant, template |
| --- | --- | --- | --- | --- |
| APPS | PASS | FAIL attendu | PASS | FAIL attendu |
| WSF | PASS | FAIL attendu | PASS | FAIL attendu |
| SAS | PASS | FAIL attendu | PASS | FAIL attendu |
| BPS | PASS | FAIL attendu | PASS | FAIL attendu |

En mode encadrant, le verdict est celui de l'essai nominal affiché par l'application. En mode étudiant, l'essai est enregistré sans verdict puis jugé par l'outil encadrant. Chaque parcours vérifie :
- la compilation, le flash et la reconnexion par l'IDE intégré ;
- l'ouverture de la mission demandée et un bus véhicule livré sans perte ni NACK, avec au moins 17 identifiants distincts ;
- un affichage identique aux trames réellement envoyées et aucune erreur N7RT-Link ;
- aucun redémarrage de la carte entre le début de session et la fin de l'essai nominal ;
- un bandeau de banc qui nomme la mission ouverte ;
- les captures réelles : bus, banc et verdict en mode encadrant ; mission verrouillée et essai enregistré en mode étudiant.

Lors d'un parcours SAS, la capture du verdict a échoué parce que la fenêtre était réduite ou indisponible au moment de la prise. Le parcours a été rejoué avec ses trois captures ; l'échec reste consigné.

**Défaut trouvé et corrigé.** Dans le Physical Lab, le bandeau d'un banc de mission affichait « Banc de la mission APPS » pour tous les bancs. Il nomme désormais la mission ouverte, et le harness vérifie ce texte. Les captures de la campagne final5 le montrent pour les quatre missions.

**Marge temporelle des hooks.** Le budget par défaut d'un hook est de 200 µs, soit 12 800 cycles, avec dépassement signalé mais non sanctionné. Les corrigés WSF, SAS et BPS s'en approchent et le dépassent ponctuellement, sans effet sur les verdicts.

En fin de campagne, Board D porte le template public, runtime vérifié, sans défaut en attente. Board C portait aussi le template public lors de sa dernière vérification, avant d'être débranchée.

## Régression du candidat final5

| Suite | Résultat |
| --- | --- |
| Rust public complet | 45 suites : **393 PASS**, 0 FAIL, 4 ignorés |
| Rust privé (oracle) complet | 45 suites : **482 PASS**, 0 FAIL, 6 ignorés |
| Conformité exacte encadrant (Rust) | 22 tests exacts PASS sur les sources publiques courantes |
| Python public | **330 PASS**, 5 skips, 0 échec |
| Python privé | **107 PASS**, dont 12 tests de conformité exacte N7RT-Link |
| Interface | **291 tests PASS** (33 fichiers) ; contrôle TypeScript et géométrie : 81 fichiers PASS |
| Smoke Windows Physical Lab (exécutable extrait de l'archive final5) | 16 étapes, 73 contrôles, 5 captures, sortie 0 : **PASS** |
| Audit de confidentialité | Arbre public : 378 fichiers, zéro finding. Archives étudiantes A/B : zéro finding |

Les sources Rust sont identiques à celles du candidat final3 : les suites Rust de final3 restent valables pour final5. Depuis final3, seuls l'interface (harness E2E, bandeau du banc), les outils Python du harness et la documentation ont changé.

## Builds Windows

Les builds étudiant A, étudiant B indépendant (cache séparé) et encadrant passent avec un job. Les exécutables étudiant A et B sont identiques, tout comme les ZIP. L'exécutable étudiant ne contient pas le marqueur oracle ; l'édition encadrant le contient, comme attendu.

Le repack documentaire final réutilise ces exécutables par leurs receipts de compilation, qui refusent toute entrée compilée modifiée. Sur les archives finales :

- les ZIP étudiants A et B sont identiques dans la totalité de leurs octets ;
- manifestes, manifeste du SDK, CRC des entrées, quatre énoncés et fins de ligne des scripts sont conformes, sans cache ni temporaire ;
- les ressources communes des éditions étudiante et encadrant sont identiques ; l'archive encadrant n'ajoute que `n7rt-tool.exe` ;
- aucun fichier de vecteurs ou d'oracle, aucune ancre de conformité, aucun corrigé, pack, bench ou secret dans l'archive étudiante ;
- l'archive encadrant ne contient ni secret opérationnel, ni pack de groupe scellé, ni source C de référence, ni source de consommateur privé ;
- audit de confidentialité de l'arbre source et des archives étudiantes : zéro finding, exceptions exactes revues uniquement.

Les empreintes des archives figurent dans leurs fichiers `.sha256` et manifestes, pas dans ce rapport embarqué.

## Vecteurs publics retirés

À la demande du responsable, les fixtures exacts CRC, COBS, cryptographie, persistance et circuits à résultats fixes ont quitté le dépôt et le build étudiants. Les tests publics construisent leurs entrées et vérifient des lois ou propriétés. Les formats, constantes et lois restent documentés dans le contrat N7RT-Link. Les comparaisons octet par octet s'exécutent côté encadrant sur les sources publiques courantes. Ce retrait ne réécrit pas l'historique Git déjà publié.

## Performances et mémoire

**Soak de deux heures du candidat final3**, sans carte, sur un circuit public de 165 composants, 154 jonctions et 393 fils :
- RUN observé de 7 200 s, aucun redémarrage moteur, journal complet, sortie 0 ;
- 12 879 échantillons ; les 17 contrôles du vérificateur indépendant passent.

| Mesure | Résultat | Limite | Verdict |
| --- | --- | --- | --- |
| Dérive de la mémoire privée (après 600 s d'échauffement) | 3,38 MiB/h = 3,55 Mo/h ; +4,5 MiB entre les extrémités | 20 Mo/h | **PASS** |
| Pic de mémoire privée (application et WebView2) | 333,9 MiB = 350,1 Mo | 300 Mo | **FAIL** |
| Surcoût du Physical Lab par rapport au cockpit seul | +121,6 Mo | +40 Mo | **FAIL** |

**Conclusion : mémoire stable sur cette charge, sans croissance persistante ; le budget absolu de 300 Mo reste dépassé.** Le pic vient du renderer et du processus GPU de WebView2.

L'exécutable final5 ne diffère de final3 que par l'interface : harness E2E et texte du bandeau de banc. Sa tentative de soak est **incomplète**. Après 480 s de RUN, la mémoire virtuelle de la machine a été épuisée par d'autres programmes, ce que Windows a diagnostiqué, et les deux collecteurs de mesure se sont arrêtés. Elle ne conclut ni stabilité ni dérive pour final5.

Deux essais GUI courts de 60 s ont été faits sur les exécutables de ce jour. Le premier passe ses étapes et ses budgets de rendu. Le second, sur l'exécutable final5, passe ses étapes mais compte une image longue, ce qui fait échouer le budget correspondant. Dans les deux cas, le surcoût mémoire du Physical Lab dépasse +40 Mo, et le watcher indépendant mesure un pic de 371,9 à 377,9 MiB. Lors du second essai, l'échantillonneur interne a relevé un pic transitoire de 585,2 Mo que le watcher, plus fin, ne voit pas ; les deux valeurs sont conservées. Ces limites étaient déjà ouvertes (budget de 300 Mo de S1.4, +40 Mo de Physical Lab v1).

## Limites

- L'alimentation de Board C s'est montrée instable : tension de la cible tombée de 0,64 à 0,02 V, avec des redémarrages spontanés. Les campagnes concernées ont été écartées et rejouées après réparation. Board D a redémarré une fois, sans cause connue, en dehors de tout parcours.
- Le pack et le code d'un groupe, livrés séparément, permettent à ce groupe de déchiffrer son propre mapping hors du dépôt étudiant (limite D7).
- Le confinement actuel du dépôt encadrant et le renouvellement des secrets ne rendent pas confidentielles d'anciennes copies ou connaissances.
- Le verrou de mission protège le parcours prévu ; il ne protège pas contre la modification de fichiers par le propriétaire du poste.

La clôture précédente reste décrite dans [le rapport de revue de la release 0.2](release-0.2-closure-r1.md).
