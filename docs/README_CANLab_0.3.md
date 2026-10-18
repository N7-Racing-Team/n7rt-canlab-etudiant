# N7RT CAN Lab 0.3 — documents

Cinq documents accompagnent la release 0.3. Un seul est destiné aux étudiants ; les quatre autres sont réservés aux
encadrants et ne figurent jamais dans l'archive étudiante.

| Document | Public | Où le trouver | À quoi il sert |
|---|---|---|---|
| `N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf` | Étudiants des quatre missions | Archive étudiante, dossier `docs\` | Prendre en main l'application et démarrer le TP sans être perdu : installation, carte NUCLEO, pack et mission, interface, fiche, bus CAN, Physical Lab, multimètre, code STM32, build, flash, CRC, vérifications, qualification, clôture et export. Il ne contient aucune réponse ni aucune donnée propre à une mission. |
| `N7RT_CANLab_0.3_Corrige_APPS.pdf` | Encadrants | Archive des outils encadrant | Corrigé et guide d'accompagnement de la mission APPS (pédale d'accélérateur). |
| `N7RT_CANLab_0.3_Corrige_WSF.pdf` | Encadrants | Archive des outils encadrant | Corrigé et guide d'accompagnement de la mission WSF (vitesse des roues avant). |
| `N7RT_CANLab_0.3_Corrige_SAS.pdf` | Encadrants | Archive des outils encadrant | Corrigé et guide d'accompagnement de la mission SAS (angle de volant). |
| `N7RT_CANLab_0.3_Corrige_BPS.pdf` | Encadrants | Archive des outils encadrant | Corrigé et guide d'accompagnement de la mission BPS (pression de freinage). |

## Pour les étudiants

Commencez par `docs\demarrage-5-minutes.md` (cinq minutes), puis gardez le tutoriel PDF ouvert pendant la première
séance. Les problèmes connus et leurs contournements sont dans `docs\known-issues-0.3.md`.

## Pour les encadrants

Chaque corrigé est confidentiel : réponses exactes du pack de référence V0, toutes les variantes d'incident tirées pour
les groupes, décodeur de référence expliqué fonction par fonction, critères de réussite de chaque étape, erreurs
classiques et trois niveaux d'aide. Il complète le manuel encadrant (`docs\manuel-encadrant-0.3.md` du dépôt privé).
Ne le projetez pas, ne l'imprimez pas pour une salle d'étudiants et ne le copiez pas dans un dossier partagé avec eux.

## Sources

Les PDF sont construits à partir de sources conservées dans les dépôts (Typst, spécifications de figures, captures
annotées de l'application finale, données de réponse générées depuis les vérités) : tutoriel dans le dépôt public
(`docs/pdf/`, `tools/docs/build_pdf.py`), corrigés dans le dépôt privé (`docs/corriges/`,
`tools/docs/build_corriges.py`).
