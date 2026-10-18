# Bibliothèque documentaire des missions

Ce dossier décrit les documents techniques des missions du N7RT CAN Lab. Il ne contient aucun PDF, ni dans le dépôt ni dans l'archive de la release : les conditions de redistribution des éditeurs ne sont pas toutes établies. Chaque document est cité avec l'adresse de son éditeur (fabricant ou organisme) et la révision retenue. L'application ouvre chaque document à l'adresse de son éditeur : un accès internet est nécessaire. Aucun document n'est fabriqué ni adapté.

## Organisation

| Dossier | Contenu | Documents |
|---|---|---|
| `common/` | Documents communs aux quatre missions | 5 |
| `apps/` | Mission APPS | 2 |
| `wsf/` | Mission WSF | 10 |
| `sas/` | Mission SAS | 5 |
| `bps/` | Mission BPS | 6 |

Chaque dossier contient un `manifest.json` : pour chaque document, le titre, l'éditeur, la référence, la révision, la date, l'adresse de téléchargement chez l'éditeur (`primary_url`), l'empreinte SHA-256 de la révision retenue, les missions concernées et le nom de sa copie locale (`local_file`). Les entrées sont triées par `local_file`. Le format est décrit par `shared/schemas/library-manifest.schema.json`.

Les noms de copie locale suivent `<éditeur>-<référence>-<type>-<révision>.pdf`, en minuscules ASCII. Quand l'éditeur n'imprime pas de numéro de révision, la date de publication en tient lieu.

`links.json` liste les documents cités par lien (ci-dessous) ; son format est décrit par `shared/schemas/library-links.schema.json`.

## Mode d'emploi

1. Dans l'application, le panneau « Bibliothèque » de la mission ouverte liste ses documents, triés par titre : ceux de son dossier, ceux de `common/` qui la concernent et ses documents cités par lien. Un clic ouvre le document à l'adresse de son éditeur.
2. La bibliothèque fige une révision. L'éditeur peut avoir publié une révision plus récente à l'adresse `primary_url` : c'est la révision indiquée dans le manifeste qui sert de référence pour la mission.
3. Copie locale, facultative, pour travailler sans connexion : télécharger le document à son adresse `primary_url`, vérifier son empreinte, puis l'enregistrer sous le nom `local_file`, dans le dossier de son `manifest.json` (le dossier `library` placé à côté de l'exécutable).
   - Windows (PowerShell) : `Get-FileHash -Algorithm SHA256 <fichier>`
   - Linux ou macOS : `sha256sum <fichier>`

   L'application ouvre la copie locale seulement si son empreinte est celle du champ `sha256` ; sinon, elle ouvre l'adresse de l'éditeur. Une empreinte différente signale une autre révision ou un fichier altéré.
4. Les copies locales restent sur le poste : elles ne sont jamais versionnées (`.gitignore` exclut `/library/**/*.pdf`) ni redistribuées. Ne pas modifier les manifestes ni `links.json` : l'application y lit la liste des documents.

## Documents cités par lien

Les documents ci-dessous figurent dans les listes des missions mais n'ont pas d'entrée dans les manifestes : leur éditeur n'autorise pas leur reproduction, ou les conditions de reproduction ne sont pas établies, et la bibliothèque ne fige pas leur empreinte. Ceux qui figurent dans `links.json` apparaissent dans le panneau « Bibliothèque » de leur mission et s'ouvrent toujours à l'adresse de leur éditeur, sans copie locale ; les autres se consultent à l'adresse indiquée ci-dessous. Tous se consultent à la révision indiquée. Les documents N7RT (énoncé de la mission, en-tête `n7rt.h`, documentation du SDK) sont fournis avec la mission et le SDK.

### Documents communs aux quatre missions (`common/`)

- CAN Specification Version 2.0 — Robert Bosch GmbH, CAN Specification 2.0 (Part A, Part B), Version 2.0, copie d'archive : <https://web.archive.org/web/20171215103144id_/http://www.bosch-semiconductors.de:80/media/ubk_semiconductors/pdf_1/canliteratur/can2spec.pdf>
- E2E Protocol Specification — AUTOSAR, AUTOSAR_FO_PRS_E2EProtocol (Document ID 849), R25-11 : <https://www.autosar.org/fileadmin/standards/R25-11/FO/AUTOSAR_FO_PRS_E2EProtocol.pdf>
- Formula Student Rules 2026 — Formula Student Germany (FSG), Rev-15f99a7, Version 1.1 : <https://www.formulastudent.de/fileadmin/user_upload/all/2026/rules/FS-Rules_2026_v1.1.pdf>
- Formula Student Rules 2027 — Formula Student Germany (FSG), FS-Rules-2027, Version 1.0 : <https://www.formulastudent.de/fileadmin/user_upload/all/2027/rules/FS_Rules_2027_v1.0.pdf>
- Specification of CRC Library — AUTOSAR, AUTOSAR_CP_SWS_CRCLibrary (Document ID 16), R25-11 : <https://www.autosar.org/fileadmin/standards/R25-11/CP/AUTOSAR_CP_SWS_CRCLibrary.pdf>

### Mission APPS (`apps/`)

- Formula Student Inspection Sheet – Electric — Formula Student Germany (FSG), 2026_FSG_v1.0 (build 4b8e7f9), 2026_FSG_v1.0 : <https://www.formulastudent.de/fileadmin/user_upload/all/2026/important_docs/FSG26_InspectionSheet_EV_v20260713.pdf>
- Formula Student Rules 2023 — Formula Student Germany (FSG), Rev-44c3679, Version 1.1 : <https://www.formulastudent.de/fileadmin/user_upload/all/2023/rules/FS-Rules_2023_v1.1.pdf>
- MLX90371 – Triaxis Position Processor, Datasheet — Melexis, MLX90371, Revision 004 : <https://www.melexis.com/-/media/files/documents/datasheets/MLX90371-datasheet-melexis.pdf>
- MLX90372 – Triaxis Position Processor, Datasheet — Melexis, 3901090372, REVISION 8 : <https://www.melexis.com/-/media/files/documents/datasheets/MLX90372-datasheet-melexis.pdf>
- MLX90377 – Triaxis Position Sensor IC, Datasheet — Melexis, 3901090377, REVISION 003 : <https://media.melexis.com/-/media/files/documents/datasheets/mlx90377-datasheet-melexis.pdf>
- TLE4997A8(D) Programmable Single/Dual Die Linear Hall Sensor, Data Sheet — Infineon Technologies AG, TLE4997A8(D), Revision 1.1, copie distributeur : <https://www.farnell.com/datasheets/3627096.pdf>

### Mission SAS (`sas/`)

- MLX90377 – Triaxis Position Sensor IC, Datasheet — Melexis, 3901090377, REVISION 003 : <https://media.melexis.com/-/media/files/documents/datasheets/mlx90377-datasheet-melexis.pdf>
- Offer drawing – Steering Angle Sensor LWS7.3.6, order no. F02U.V02.894-01 — Bosch Engineering GmbH (Bosch Motorsport), F 02U A02 894-01, Index 01 : <https://www.bosch-motorsport.com/content/downloads/Raceparts/Resources/pdf/Offer%20Drawing_191958539_Steering_Wheel_Angle_Sensor_LWS.pdf>

### Mission BPS (`bps/`)

- Brake Light Switch Pressure Type Bullet ALL76251 — Allstar Performance, ALL76251, page produit non datée : <https://allstarperformance.com/brake-light-switch-pressure-type-bullet-all76251/>
- Formula Student Inspection Sheet – Electric — Formula Student Germany (FSG), 2026_FSG_v1.0 (build 4b8e7f9), 2026_FSG_v1.0 : <https://www.formulastudent.de/fileadmin/user_upload/all/2026/important_docs/FSG26_InspectionSheet_EV_v20260713.pdf>
- Formula Student Rules 2023 — Formula Student Germany (FSG), Rev-44c3679, Version 1.1 : <https://www.formulastudent.de/fileadmin/user_upload/all/2023/rules/FS-Rules_2023_v1.1.pdf>
- Installation Instructions for the Heavy Duty Pressure Transducers PX3 Series — Honeywell International Inc., 32313758-G-EN, Issue G, copie distributeur : <https://cdn.components.ru/media/PDF/8062774.pdf>
- Offer drawing, rail pressure sensor DS-HD-KV4.2 26 MPa, no. 0 261 545 040 — Robert Bosch GmbH, 0 261 A04 407, AGZ 001 01 : <https://www.bosch-motorsport.com/media/catalog_content/downloads_catalog/pdf_catalog/offer_drawing_69607563_pressure_sensor_fluid_pss-260.pdf>
