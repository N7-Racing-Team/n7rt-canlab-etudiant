# Physical Lab v1 — rapport de validation

Date : 2026-10-05. Contrat : [physical-lab-v1.md](physical-lab-v1.md) (v2). Application réelle : `app/target/release/n7rt-can-lab.exe`, construite le 2026-10-05 (profil release, `tauri/custom-protocol`).

**Rapport historique.** Les résultats, comptes de suites, mesures et captures ci-dessous proviennent du checkpoint du 5 octobre ; ils ne valident pas le candidat courant. Les tableaux décrivent les mesures effectivement relevées, pas un fixture de réponses à importer dans les tests. Pour une nouvelle validation, calculer les contrôles à partir du montage et des lois du contrat courant. Le retrait des jeux fixes ne réécrit ni ces mesures ni leurs verdicts.

## 1. Verdict

- **Fonctionnel historique : conforme.** Le scénario de bout en bout tourne dans l'application Tauri réelle. Les 16 étapes passaient les contrôles du §9.1 alors en vigueur lors de trois exécutions consécutives sur le binaire de ce checkpoint (22:43, 22:45, 22:47).
- **Fluidité : conforme.**
  - Aucun gel au-delà de 50 ms : pire écart entre images 9,1 ms.
  - Aucune image longue.
  - Fil de calcul : 0,07 % de la machine en moyenne.
- **Mémoire : budget non tenu, à arbitrer.**
  - Pic : +178 MB par rapport au cockpit seul, pour un budget de +40 MB.
  - Avec le document chargé et au repos, l'écart n'est que de +28 MB.
  - Le pic vient des caches de WebView2 (processus GPU et moteur de rendu), pas du Physical Lab lui-même ; le §6 détaille les mesures.
- Protocole, runtime et SDK n'ont pas été modifiés. La suite existante reste verte (§4).

## 2. Lancement

Depuis `app/`, sous Windows, après chargement de l'environnement `scripts/windows-gnu-env.ps1` :

| Usage | Commande |
|---|---|
| Développement | `npm run tauri dev` |
| Exécutable autonome | `npm run tauri build -- --no-bundle`, puis `target/release/n7rt-can-lab.exe` |
| Scénario automatique dans l'application réelle | `python tools/stm32/stm32lab.py app-test --phys --scenario phys --release` (depuis la racine) |
| Mesures de performance | `python tools/stm32/stm32lab.py app-test --phys --scenario phys-perf --release` |

Le Physical Lab s'ouvre par l'icône d'oscillogramme de la barre d'activité, à gauche. Aucune carte n'est nécessaire.

## 3. Test manuel de 3 minutes

Montage de référence du §9.1 : alimentation 5 V, interrupteur côté haut, potentiomètre 10 kΩ lu par ADC1, filtre RC 10 kΩ / 10 µF lu par ADC2, ADC 12 bits avec Vref 5 V. Pour gagner du temps : **Fichier → Démonstration → « Référence : potentiomètre… » → Charger**.

| # | Action | Contrôle calculé pour une nouvelle validation |
|---|---|---|
| 1 | Ouvrir la vue Physical Lab, charger la démonstration de référence | Schéma affiché, 5 nets, aucun diagnostic |
| 2 | RUN | ADC1 suit la loi du diviseur ; ADC2 converge vers cette lecture avec le τ calculé du montage |
| 3 | Sélectionner RV1, déplacer le curseur (champ ou symbole) | ADC1 varie dans le même sens ; ADC2 suit la réponse RC, sans saut de tension du condensateur |
| 4 | Clic sur la lame de S1 (ouvrir), puis refermer | Décharge par le chemin résistif restant, puis retour vers le régime établi calculé |
| 5 | Modifier R1 en RUN | τ suit la résistance de Thévenin, sans redémarrage ni saut |
| 6 | Modifier C1 en RUN | τ suit la capacité, tension du condensateur continue |
| 7 | Lire l'inspecteur de RV1 | Tensions, courant et puissance satisfont les lois du diviseur, d'Ohm et de puissance ; brochage et état cohérents |
| 8 | Onglets Nets et Diagnostics de la console | 5 nets ; aucun diagnostic |
| 9 | STOP, Enregistrer sous un nom, Nouveau, Ouvrir ce nom | Circuit identique |
| 10 | Glisser un composant, Suppr, Ctrl+Z, Ctrl+Y ; molette (zoom) ; Espace + glisser (pan) | Annuler et rétablir exacts, zoom par pas de 1,25 |

Le scénario automatique `--scenario phys` rejoue les étapes 1 à 9 en construisant le circuit pièce par pièce (placement, câblage avec une jonction sur le fil du curseur), puis vérifie la bascule vers le cockpit et les deux démonstrations.

## 4. Résultats automatiques

| Suite | Résultat |
|---|---|
| Rust (`n7rt-link`, `n7rt-solver`, `n7rt-circuit`, `n7rt-lab`) | 256 réussis : 5 + 40 + 82 + 129 ; 4 ignorés (mesures et régénération) ; 0 avertissement |
| Interface (Vitest) | 210 réussis, 19 fichiers ; `tsc --noEmit` sans erreur |
| Garde du design system (`npm run check`) | OK : 57 fichiers TS, 5 CSS, `index.html` ; géométrie carrée vérifiée, y compris dans le Canvas |
| Outils Python (`pytest tools/stm32/tests`) | 126 réussis, 2 ignorés |
| Application réelle, `--scenario phys` | **PASS** ×3 : `data/logs/app_phys_20261005-224327.json`, `-224532`, `-224737` |
| Application réelle, `--scenario phys-perf` | Tous les critères de temps PASS ; mémoire FAIL : `data/logs/app_phys-perf_20261005-224400.json` |

Valeurs historiques relevées dans l'application (exécution de 22:45). La colonne centrale rappelle la loi vérifiée ; les chiffres mesurés sont conservés comme preuve de cette exécution, sans constituer le jeu de réponses d'une future campagne :

| Grandeur | Loi ou propriété contrôlée | Mesuré le 5 octobre |
|---|---|---|
| V(W) à 0+, C1 déchargé (aperçu en STOP) | Réseau instantané avec tension initiale de C1 conservée | 1,2396668 V |
| τ de charge de C1 | C1 multiplié par la résistance de Thévenin | 121,00 ms |
| V(W), ADC1, ADC2 en régime | Diviseur et quantification de la tension finale | 1,4999968 V ; 1228 ; 1228 |
| Curseur 0,7 : V(W) à 0+ ; ADC1 à 0+ | Redistribution avec tension de C1 continue | 3,1528849 V ; 2582 |
| Curseur 0,7 : V(W), ADC1, ADC2 en régime | Nouveau diviseur et quantification | 3,4999928 V ; 2867 ; 2867 |
| S1 ouvert : V(W) à 0+ ; ADC1 à 0+ ; τ | Décharge par le chemin résistif restant | 1,4411879 V ; 1180 ; 169,78 ms |
| S1 refermé : τ ; ADC1 | Retour au réseau alimenté | 120,98 ms ; 2867 |
| R1 = 22 kΩ en RUN : τ | Nouvelle résistance de Thévenin | 241,00 ms, sans redémarrage |
| C1 = 4,7 µF en RUN : τ ; ADC2 avant le changement | τ proportionnel à C1 ; tension continue | 113,25 ms ; 1228 constant |
| Inspecteur RV1 à 0,3 : V(B), I, P | Diviseur, loi d'Ohm et P = V·I | 4,99999 V ; 499,999 µA ; 2,49999 mW |
| Enregistrer puis recharger | document identique | identique (2392 caractères) |

## 5. Revue des démonstrations

Chaque démonstration est chargée dans l'application réelle, en RUN pendant au moins 3 s ; la capture est faite avec l'élément clé sélectionné.

**`ref-pot-rc`** (§9.1) — capture `img/physical-lab/phys-demo-ref-pot-rc.png`.

| Point vérifié | Attendu | Observé |
|---|---|---|
| Polarité de V1 | + vers S1, − à la masse | conforme |
| Potentiomètre | A à la masse, B côté interrupteur, W vers ADC1 et R1 | conforme, curseur à 0,300 |
| V(B), V(A), V(W) | Loi du diviseur avec A à la masse | 4,99999 V ; 0,00000 V ; 1,50000 V |
| RC | R1 10 kΩ, C1 10 µF, ADC2 sur le point milieu | conforme |
| ADC | 12 bits, Vref 5 V, REF à la masse | conforme |
| Diagnostics | aucun | aucun |

**`tp1-capteur-can`** (§9.2), base du premier TP — capture `img/physical-lab/phys-demo-tp1-capteur-can.png`.

| Point vérifié | Attendu | Observé |
|---|---|---|
| Alimentation du MCP2551 | VDD sur le 5 V de l'ECU, VSS à la masse | VDD 4,99899 V ; VSS 0 V |
| Liaison logique | TXD sur U1.PD1, RXD sur U1.PD0 | TXD 3,30337 V (récessif) ; RXD 4,99899 V |
| Mode Rs | Rs à la masse : haute vitesse | « Mode Rs : haute vitesse » |
| Bus récessif | Symétrie CANH/CANL et bias du modèle | 2,4997483 V tous les deux ; CANH − CANL = 0,00000 V |
| VREF | ≈ VDD/2 | 2,49950 V |
| Terminaison | RT1 120 Ω entre CANH et CANL, un nœud distant sur BUS1 | conforme |
| Diagnostics | aucun | aucun |

## 6. Performances

Exécution `phys-perf` du 2026-10-05 à 22:44, écran 144 Hz (T = 6,9 ms), document de 150 composants et 300 fils.

| Critère | Budget | Mesuré | |
|---|---|---|---|
| Chargement jusqu'à la première image | ≤ 200 ms | 23 ms | PASS |
| Écart entre images, p95 | ≤ 1,1 T (7,6 ms) | 7,1 ms | PASS |
| Écart entre images, p99 | ≤ 2 T + 1 ms (14,8 ms) | 7,5 ms | PASS |
| Écart entre images, max (gel) | ≤ 50 ms | 9,1 ms | PASS |
| Dessin, p95 / p99 | ≤ 8 / 12 ms | 1,3 / 1,5 ms | PASS |
| Images longues > 50 ms | 0 | 0 | PASS |
| Entrée → dessin, p95 | ≤ 50 ms | 7,6 ms | PASS |
| Modification → valeurs, p95 | ≤ 100 ms | 20,7 ms | PASS |
| Retard du moteur, p99 | ≤ 20 ms | 0 ms | PASS |
| CPU du fil `phys` | ≤ 5 % | 0,07 % (max 1,16 %) | PASS |
| CPU de l'arbre de processus | (relevé) | 6,4 % (max 19,6 %) | — |
| Solveur | pas de redémarrage | trame p95 692 µs ; 11 îlots ; pas min 500 µs ; 0 redémarrage | PASS |
| Mémoire privée, écart au cockpit seul | ≤ +40 MB | pic +178 MB ; fin +153 MB | **FAIL** |

Côté Rust (test ignoré `n7rt-circuit/tests/perf.rs`, release) : contrôle du document 1 ms ; aperçu du déplacement de l'ECU 1 à 6 ms.

**Mémoire.**

| Point de mesure | Mémoire privée | Écart au cockpit |
|---|---|---|
| Cockpit seul | 196,7 MB | — |
| Document chargé, au repos | 224,6 MB | +27,9 MB |
| Pic, pendant les manipulations | 375,0 MB | +178,3 MB |
| Fin, 12 s après les manipulations | 349,7 MB | +153,0 MB |

Au pic, l'écart se répartit ainsi : +109,5 MB pour le processus GPU de WebView2 (caches des deux canvas accélérés), +65,2 MB pour le moteur de rendu, +2,4 MB pour l'application. Le cache GPU se stabilise puis se libère au repos ; une exécution antérieure, sur batterie à 60 Hz, montait à +222,6 MB. C'est la même famille d'écart que celle du cockpit S1.4 (327 MB pour un budget de 300 MB).

Deux choix pour l'arbitrage :
- juger un état stabilisé (document chargé, au repos : +28 MB, conforme) ;
- ou garder le pic, en limitant les caches de WebView2.

La dérive sur 10 min de RUN n'est pas mesurée par ce scénario.

## 7. Captures

Fichiers dans [img/physical-lab/](img/physical-lab/), issus de l'exécution de 22:47. Le harnais capture désormais la fenêtre par `PrintWindow` : une fenêtre posée par-dessus n'apparaît plus dans l'image.

- `phys-editor.png` : circuit construit, en STOP ;
- `phys-run.png` : RUN, valeurs sur le schéma, inspecteur de RV1 ;
- `phys-inspector.png` : inspecteur, brochage, mesures, état ;
- `phys-demo-ref-pot-rc.png` et `phys-demo-tp1-capteur-can.png` : démonstrations.

## 8. Composants pris en charge

Table normative : [catalog.rs](../app/crates/n7rt-circuit/src/catalog.rs), résumée au §2 du contrat. Chaque valeur du modèle porte sa provenance (fiche constructeur ou `N7RT_MODEL`), affichée dans l'inspecteur.

| Composant | Désignateur | Remarques |
|---|---|---|
| Alimentation DC | V | 0–30 V, limite de courant (CC/CV), résistance de sortie |
| Masse | — | Référence 0 V ; plusieurs symboles, un seul net |
| Résistance | R | 10 mΩ–100 MΩ ; surcharge en puissance |
| Condensateur | C | 1 pF–1 F ; polarisé ou non ; surtension, inversion |
| Potentiomètre 3 bornes | RV | Curseur 0–1 au pas de 0,001, réglable sur le schéma ou dans l'inspecteur |
| Bouton poussoir | BP | NO/NC, momentané ou à accrochage ; R_on, R_off |
| Interrupteur | S | État ouvert/fermé dessiné |
| Entrée ADC | ADC | 8–16 bits, Vref 1–5 V, R_in (∞ possible), cadence diviseur de 1 kHz ; saturation, flottant |
| ECU STM32 (NUCLEO-H753ZI) | U | 5 V, 3V3, 2 entrées analogiques, CAN_TX/RX (CAN ou haute impédance), état USB |
| Transceiver CAN (MCP2551) | U | Seuils POR, mode Rs, VREF, entrée mode commun |
| Bus CAN (reste du véhicule) | BUS | 0–30 nœuds distants, terminaison distante |
| Terminaison 120 Ω | RT | Fixe, 0,25 W |
| Jonction | — | Créée par le câblage sur un fil, jamais posée seule |

## 9. Limites du solveur et du modèle

- Solveur nodal linéaire par morceaux, déterministe ; ce n'est pas SPICE : ni diode ni transistor.
- Pas de bit dominant ni de dynamique du bus CAN : le bus est statique (récessif).
- Pas de bruit.
- Quasi statique sous 10 µs (pas minimal).
- Consommation des circuits intégrés modélisée par une résistance équivalente, à la valeur maximale de la fiche.
- Courant interne de Rs (MCP2551) non modélisé ; VREF+ de l'ECU supposé égal au 3V3.
- Le test de pivot relatif (1e−13) peut déclarer singulier un circuit légal qui combine des conductances à 15 décades d'écart (cas extrême, aucun dans les démonstrations).
- Mesures de performance sous Windows seulement : ni macOS ni Linux ne sont construits.

## 10. Écarts connus, non bloquants

Relevés par les revues de code et d'ergonomie, puis vérifiés ; aucun n'affecte les démonstrations ni les valeurs du §9.

| Zone | Écart | Fichier |
|---|---|---|
| Moteur | Après une panique, la commande en quarantaine est refusée avec `engine_unavailable` au lieu de `engine_error` (§8.1) | `n7rt-lab/src/phys/handle.rs` |
| Moteur | Échantillonneur ADC en retard d'une trame pour une cadence < 1000 Hz (défaut : 1000 Hz, non concerné) | `n7rt-lab/src/phys/kernel.rs` |
| Sonde | Rs référencé à une autre masse (`NoReference`) signalé comme flottant | `n7rt-lab/src/phys/probe.rs` |
| Interface | « — » (non disponible) en ambre au lieu de la couleur de texte faible (DESIGN §5) | `ui/src/lab/units.ts` |
| Interface | Le nom prérempli « sans titre » est invalide : le premier Enregistrer d'un nouveau circuit est refusé | `ui/src/lab/panels/library.ts` |
| Interface | La confirmation d'écrasement s'affiche comme une erreur et se journalise « REFUS » | `ui/src/lab/state.ts` |
| Interface | Le compteur de l'onglet Journal reste à 0 tant que l'onglet n'a pas été ouvert | `ui/src/lab/panels/console.ts` |
| Interface | En-tête « Broche » tronqué en « BRO… » sans infobulle à la largeur par défaut | `ui/styles/lab.css` |
| Accessibilité | Lignes Diagnostics et Nets non atteignables au clavier | `ui/src/lab/panels/vlist.ts` |
| Harnais | Démonstrations chargées par le contrôleur et non par le panneau Fichier : l'ancien message « Circuit « e2e-phys » ouvert » reste affiché sur les captures | `ui/src/lab/e2e-phys.ts` |
| Harnais | ADC1 et ADC2 trop proches dans le scénario : « 12 bits · 5 V » chevauche ADC2 sur `phys-run.png` | `ui/src/lab/e2e-phys.ts` |
| Harnais | Suppression, annuler/rétablir, rotation, sélection au cadre et refus ne sont prouvés que par des tests unitaires (jsdom), pas dans l'application réelle | `ui/src/lab/e2e-phys.ts` |
| Performance | Image de 76 ms observée une fois et jamais reproduite depuis (cause non établie) | — |

## 11. Avant l'oscilloscope et le multimètre

- Sondes posées sur des broches (adresse `PinRef`) et affichées sur le schéma.
- Ohmmètre par courant de test injecté dans le réseau vivant (aujourd'hui `unpowered_resistance` ne mesure que hors tension).
- Mémoire d'échantillons par trame et par sous-pas, pour le déclenchement et la base de temps de l'oscilloscope.
- Mode pas à pas du moteur.
- Mode mission piloté par le moteur de missions (ch. 24 de la spécification véhicule, document du dépôt encadrant).
- Couche L3 du bus pour le bit dominant, nécessaire à toute mesure CAN dynamique.
- Arbitrage du budget mémoire (§6) avant d'ajouter des canvas d'instruments.
