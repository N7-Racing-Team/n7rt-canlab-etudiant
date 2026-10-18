# Physical Lab v1 — éditeur de schéma, modèle électrique, solveur

Statut : **contrat de conception v2** (2026-10-05), source de vérité pour l'implémentation. Il applique [can-lab-architecture.md](can-lab-architecture.md) §8, §9, §10 et §14 ; chaque écart est marqué **[écart]** et justifié (liste en §1.8).

La v1 de ce contrat a été relue par quatre critiques indépendants (électrique, éditeur, solveur, couverture). Cette v2 intègre leurs constats. Les interfaces entre les lots sont **compilées**, et ce document en donne la sémantique :

| Contrat compilé | Contenu |
|---|---|
| [app/crates/n7rt-solver/src/lib.rs](../app/crates/n7rt-solver/src/lib.rs) | réseau, éléments, portes, modes, lectures du solveur |
| [app/crates/n7rt-circuit/src/lib.rs](../app/crates/n7rt-circuit/src/lib.rs) | document, éditions, correctifs, refus, persistance, démonstrations |
| [app/crates/n7rt-circuit/src/catalog.rs](../app/crates/n7rt-circuit/src/catalog.rs) | **la bibliothèque elle-même** (broches, positions, paramètres, unités, plages, défauts, provenance) |
| [app/crates/n7rt-lab/src/phys/mod.rs](../app/crates/n7rt-lab/src/phys/mod.rs) | lectures, sonde, valeurs publiées, commandes, cœur, poignée du moteur |
| [app/ui/src/lab/types.ts](../app/ui/src/lab/types.ts), [api.ts](../app/ui/src/lab/api.ts), [state.ts](../app/ui/src/lab/state.ts), [editor.ts](../app/ui/src/lab/editor.ts) | miroir TypeScript du JSON, ordre des appels, état partagé de l'UI, interface de l'éditeur |

## 0. Périmètre

**Dans le périmètre.**
- Vue « Physical Lab » dans l'application Tauri existante, avec le design system ([app/ui/DESIGN.md](../app/ui/DESIGN.md)).
- Éditeur Canvas2D :
  - pan, zoom, sélection, multi-sélection, déplacement, rotation, suppression, annuler/rétablir ;
  - câblage orthogonal pin→pin et pin→fil (jonction).
- Modèle de graphe électrique en Rust, indépendant du rendu : composants, broches, fils, nets, paramètres, états, transactions journalisées.
- Bibliothèque V1 de 12 composants plaçables (§2).
- Solveur déterministe : analyse nodale par îlot, transitoire, états invalides explicites (§5).
- API de sonde pour les futurs instruments, le moteur CAN et les missions (§6).
- RUN / STOP / RESET du moteur de simulation, indépendants de la session STM32.
- Sauvegarde et chargement exacts (§7) ; deux démonstrations vérifiées (§9).
- Tests (§10), mesures de performance (§8.6), traçabilité des critères d'acceptation (§12).

**Hors périmètre** :
- oscilloscope, multimètre et analyseur CAN complets ;
- couche bus CAN détaillée : états dominants, liaison L3 ;
- bruit analogique ;
- stubs, missions, serveur ;
- boutons « injecter la panne X ».

**Interdit** : modifier N7RT-Link, le runtime ou le SDK, sauf défaut bloquant démontré et documenté. La suite de tests existante et `npm run check` restent verts.

## 1. Conventions

### 1.1 Unités
- SI, jamais implicites : V, A, Ω (écrit U+03A9), F, W, s, Hz. Chaque paramètre déclare unité, minimum, maximum et défaut ([catalog.rs](../app/crates/n7rt-circuit/src/catalog.rs)) ; toutes les valeurs sont finies.
- Syntaxe de saisie, affichage et validation : §4.
- Aucune grandeur n'est inventée pour l'affichage : une valeur affichée vient d'un paramètre ou d'une lecture de la sonde (§6), sinon l'état invalide est affiché tel quel.

### 1.2 Temps simulé
- Entier en ns (`i64`), pas logique (« frame ») de 1 ms (`FRAME_NS`).
- Les éditions et interactions sont validées dans le modèle **avant** la réponse à l'UI. Leur effet électrique est calculé :
  - en RUN, au début de la frame suivante ;
  - en STOP, immédiatement, par un aperçu (§5.6).

### 1.3 Grille et repère
- Coordonnées entières en unités de grille ; 1 unité = 10 px CSS au zoom 1 ; |x|, |y| ≤ 10 000.
- x vers la droite, y vers le bas. Toutes les broches tombent sur la grille.
- Rotation horaire par pas de 90° autour de l'origine du composant : `R90 : (x, y) → (−y, x)`, `R180 : (−x, −y)`, `R270 : (y, −x)`. Pas de miroir en V1.

### 1.4 Identifiants
- `CompId` et `WireId` viennent d'un seul compteur `next_id`, qui ne fait que croître et **n'est jamais restauré** par annuler : un identifiant n'est jamais réutilisé.
- `PinRef { comp, pin }` : `pin` est l'identifiant ASCII de la bibliothèque (`p1`, `w`, `vdd`). Adresse durable des instruments et des missions.
- `NetKey` : le plus petit `PinRef` du net. Il change quand une broche plus petite rejoint ou quitte le net : il sert à l'affichage et aux différences, jamais d'adresse durable.

### 1.5 Désignateurs et noms de nets
- Désignateur automatique = préfixe + plus petit numéro libre (`R1`, `RV2`, `U3`). Modifiable, unique, `[A-Za-z][A-Za-z0-9_]{0,15}`. Pas de désignateur pour la masse et les jonctions.
- Nom d'un net :
  - « GND » pour le net de masse ;
  - sinon `<désignateur>.<nom court>` de sa plus petite broche hors jonction (« R1.2 », « U1.PD0 ») ;
  - « J<id> » s'il ne contient que des jonctions.
- Un nom ne change donc que si le net lui-même change ; aucune renumérotation globale.

### 1.6 Symboles
Convention complète en §2.2.
- IEC 60617 : traits de 1,5 px, angles droits (sauf les obliques prévues par la norme : lame, flèche), pas de cercle ni d'arc. Deux écarts, dus à la règle « aucune forme ronde » du design system :
  - [écart] une jonction est un **carré plein**, une broche non reliée un **carré creux**, au lieu des pastilles rondes ;
  - [écart] l'alimentation de labo utilise le symbole de source continue à deux plaques, annoté « 5 V · 0,1 A », au lieu du cercle de la source de tension.
- Aucun style breadboard ou dessin animé.

### 1.7 Classes de provenance
Chaque valeur de modèle porte sa classe (§2.1) :
- **DATASHEET** : fiche technique, référence, révision et page ;
- **STANDARD** : norme (ISO 11898-2) ;
- **ARCHITECTURE** : choix déjà fixé par l'architecture ;
- **N7RT_MODEL** : choix de modélisation documenté et justifié, jamais présenté comme une mesure.

### 1.8 Écarts du protocole V1 historique
La table suivante décrit le checkpoint V1 initial. Les missions et composants ajoutés dans le candidat courant sont décrits plus loin et dans [le rapport des quatre missions](missions-four-r1.md).
| Écart | Architecture | V1 | Justification |
|---|---|---|---|
| Plancher du pas | §10.1 : 1 µs | 10 µs, condensateurs de τ < 10 µs quasi statiques | §14 : ≤ 100 sous-pas par îlot et par ms. Un découplage de 100 nF sur 10 mΩ (τ = 1 ns) imposerait sinon 1000 sous-pas par ms à tout l'îlot |
| Pas-à-pas, pause | §9.1 | RUN / STOP / RESET seulement | STOP fige déjà le temps. Le pas-à-pas viendra avec l'oscilloscope |
| Composants V1 | §10.6 : régulateur, CTN, capteur ratiométrique | absents | Livrés avec les missions capteurs. La bibliothèque est extensible sans changer le contrat |
| Fichier de session | §8 : `sessions/…/journal.ndjson` | préfixe déversé au-delà de 10 000 entrées, queue de 1 000 entrées en RAM ; perte signalée si le stockage est absent ou échoue | Le fichier seul n'est pas une archive complète finalisée ; la RAM reste bornée dans tous les modes |
| Pastilles de jonction ; symbole de l'alimentation | IEC : point rond ; cercle | carré plein ; deux plaques annotées | Design system : aucune forme ronde |
| Image du runtime | — | PD1 récessif (image `--can-bus`) par défaut, `can_mode` = `hi_z` pour l'image sans FDCAN | Le dominant n'est pas modélisé en V1 |
| Mode mission | §9.3 complet | filtre minimal dans le cœur (§3.8) | Les missions n'existent pas encore |
| Hachage identique sur 3 OS | §10.4 | vérifié sous Windows seulement | macOS et Linux pas encore construits |
| Budget mémoire | §14 : 300 Mo absolus | delta ≤ +40 Mo par rapport au cockpit | Le cockpit mesure déjà 327 Mo (S1.4) |

Cette table décrit les choix du protocole historique Physical Lab V1. Le delta au cockpit et les mesures courtes de ce protocole ne valent pas dérogation au budget de la clôture 0.2. La revue courante dans `docs/release-0.2-closure-r1.md` du clone actif `N7-Racing-Team/n7rt-canlab` juge séparément le budget absolu de 300 MB, la dérive de 20 MB/h et une campagne représentative de deux heures sur le candidat final. Aucune acceptation du dépassement absolu ne se déduit des mesures historiques.

## 2. Bibliothèque V1

La table normative est [catalog.rs](../app/crates/n7rt-circuit/src/catalog.rs). Pour chaque kind, elle fixe :
- broches : id, nom complet, nom court, position, direction sortante, rôle, obligatoire ;
- corps (rectangle sans les pattes) ;
- paramètres : nom, libellé, unité, type, plage, défaut, modifiable ;
- états d'interaction et zones d'actionneur ;
- provenance.

Résumé :

| `kind` | Désignateur | Broches | Paramètres | États |
|---|---|---|---|---|
| `dc_supply` | V | `pos` +, `neg` − | `voltage` 0–30 V (5) ; `current_limit` 1 mA–5 A (0,1 A) ; `r_out` 1 mΩ–10 Ω (10 mΩ) | `on` |
| `ground` | — | `gnd` | — | — |
| `resistor` | R | `p1`, `p2` | `resistance` 10 mΩ–100 MΩ (10 kΩ) ; `power_rating` 62,5 mW–10 W (0,25 W) | — |
| `capacitor` | C | `p1`, `p2` (« + », « − » si polarisé) | `capacitance` 1 pF–1 F (10 µF) ; `voltage_rating` 1–1000 V (25 V) ; `polarized` (faux) | — |
| `potentiometer` | RV | `a`, `b`, `w` | `resistance` 10 Ω–10 MΩ (10 kΩ) ; `power_rating` 50 mW–5 W (0,25 W) | `position` 0–1 (0,5) |
| `push_button` | BP | `p1`, `p2` | `contact` {`no`, `nc`} ; `mode` {`momentary`, `latching`} ; `r_on` 1 mΩ–10 Ω (0,1 Ω) ; `r_off` 1 MΩ–1 TΩ (100 MΩ) | `pressed` (transitoire, jamais sauvegardé), `latched` |
| `switch` | S | `p1`, `p2` | `r_on` (10 mΩ) ; `r_off` (1 GΩ) | `closed` |
| `adc_input` | ADC | `in`, `ref` | `vref` 1–5 V (3,3 V) ; `bits` 8–16 (12) ; `r_in` 1 kΩ–1 TΩ ou ∞ (∞ : entrée idéale) ; `sample_rate` diviseur de 1000 Hz (1000) | — |
| `ecu_stm32` | U | `v5`, `v3v3`, `gnd`, `a0`, `a1`, `can_tx`, `can_rx` | `adc_bits` {8, 10, 12, 14, 16} ; `adc_sample_rate` ; `can_mode` {`can`, `hi_z`} ; `r_out_5v` (0,1 Ω) ; `r_out_3v3` (0,05 Ω) | `usb` |
| `can_transceiver` | U | `txd`, `vss`, `vdd`, `rxd`, `vref`, `canl`, `canh`, `rs` | `r_in` 10–50 kΩ (25 kΩ) | — |
| `can_bus` | BUS | `canh`, `canl`, `gnd` | `remote_nodes` 0–30 (1) ; `remote_r_in` 10–50 kΩ (25 kΩ) ; `remote_termination` (vrai) | — |
| `termination` | RT | `p1`, `p2` | `resistance` 120 Ω fixe ; `power_rating` 0,25 W fixe | — |
| `ecu_apps` | U | `lv_in`, `gnd`, `v5_1`, `sig_1`, `gnd_s1`, `v5_2`, `sig_2`, `gnd_s2`, `can_h`, `can_l` | paramètres cachés seulement (aucun visible) | — |
| `mlx90377` | U | `vdd1`, `vss1`, `out1`, `vdd2`, `vss2`, `out2` | paramètres cachés seulement (aucun visible) | `position` 0–1 (0), course de la pédale |
| `junction` | — | `j` | — (non plaçable) | — |

**Bibliothèque version 3** (`LIBRARY_VERSION` = 3 ; lecture des versions 1, 2 et 3) : la version 2 avait ajouté `ecu_apps` et `mlx90377`, pièces du banc APPS, plaçables aussi en exploration. La version 3 ajoute les pièces physiques décrites ci-dessous. Un ancien fichier ne peut pas contenir un kind apparu dans une version ultérieure.
- **Paramètres cachés** (`ParamDef.hidden`) : valeurs internes d'une boîte noire ou d'une pièce de mission. Gardés dans le document et dans les fichiers enregistrés ; jamais envoyés à l'UI (bibliothèque, instantané, correctifs, inspecteur) ; jamais modifiables (refus `param_hidden`, à la pose comme par `set_param`). Les défauts du catalogue sont **neutres** ; les valeurs réelles ne viennent que du fichier de banc du pack privé.
- `ecu_apps` (« ECU APPS (boîte noire) ») : R_ECU entre LV_IN et GND ; porte de reset sur LV_IN − GND qui porte deux rails `Supply` un quadrant limités (5V_1, 5V_2) et le côté bus du modèle MCP2551 (réseau en T de 25 kΩ, polarisation récessive) ; par canal, diviseur SIG_x → GND avec condensateur anti-repliement au point milieu, et diviseur de mesure du rail ; GND_S1 et GND_S2 reliées à GND (1 mΩ). Canaux ADC de `CircuitProbe::adc` : `sig1`, `sig2` (milieux des diviseurs SIG), `sup1`, `sup2` (milieux des diviseurs de rail), loi de `adc_input` à 12 bits, 3,3 V, à chaque frame ; Unavailable(`ecu_unpowered`) en reset. État de fonctionnement : `ecu` (« Alimenté » / « En reset ») ; mesures en exploration : celles du connecteur seulement.
- `mlx90377` (« Capteur de position double puce ») : par puce, R_IDD et découplage VDD–VSS, porte de démarrage sur VDD − VSS qui porte `Vcvs` OUT − VSS = TF(θ)/100 · (VDD − VSS), TF = offset + pente·θ écrêtée, θ = 100·position, derrière r_out ; fuite permanente et condensateur OUT–VSS ; R_iso entre VSS1 et VSS2 (sans limitation de courant bidirectionnelle ni temporisation de démarrage). `position` est pilotée par `set_pot` et par un actionneur de glissement ; un changement en RUN met à jour le gain sans redémarrage. État de fonctionnement : `pot` (la course seulement).
- En mode mission l'enregistrement est refusé (`mission_locked`, « Enregistrement indisponible en mission. ») tant que le banc chargé par `PhysLab::load_circuit_json` est le document courant ; `SnapshotDto` et `ValuesDto` portent `doc_id`, qui change à chaque remplacement du document, pour que l'UI recharge un banc chargé hors d'elle. Le journal d'un banc de mission n'est jamais montré (`journal_tail` vide). Pendant un essai, `set_check_lock(true)` rend le banc en lecture seule pour l'UI (refus `bench_locked`, « Banc en lecture seule pendant l'essai. ») et fige les états d'interaction qu'elle reçoit ; le contrôleur de mission pilote la course par `command_internal`, la sonde lit le circuit réel.

Le candidat des quatre missions ajoute `ecu_wsf`, `ecu_sas`, `ecu_bps`, `ats682lsh`, `tle5014`, `pss260`, `pressure_switch`, `front_axle`, `steering_column` et `brake_hydraulics`. Les modèles internes restent cachés ; l'inspecteur expose les commandes mécaniques et les manipulations pertinentes : vitesse et braquage du train avant, entrefer et tolérance du capteur de roue, angle dans les butées et vitesse maximale de la colonne, pression et rapport hydraulique. `set_world` valide la quantité, ses bornes, les butées déclarées et le composant destinataire ; ses requêtes sont regroupées par composant et quantité pendant un glissement.

Les ECU lisent les grandeurs calculées sur les vrais réseaux du solveur : courant et fronts des capteurs de roue, alimentation et signal SENT à travers les filtres RC, ADC des capteurs de pression et état du contact hydraulique. Une mesure indisponible ne devient pas une valeur nominale inventée. Les sorties de capteur avec limite bidirectionnelle utilisent `LimitedVcvs` ; ses modes CV/CC+ et CC− conservent la conservation des courants. Le banc APPS historique garde ses paramètres.

Pendant une session de mission, le bus possède l'horloge physique et avance le banc par pas de 1 ms avant chaque échantillon ECU. Le regroupement de plusieurs millisecondes dans une lecture du bus ne supprime pas les fronts ou trames intermédiaires. Les captures sont immuables et l'historique par ECU est borné à 256 échantillons ; arrêt, reset et remplacement de document invalident l'ancien propriétaire. Le rechargement d'un banc attend la libération de l'ancienne source. La reproductibilité des missions est vérifiée sur les octets et horodatages CAN des vraies factories ; le digest et le replay de journal historiques ne couvrent pas à eux seuls les nouveaux états de protocole ni la configuration source du pack.

### 2.1 Modèles électriques et provenance

Le passage au solveur (« abaissement ») est fait par `n7rt-lab::phys` (§8.1). Notations : nœuds des nets, nœuds internes (`bias`), éléments du §5. Toutes les mesures sont référencées à la masse **locale** du composant (sa broche `ground_pin`), jamais à une masse implicite.

| Composant | Modèle | Valeurs et classe |
|---|---|---|
| Alimentation DC | `on` : `Supply{pos, neg, v = voltage, r_out, i_lim = current_limit}` : un quadrant, modes CC / CV / REV (§5.4). `off` : aucun élément | Consigne et limite : paramètres. `r_out` : N7RT_MODEL. Un quadrant : une alimentation de labo ne peut pas absorber de courant |
| Masse | Toutes les broches `ground` forment le net « GND », nœud de référence 0 V. Sans masse : îlots relatifs (§5.7) | ARCHITECTURE §9.1 |
| Résistance | `Resistor{r}`. Diagnostic `resistor_overload` si U²/R > `power_rating` | — |
| Condensateur | `Capacitor{c}`, déchargé au RESET. `cap_overvoltage` si \|V(p1) − V(p2)\| > `voltage_rating`. Polarisé : `cap_reverse` si V(p1) − V(p2) < −0,5 V | Seuil de −0,5 V : règle pédagogique prudente, sans source constructeur (N7RT_MODEL) |
| Potentiomètre | R_AW = max(α·R, 1 mΩ) entre A et W ; R_WB = max((1 − α)·R, 1 mΩ) entre W et B ; α = k/1000, k entier, calculé `k as f64 / 1000.0`. `resistor_overload` si \|I_AW\| ou \|I_WB\| > √(P_nom/R) | Linéaire. Plancher de 1 mΩ : garde numérique. Règle de surcharge par segment : N7RT_MODEL, non vérifiée sur une fiche |
| Bouton | Contact fermé ⇔ (`momentary` ? `pressed` : `latched`) XOR (`contact` = `nc`). `latched` bascule à chaque front montant de `pressed`. Fermé : `Resistor{r_on}` ; ouvert : `Resistor{r_off, leakage}` | DATASHEET Omron B3F (A070-E1-08, p. 3) : 100 mΩ max, 100 MΩ min |
| Interrupteur | Unipolaire. Fermé : `r_on` ; ouvert : `r_off` (fuite) | DATASHEET NKK M2011 (catalogue p. A52) : 10 mΩ max (argent), 1 000 MΩ min |
| Entrée ADC | r_in = ∞ (défaut) : aucun élément, entrée idéale, flottante si non reliée. r_in finie : `Resistor{in, ref, r_in}`, une vraie conductance (pas une fuite) qui référence l'entrée. Échantillon : V(in) − V(ref) à la fin de la frame dont l'indice est multiple de 1000/`sample_rate`, bloqué jusqu'au suivant. `code = floor(V·2^bits/vref)`. Valid si 0 ≤ V < vref ; sinon Saturated, code borné à [0, 2^bits − 1] | ARCHITECTURE §10.5 (loi de quantification ADC) |
| ECU STM32, `usb` vrai | `Supply` 5,0 V / `r_out_5v`, i_lim 0,5 A entre `v5` et `gnd` | DATASHEET UM2407 Rev 4 p. 20 : interrupteur U2 à limitation, surintensité au-delà de 500 mA. `rail_overload` au-delà de 300 mA (p. 16, p. 20) |
| | `Supply` 3,3 V / `r_out_3v3`, i_lim 1,3 A entre `v3v3` et `gnd` | DATASHEET ST1L05 (DocID14492 Rev 3, table 8) : limite ≥ 1,3 A ; régulateur cité par UM2407 table 14 |
| | A0 (PA3, CN9-1) et A1 (PC0, CN9-3) : entrées idéales (aucun élément) ; ADC de `adc_bits` bits, Vref = V(`v3v3`) − V(`gnd`) (ratiométrique), échantillonnage `adc_sample_rate` | DATASHEET UM2407 table 20 ; DS12117 Rev 9 p. 35 (8 à 16 bits). Vref = VDDA = 3V3 : N7RT_MODEL (UM2407 ne précise pas la source de VREF+, accessible en CN7-6) |
| | PD1 (CAN_TX, CN9-27), `can_mode` = `can` (image `--can-bus` du runtime) : `Vcvs{can_tx, gnd, v3v3, gnd, gain 1, r_out 50 Ω}`, niveau récessif. `hi_z` (image sans FDCAN) : aucun élément | DATASHEET DS12117 table 157 : VOH ≥ VDD − 0,4 V à 8 mA, d'où r_out ≤ 50 Ω |
| | PD0 (CAN_RX, CN9-25) : entrée idéale (aucun élément) ; seuils VIL 0,3·V3V3, VIH 0,7·V3V3 par rapport à `gnd` | DATASHEET DS12117 table 156 (spécification CMOS) |
| | Limites : PD0 et PD1 dans [−0,3 V ; VDD_ecu + 4,0 V], VDD_ecu = V(`v3v3`) si `usb`, 0 sinon (`ft_pin_abs_max`). \|I(PD1)\| > 20 mA : `output_overcurrent`. 5V > 0,3 A : `rail_overload`. A0 et A1 : pas de diagnostic de limite (type de broche non vérifié) | DATASHEET DS12117 table 118 (FT : Min(VDD, VDDA, VDD33USB, VBAT) + 4,0 V) et table 119 (20 mA absorbés). « VDD = 0 ⇒ 4,0 V » est une déduction de la formule |
| ECU, `usb` faux | Ni rails ni driver PD1 ; ADC et logique de PD0 : Unavailable(`ecu_unpowered`) | — |
| Transceiver MCP2551 | Éléments permanents : `Resistor{canh, bias, r_in}` et `Resistor{canl, bias, r_in}` (réseau d'entrée en T, nœud interne `bias`) ; pull-up `Resistor{txd, vdd, 25 kΩ}` ; consommation `Resistor{vdd, vss, 500 Ω}` | D20 : R_IN 5–50 kΩ ; D22 : R_DIFF 20–100 kΩ (d'où r_in ≥ 10 kΩ) ; §1.7.1 : 25 kΩ nominal (D28 : IIL −100…−400 µA à 0 V) ; D2 : IDD récessif ≤ 10 mA (valeur max, pas de typique) : modèle pessimiste N7RT_MODEL, présent même hors tension, ce qui évite les cycles de mode |
| | Porte POR `Gate{vdd, vss, on_above 4,05 V, off_below 3,70 V}`, qui porte : | DS20001667G Rev G : VPORH 3,8–4,3 V, VPORL 3,4–4,0 V, hystérésis 0,3–0,8 V (p. 9, D4–D6). Seuils choisis aux milieux : N7RT_MODEL |
| | `Vcvs{bias, vss, vdd, vss, gain 0,5, r_out 1 mΩ}` | D7 : récessif 2,0–3,0 V. 1 mΩ : constante numérique N7RT_MODEL |
| | `RXD = Vcvs{rxd, vss, vdd, vss, gain 1, r_out 187,5 Ω}` (récessif = niveau haut) | D31 : VOH ≥ 0,7·VDD à 8 mA, d'où r_out ≤ 0,3·5 V/8 mA (borne de la fiche, sans typique inventé) |
| | `VREF = Vcvs{vref, vss, vdd, vss, gain 0,5, r_out 5 kΩ}` | D33 : 0,45–0,55·VDD à ±50 µA, d'où r_out ≤ 5 kΩ |
| | Non alimenté (porte ouverte) : polarisation, RXD et VREF retirés ; le réseau en T, le pull-up et R_DD restent. Un nœud non alimenté ne perturbe pas le bus (50 kΩ face à 60 Ω) | §1.6 de la fiche ; D24 : fuite ≤ 150 µA non modélisée |
| | **Rs** (U = V(vdd) − V(vss)) : net de Rs = net de VSS → haute vitesse. Net portant une broche `source` ou `logic_out` : haute vitesse sous 0,3·U, veille (`ic_standby`) au-dessus de 0,75·U, sinon indéfini (`rs_non_modelise`). Tout autre cas, dont une résistance de pente : indéfini, avec `rs_floating` si le net est flottant, `rs_non_modelise` sinon. Jamais de repli sur haute vitesse | Table 1-1, D34–D36. Limite V1 : le courant interne de Rs (contrôle de pente) n'est pas modélisé |
| | **TXD** (VIL 0,8 V, VIH 2,0 V par rapport à VSS). Alimenté, hors veille, TXD pas haut : `dominant_requested` ; les lectures des nets CANH, CANL et RXD, et la logique qui en dérive (PD0), valent Unavailable(`dominant_non_modelise_v1`). TXD bas depuis plus de 1,25 ms : `dominant_timeout`, drivers coupés, bus de nouveau récessif jusqu'au prochain front montant de TXD. TXD entre les seuils : Unavailable(`txd_undefined`). En veille, RXD suit le bus | D25, D26 ; paramètre AC 14 : minuterie 1,25–4 ms (valeur minimale retenue : N7RT_MODEL) |
| | Limites : U > 7,0 V `ic_abs_max` ; alimenté et U hors [4,5 ; 5,5] V `vdd_out_of_spec` (sans retirer la polarisation) ; \|courant RXD\| > 8 mA `output_overcurrent` (avertissement, hors spécification D31/D32) | p. 8 (maximums absolus) ; tableau DC |
| Bus CAN | `remote_termination` : `Resistor{canh, canl, 120 Ω}`. n = `remote_nodes` ≥ 1 : nœud interne `rbias`, `Resistor{canh, rbias, remote_r_in/n}`, `Resistor{canl, rbias, remote_r_in/n}`, `VSource{rbias, gnd, 2,5 V, r_out 1 Ω}`. n = 0 : aucune polarisation | n transceivers distants alimentés par leur carte à 5,0 V, polarisés à VDD/2 par R_IN : N7RT_MODEL fondé sur DS20001667G. La masse des nœuds distants est la broche `gnd`, explicite (hardware-wiring.md, dépôt encadrant : fil de masse commun) |
| Terminaison | `Resistor{120 Ω}`, 0,25 W | STANDARD ISO 11898-2 (nominal) |
| Jonction | Aucun élément : point du net | — |

**Dominant non modélisé en V1.** Le bus reste récessif. Une demande de dominant est un état affiché, avec des lectures indisponibles, jamais une valeur inventée.

### 2.2 Géométrie des symboles

- **Pattes** : 1 unité, du bord du corps jusqu'au point de connexion, dans la direction `dir` de la broche. Elles tournent avec le composant.
- **Corps** : rectangle `body` du catalogue. Un fil ne peut pas le traverser (`wire_through_body`) ; les pattes en sont exclues.
- **Dessin**, dans le corps, en `--sch-symbol`, 1,5 px :

  | Kind | Dessin (rotation 0, unités de grille) |
  |---|---|
  | `dc_supply` | traits de (0, −3) à (0, −0,5) et de (0, 0,5) à (0, 3) ; plaques horizontales en y = −0,5 (longueur 4, « + ») et y = 0,5 (longueur 2) ; annotation « 5 V · 0,1 A » à droite |
  | `ground` | traits horizontaux en y = 1, 1,5 et 2, de longueurs 3, 2 et 1 |
  | `resistor`, `termination` | rectangle [−2, 2] × [−0,75, 0,75] |
  | `capacitor` | plaques verticales en x = ±0,5 sur une hauteur de 3. Polarisé : la plaque `p1` est un rectangle creux, marqué « + » |
  | `potentiometer` | rectangle [−2, 2] × [−0,75, 0,75], plus la flèche dynamique (0, −3) → (0, −2) → (x_α, −2) → (x_α, −0,75), x_α = −2 + 4α, dans la superposition |
  | `switch` | lame de (−1,5, 0) à (1,5, −1,5) si ouvert, horizontale si fermé (superposition) |
  | `push_button` | lame selon l'état réel du contact, poussoir en T au-dessus, marque de verrouillage si `latching` (superposition) |
  | `adc_input`, `ecu_stm32`, `can_transceiver` | rectangle du corps, titre en haut, noms courts des broches à l'intérieur, près de chaque patte |
  | `can_bus` | lignes CANH et CANL en y = −2 et y = 2, de x = −3 à x = 9 ; résistance verticale en x = 9 si `remote_termination` ; « n nœuds distants » |
  | `junction` | carré plein de côté clamp(0,5·u, 4, 8) px CSS, en `--sch-wire`, si 3 conducteurs s'y rejoignent |
- Test : chaque broche se trouve à l'extrémité d'un trait de son symbole.

- **Textes** (Plex Mono, mis à l'échelle avec le zoom, masqués sous 7 px, toujours à l'endroit) :
  - désignateur au-dessus du corps (à gauche s'il est vertical) ;
  - valeur principale sous le corps (« 10 kΩ », « 5 V / 0,1 A ») ;
  - noms de broches seulement s'ils font au moins 8 px.
- **Clé du cache `Path2D`** : (kind, rotation, variante), la variante valant `polarized` pour le condensateur. Les parties qui dépendent de l'état (lame, curseur, poussoir) sont dans la superposition, jamais dans le cache.
- **Limites de dessin** (`draw-bounds`) = corps ∪ pattes ∪ boîtes de texte. Elles servent à l'élimination hors champ et aux rectangles sales.
- **Actionneurs** : zones du catalogue (`actuators`). Dans une zone, le curseur est `pointer` ; un appui actionne, dans tous les outils et tous les états de marche, et ne sélectionne jamais.

## 3. Modèle (crate `n7rt-circuit`)

### 3.1 Document
- `Circuit` : nom, `next_id`, composants et fils (par id croissant), états d'interaction par composant.
  - Les états des composants supprimés ne sont conservés en mémoire que tant qu'ils restent restaurables par l'historique annuler/rétablir, borné à 200 entrées : un composant recréé par annuler retrouve son dernier état. Lorsqu'un identifiant n'est plus présent dans le circuit ni référencé par cet historique, son état est retiré.
  - Seuls les états des composants présents sont sauvegardés.
- `Component { id, kind, label, pos, rot, params }`. `params` contient **tous** les paramètres du kind, défauts compris, tous dans leur plage.
- `Wire { id, a, b, bends }`.
  - Tracé = [position de `a`, `bends`…, position de `b`] : canonique, orthogonal, chaque segment de longueur > 0.
  - Fils toujours pin→pin, une broche de jonction comprise.
- **Nets** : union-find sur les fils, plus toutes les broches de rôle `ground` réunies.
  - Chaque broche appartient à exactement un net ; une broche non reliée forme un net à elle seule.
  - Les broches `ground_pin` (masse locale de l'ECU, VSS, référence d'ADC, masse du bus) ne sont **jamais** reliées implicitement.
- `Document` = circuit + révision `rev` + historique.
  - `rev` croît à chaque édition, annuler, rétablir et chargement validés.
  - Les interactions ne changent **pas** `rev` : ce sont des manipulations, et une édition en file ne doit pas devenir périmée pendant qu'on tourne un curseur. Leurs effets sont suivis par l'indice de publication des valeurs (`seq`).
  - Un correctif vide (déplacement nul, même valeur) n'est pas validé : pas de nouvelle `rev`, pas d'entrée d'historique, pas de journal.

### 3.2 Éditions, interactions, correctifs
- **Éditions** (`Edit`, annulables ; un geste = une édition, envoyée au relâchement) :

  | Édition | Effet |
  |---|---|
  | `add_component{kind, pos, rot, params?, label?}` | Composant aux défauts, surchargés par `params`. Refus `unknown_kind` pour la jonction |
  | `delete{components, wires}` | Supprime les composants avec leurs fils, puis les fils sélectionnés. Les jonctions sélectionnées sont ignorées, le nettoyage s'en charge (§3.4.7) |
  | `move{components, wires, delta}` | Ensemble déplacé et réparation des extrémités (§3.4.3) |
  | `rotate{components, wires}` | +90° : un composant seul autour de son origine, une sélection autour de son centre arrondi (§3.4.5) |
  | `set_param{comp, name, value}` | Valeur typée, ou texte analysé par le cœur selon la grammaire du §4, puis validée |
  | `set_label{comp, label}` | Désignateur validé (§1.5) |
  | `add_wire{a, b, bends?}` | Extrémités `Endpoint` (§3.4.6). Sans `bends` : premier candidat valide de l'auto-routage (§3.4.4) |
  | `move_wire_end{wire, end, to, bends?}` | Rattache une extrémité. La jonction abandonnée passe au nettoyage |
  | `set_bends{wire, bends}` | Retrace un fil (glissement d'un segment), points canonisés |

- **Interactions** (`Interact`) : journalisées, **jamais** dans annuler/rétablir.
  - `set_pot{comp, position}` : α ∈ [0, 1], quantifié à 1e−3.
  - `set_button{comp, pressed}`.
  - `set_switch{comp, closed}`.
  - `set_power{comp, on}` : sortie d'alimentation, ou carte ECU alimentée.
- **Correctif** (`Patch`) : différence exacte des tables composants et fils. Chaque id dont la valeur change y figure en `(id, avant, après)`, cascades comprises : jonctions, coupures et fusions de fils, réparations. Il ne contient ni états d'interaction ni `next_id`.
- **Annuler / rétablir** :
  - appliquer l'inverse, ou le correctif ;
  - les états d'interaction restent inchangés ;
  - 200 entrées au plus, les plus anciennes abandonnées ;
  - une édition réussie vide la pile de rétablissement, une interaction non ; charger, nouveau et démonstration vident les deux piles ;
  - autorisés en RUN (effet électrique à la frame suivante) et journalisés ;
  - propriété : après tout annuler, le texte canonique égale l'original, sauf `next_id`.
- **Commutation électrique.** Seules les éditions qui changent le réseau électrique (connectivité, paramètre) reconstruisent le réseau du solveur. Une édition purement géométrique (déplacer, tourner, retracer, renommer) ne change aucune valeur calculée (`Applied.electrical_changed` = faux).

### 3.3 Protocole d'édition (UI ↔ cœur)
1. `phys_preview{base_rev, edit}` est une exécution à blanc sur le document validé, avec **la même fonction** que la validation. Elle n'est jamais journalisée et ne consomme pas d'id. Elle renvoie le correctif ou un refus.
   - L'UI garde un seul aperçu en vol par geste, via le canal « dernier seulement » de `api.ts`.
2. `phys_command{edit | undo | redo | interact | run | stop | reset}` passe par la file FIFO, un seul appel en vol.
   - Un `base_rev` ≠ `rev` donne le refus `stale_revision`. L'UI recharge alors `phys_snapshot`.
   - Le modèle est validé **avant** la réponse : `EditReply{rev, patch, nets?, can_undo, can_redo, dirty}`.
3. L'UI tient un **miroir en lecture seule** ([state.ts](../app/ui/src/lab/state.ts)), mis à jour seulement par ces réponses et par `phys_snapshot`. Elle ne calcule ni connectivité ni validité géométrique.
4. Un glissement :
   - montre la silhouette et les fils de l'aperçu à chaque image ;
   - envoie l'édition au relâchement.
   - Un relâchement sur un aperçu refusé n'envoie rien : la scène est restaurée et le motif s'affiche dans la ligne de message et dans l'onglet Journal (ni toast ni modale).
   - Échap pendant un geste annule le geste seulement.
5. Après chaque réponse de commande, l'UI demande aussitôt `phys_values`. Édition → valeurs ≈ 1 frame + 2 appels IPC + 1 image.

### 3.4 Géométrie (normative)

#### 3.4.1 Chemin canonique
Retirer, jusqu'à stabilité :
- les segments de longueur nulle ;
- tout point intérieur aligné avec ses deux voisins (ce qui supprime aussi les allers-retours).

Seuls les points canoniques sont stockés ; une entrée de l'UI est canonisée avant validation.

#### 3.4.2 Règles d'ambiguïté
« Ce qui semble relié l'est ; rien ne semble relié sans l'être. » Elles sont vérifiées sur la géométrie finale de chaque édition, et au chargement.

| Sous-code | Condition |
|---|---|
| `pin_on_pin` | deux broches de composants différents au même point |
| `pin_on_wire` (édition d'un composant) / `wire_over_pin` (édition d'un fil) | une broche (jonction comprise) sur un fil (intérieur ou coude) qui ne se termine pas sur elle. Un fil ne touche une broche **qu'à ses deux extrémités**, quels que soient les nets : une suppression ne peut donc jamais créer d'ambiguïté |
| `collinear_overlap` | deux segments colinéaires dont l'intersection a une longueur > 0 : fil/fil, fil/patte. Partager une extrémité n'est pas un chevauchement |
| `self_overlap` | un fil qui se chevauche ou se touche lui-même |
| `bend_on_wire` | un coude sur un autre fil, ou sur un coude d'un autre fil |
| `wire_through_body` | l'intérieur d'un segment coupe le rectangle fermé d'un corps ; pattes exclues, jonctions exclues |

- Les croisements perpendiculaires d'intérieurs sont **autorisés** et ne relient rien : pas de carré.
- Un refus renvoie le point et les items, que l'aperçu dessine en `--sch-invalid`.

#### 3.4.3 Déplacement
- **Ensemble déplacé M** :
  - les composants sélectionnés ;
  - puis, jusqu'à stabilité, toute jonction dont les extrémités lointaines des fils sont toutes dans M.
- **Fils** :
  - deux extrémités dans M : translation rigide (coudes + delta) ;
  - fil sélectionné à une seule extrémité dans M, ou à aucune : coudes + delta, puis réparation des extrémités fixes ;
  - fil non sélectionné à une extrémité dans M : réparation de cette extrémité.
- **Réparation** d'un chemin canonique Q0…Qm dont Q0 devient Q0′ (règle symétrique en Qm) :
  - m ≥ 2 : si le premier segment était horizontal, Q1.y ← Q0′.y, sinon Q1.x ← Q0′.x ;
  - m = 2 et les deux extrémités déplacées : Q1 = (Qm′.x, Q0′.y) si le premier segment était horizontal, sinon (Q0′.x, Qm′.y) ;
  - m = 1 (fil droit) et extrémités désalignées : Z qui garde l'orientation d'origine. Pour un fil horizontal, x_mid = Q0′.x + ⌊(Q1′.x − Q0′.x)/2⌋, coudes (x_mid, Q0′.y), (x_mid, Q1′.y) ; symétrique pour un fil vertical ;
  - puis canonisation.
- Si le chemin réparé est invalide : premier candidat valide de l'auto-routage. Sinon refus, avec le motif du premier candidat.

#### 3.4.4 Auto-routage
Le point de départ est la broche A, de direction sortante dA ; l'arrivée est B, de direction dB. Les candidats, dans cet ordre fixe, sont :
1. droit (si aligné) ;
2. L par (B.x, A.y) ;
3. L par (A.x, B.y) ;
4. Z horizontal (x_mid comme en §3.4.3) ;
5. Z vertical ;
6. U avec des amorces de 2 unités le long de dA et de dB.

Le premier candidat qui passe toute la validation est retenu. Aucun candidat valide : refus `no_route`.

#### 3.4.5 Rotation
- Un composant seul tourne autour de son origine : pos inchangée, rot += 90°.
- Une sélection tourne autour de c = (⌊cx + 0,5⌋, ⌊cy + 0,5⌋), centre du rectangle englobant les corps et les broches de la sélection. Pour chaque composant : pos → c + R90(pos − c), rot += 90°.
- Les fils rigides tournent leurs coudes autour de c ; les fils attachés sont réparés (§3.4.3, un delta par extrémité).

#### 3.4.6 Extrémités et jonctions
- `Endpoint::Pin` : une broche existante. Même broche aux deux bouts : `same_pin` ; paire déjà câblée : `duplicate_wire`.
- `Endpoint::OnWire{wire, at}` :
  - `at` doit être un point de grille du tracé du fil, intérieur ou coude ; sinon `endpoint_not_on_wire` ;
  - si `at` est une extrémité du fil, il vaut la broche de cette extrémité ;
  - sinon, une jonction J est créée en `at`, et le fil est coupé : la moitié qui contient l'extrémité `a` d'origine garde son id, l'autre reçoit un nouvel id ;
  - ordre fixe des ids : pour `a`, puis pour `b` : l'id de J, puis celui de la seconde moitié ; l'id du nouveau fil vient en dernier ;
  - deux extrémités sur le même fil : `same_wire`.
- **Jonction à 4 branches** : interdite. Une jonction porte au plus 3 fils ; un quatrième, ou une jonction posée au croisement de deux fils, est refusé (`four_way_junction`, « Jonction à 4 branches interdite : décalez en deux T »).

#### 3.4.7 Nettoyage des jonctions
Après toute suppression ou tout rattachement, répéter jusqu'à stabilité. J est la jonction de plus petit id, de degré ≤ 2, non sautée dans la passe :
- degré 0 : supprimer J ;
- degré 1 : supprimer J et son fil ;
- degré 2 :
  - W1 est le fil de plus petit id, W2 l'autre ; P et Q sont leurs extrémités lointaines ;
  - si P = Q, ou si un autre fil relie déjà {P, Q} : sauter J pour la passe (jonction de degré 2 légale, dessinée comme un simple coude, sans carré) ;
  - sinon : W1 ← {id W1, a P, b Q, coudes canonical(P→J par W1 ++ J→Q par W2)}, puis supprimer W2 et J.

#### 3.4.8 Rendu des connexions
- Carré plein là où au moins 3 conducteurs se rejoignent : jonction de degré ≥ 3, ou broche de composant avec ≥ 2 fils.
- Carré creux de 0,4 unité sur une broche non reliée.
- Jamais de carré à un croisement simple.

### 3.5 Invariants (`Circuit::check`, chargement, tests de propriétés)
- Ids uniques entre composants et fils, tous < `next_id`.
- Composants :
  - kind connu ;
  - paramètres exactement ceux du kind, typés et dans leur plage (y compris `sample_rate` diviseur de 1000) ;
  - désignateur valide et unique ;
  - coordonnées dans les bornes.
- États : exactement ceux du kind, dans leur plage.
- Fils :
  - extrémités existantes et distinctes ;
  - tracé canonique et orthogonal dans les bornes ;
  - pas deux fils sur la même paire non ordonnée de broches.
- Jonctions : degré 2 ou 3. Le degré 2 n'est permis que si la fusion recréerait un doublon.
- Aucune ambiguïté (§3.4.2) sur tout le circuit.

### 3.6 Refus (édition refusée : rien ne change)
- Codes `RefusalCode` de [lib.rs](../app/crates/n7rt-circuit/src/lib.rs), dont `param_parse` et `param_unit` (saisie texte, §4), `four_way_junction`, `stale_revision`.
- Message en français, affiché tel quel, avec les valeurs. Exemples :
  - « 0 Ω hors plage [10 mΩ ; 100 MΩ] » ;
  - « La fréquence d'échantillonnage doit diviser 1000 Hz (1, 2, 4, 5, 8, 10, 20, 25, 40, 50, 100, 125, 200, 250, 500, 1000) » ;
  - « Ce fil passerait sur la broche R2.1 sans s'y relier ».
- **Aucune valeur n'est corrigée en silence.**

### 3.7 Diagnostics (états calculés, jamais des refus)
Produits par `n7rt-lab::phys` à chaque frame et à chaque aperçu, à partir du netlist et des lectures **Valid** seulement. Une décision fondée sur une lecture invalide est impossible : le diagnostic décrit alors l'état invalide.

**Topologiques** (calculés sur le netlist) :

| Code | Gravité | Règle |
|---|---|---|
| `no_ground` | avertissement | une source et aucun symbole de masse : « Aucune masse : tensions relatives seulement » |
| `unconnected_pin` | avertissement | broche `required` seule sur son net, alors qu'une autre broche du même composant est reliée. Liste : `adc_input.in` et `.ref` ; `can_transceiver.vdd` et `.vss` ; `ecu_stm32.gnd` ; `can_bus.gnd` si `remote_nodes` ≥ 1 |
| `open_pin` | info | broche passive seule sur son net : circuit ouvert, aucun courant ne la traverse |
| `output_short` | erreur | un net qui porte une `logic_out` (PD1 en `can`, RXD) porte aussi une autre `logic_out`, ou une broche `source`, `ground` ou `ground_pin` : court-circuit logique |
| `source_short` | erreur | `pos` et `neg` d'une alimentation sur le même net |

**Électriques** (calculés à chaque frame et à chaque aperçu, sur des lectures Valid seulement) :

| Code | Gravité | Règle |
|---|---|---|
| `floating_net` | avertissement | net de classe Floating avec ≥ 2 broches, ou avec une entrée (`analog_in`, `logic_in`, `power_in`) |
| `island_no_reference` | avertissement | groupe K relatif (sans masse) contenant une source ; message avec la liste de ses nets |
| `adc_floating` | avertissement | entrée d'ADC (composant, ou A0/A1 de l'ECU alimenté) non Valid |
| `adc_saturated` | avertissement | échantillon hors [0 ; Vref) |
| `logic_floating`, `logic_undefined` | avertissement | entrée logique non Valid ; entre ses seuils (PD0 si `can`, TXD alimenté) |
| `supply_current_limit` | avertissement | alimentation en CC |
| `supply_backfeed` | avertissement | alimentation en REV : poussée au-dessus de sa consigne |
| `rail_overload` | avertissement | courant du rail 5V de l'ECU > 0,3 A (UM2407) |
| `resistor_overload` | erreur | résistance ou terminaison : U²/R > nominale ; potentiomètre : courant d'un segment > √(P/R) |
| `cap_overvoltage`, `cap_reverse` | erreur | \|V\| > `voltage_rating` ; polarisé et V(p1) − V(p2) < −0,5 V |
| `output_overcurrent` | erreur | \|I(PD1)\| > 20 mA (DS12117 table 119) ; \|I(RXD)\| > 8 mA (hors spécification D31/D32) |
| `ft_pin_abs_max`, `ic_abs_max` | erreur | §2.1 (PD0/PD1 de l'ECU ; VDD du MCP2551) |
| `ic_unpowered` | avertissement | transceiver sous son POR ; ECU avec `usb` faux |
| `vdd_out_of_spec` | avertissement | transceiver alimenté, VDD hors [4,5 ; 5,5] V |
| `rs_floating`, `rs_non_modelise` | avertissement | §2.1 |
| `ic_standby` | info | §2.1 |
| `dominant_requested`, `dominant_timeout` | avertissement | §2.1 |
| `solver_singular`, `solver_mode_cycle` | erreur | îlot sans résultat publié (§5) |
| `engine_restarted`, `internal_error` | erreur | §8.1 |
| `journal_incomplete` | avertissement | §8.1 : entrées non archivées ou archive endommagée ; compteur conservé jusqu'au nouveau document et note visible dans l'onglet Journal |

- Forme : `Diagnostic{code, severity, message_fr, items}` ; tri par gravité, puis code, puis premier item, pour un ordre stable.
- Le mauvais conditionnement d'une matrice reste une statistique interne, pas un diagnostic.
- **Chaque code a une paire de tests** : un circuit minimal qui le lève, un circuit voisin qui ne le lève pas.

### 3.8 Anti-divulgation (architecture §9.3)
- Le **cœur** filtre ; l'UI ne fait que montrer ce qu'elle reçoit.
- **Mode exploration**, le seul de la V1 : tout est publié, comme la vérification électrique d'un logiciel de CAO.
- **Mode mission** :
  - `ValuesDto.nets`, `pins`, `measures` et `diagnostics` vides ;
  - `op` réduit à ce que la pièce réelle montre : contact, curseur, mode CC de l'alimentation, carte alimentée. Le transceiver devient `passive`.
  - Ne restent que les états d'interaction et, plus tard, les lectures des instruments placés.
- Un test Rust force le mode mission et vérifie le filtre.

## 4. Unités et paramètres
- **Saisie** : l'UI envoie le **texte** tapé dans `set_param` ; le cœur l'analyse (une seule implémentation, en Rust).
  - Forme : `[signe] nombre [préfixe] [unité]`, espaces permis, signe − (U+2212) accepté.
  - Séparateur décimal point ou virgule ; exposant `e±n`.
  - Préfixes p, n, u, µ (U+00B5) ou μ (U+03BC), m, k, M, G (m = milli, M = méga).
  - Notation RKM : 4k7 = 4,7 k ; 2R2 = 2,2 Ω ; 1M5.
  - Unité facultative ; si elle est présente, elle doit être celle du paramètre (Ω, ohm, F, V, A, W, Hz, s), sinon `param_unit` (« unité attendue : Ω »). « inf » ou « ∞ » seulement pour un paramètre `number_or_inf`. Erreur de syntaxe : `param_parse`.
  - **Conversion exacte** : construire la chaîne décimale complète (« 2.2e-9 »), puis la lire une seule fois (`f64::from_str`). Ne jamais multiplier par le préfixe : 2.2 × 1e−9 donne 2,2000000000000003e−9.
  - Le cœur valide ensuite type et plage ; l'UI n'arrondit ni ne borne rien.
- **Affichage** (UI, `units.ts`) :
  - préfixe d'ingénieur, point décimal (comme le cockpit) ;
  - séparateur valeur–unité U+00A0 (U+2009 et U+202F n'existent pas dans Plex Mono) ; micro écrit U+00B5, jamais U+03BC ;
  - 6 chiffres significatifs au plus, zéros de fin retirés pour les paramètres ; représentation exacte dans un champ en cours d'édition. Exemples : « 10 kΩ », « 4.7 µF », « 1.49999 V », « 499.999 µA ».
  - Les lectures non Valid s'affichent en mots, jamais en nombre :
    - « flottant » ;
    - « sans référence » ;
    - « indéfini » ;
    - « circuit ouvert » ;
    - « saturé (4095) » ;
    - « — », avec le motif en infobulle (indisponible).
- **Couverture des glyphes** : tout caractère que le formateur peut produire (Ω µ − ∞ ± ≤ ≥) existe dans les polices embarquées : Plex, plus le complément N7RT Symbols ([DESIGN.md](../app/ui/DESIGN.md) §3). Un test le vérifie.
- **Tests** : chaque valeur E12 × chaque préfixe donne le même `f64` que le littéral, au bit près ; chaque message d'erreur est exact.
- **Règles particulières** :
  - `sample_rate` et `adc_sample_rate` divisent 1000 ;
  - `bits` entier de 8 à 16 ;
  - `adc_bits` ∈ {8, 10, 12, 14, 16} ;
  - les paramètres `editable = false` sont refusés (`param_locked`).
  - Tout paramètre modifiable l'est aussi en RUN.

## 5. Solveur (crate `n7rt-solver`)

Numérique pur, sans dépendance au modèle ; contrat compilé [lib.rs](../app/crates/n7rt-solver/src/lib.rs). Ce n'est pas un SPICE : réseau linéaire par morceaux, méthode fixée ci-dessous, résultats déterministes.

### 5.1 Réseau
- `Network { nodes, ground, elements, gates }` :
  - `Resistor{a, b, r, leakage, gate}` ;
  - `Capacitor{a, b, c, key}` ;
  - `VSource{pos, neg, v, r_out, gate}` : Thévenin linéaire, deux quadrants ;
  - `Supply{pos, neg, v, r_out, i_lim, key, gate}` : alimentation un quadrant ;
  - `Vcvs{pos, neg, cp, cn, gain, r_out, gate}` ;
  - `Gate{cp, cn, on_above, off_below, key}` : comparateur à hystérésis.
- **Fuite** : la résistance `r_off` d'un contact ouvert, seule résistance de modèle non intentionnelle. Elle conduit dans la solution mais ne compte jamais pour la classification (§5.7) : un nœud relié seulement par un contact ouvert reste **flottant**. Les entrées idéales (ADC à r_in = ∞, A0, A1, PD0) n'ont aucun élément : non reliées, elles sont flottantes, jamais « 0 V ». Une r_in finie est une vraie conductance.
- **Validation** (`NetworkError`) :
  - valeurs finies ; r, c, r_out > 0 ; i_lim > 0 ; v ≥ 0 pour Supply ;
  - on_above > off_below ;
  - clés uniques.
  - Une erreur est un bogue de l'abaissement : diagnostic `internal_error`, réseau précédent conservé.

### 5.2 Îlots
- Graphe :
  - sommets : tous les nœuds **sauf la masse** ;
  - arêtes : une par branche d'élément, fuites et éléments à porte compris quel que soit leur état ;
  - pour chaque Vcvs, les arêtes (pos, cp) et (pos, cn) ;
  - pour chaque porte, des arêtes entre ses nœuds de commande et les nœuds des éléments qu'elle commande.
- Chaque composante connexe est un îlot. Deux îlots ne partagent que la masse, à 0 V : ils sont donc exactement découplés et chacun a son pas et ses factorisations.
- Signature d'îlot : éléments dans l'ordre du réseau, nœuds renumérotés par rang dans l'îlot, valeurs au bit près, clés, portes. Un îlot de signature inchangée garde son état et n'est **pas** commuté.

### 5.3 Méthode
- **Mise en équations.** Y (conductances) et rhs, nœuds de l'îlot dans l'ordre croissant, masse exclue (V = 0) :
  - **R** : g = 1/r ; Y[a,a] += g, Y[b,b] += g, Y[a,b] −= g, Y[b,a] −= g.
  - **C** :
    - Notations : v_n = V_a − V_b ; i_n = courant entrant par a.
    - Euler implicite (BE) : G = C/h, J = G·v_n. Trapèzes : G = 2C/h, J = G·v_n + i_n.
    - Inscription de G comme pour R ; rhs[a] += J, rhs[b] −= J.
    - Après résolution : i_{n+1} = G·v_{n+1} − J.
    - Passage BE → trapèzes : i_n du dernier pas BE. Passage trapèzes → BE, et reconstruction : i_n ignoré puis recalculé.
  - **VSource, Supply en CV** : g = 1/r_out ; inscription de g ; rhs[pos] += V·g, rhs[neg] −= V·g. Courant sortant par pos = (V − (V_pos − V_neg))·g.
  - **Supply en CC** : rhs[pos] += i_lim, rhs[neg] −= i_lim. **REV** : rien.
  - **Vcvs** : g = 1/r_out, gm = gain·g ; inscription de g, plus Y[pos,cp] −= gm, Y[pos,cn] += gm, Y[neg,cp] += gm, Y[neg,cn] −= gm. À vide : V_pos − V_neg = gain·(V_cp − V_cn). La matrice n'est pas symétrique : LU, pas Cholesky.
  - **Élément dont la porte est ouverte** : absent.
- **Régularité sans gmin.** À chaque résolution, dans l'îlot, on prend les composantes du graphe des conductances (R fuites comprises, compagnons de C, CV, VSource, sorties des Vcvs actives).
  - Une composante qui ne touche pas la masse a son plus petit nœud fixé à 0 V : sa ligne est remplacée par v = 0. C'est exact, car ses lois de Kirchhoff sont liées.
  - Un nœud sans conductance est fixé à 0 V.
  - Ces potentiels sont des artefacts, classés Relative ou Floating, **jamais publiés** comme mesure.
- **LU dense** écrite dans le crate (pas de BLAS, de LAPACK ni de crate à dispatch SIMD) :
  - pivot = premier indice du |a| maximal strict ;
  - îlot **singulier** si |u_kk| ≤ 1e−13·max_j |a_kj| de la ligne d'origine ;
  - solution non finie : **NonFinite**.
  - Dans les deux cas, rien n'est publié pour l'îlot (lectures Unavailable `solver_singular`, diagnostic `solver_singular`) et l'état précédent est conservé.
  - max|u_kk|/min|u_kk| > 1e14 : îlot marqué mal conditionné (statistique interne), résultats publiés.
- Résolution absolue du courant d'un condensateur ≈ ε·(2C/h)·|V|, soit ≈ 2e−10 A pour 1 F à h = 10 µs. Elle est documentée ; aucune valeur plus petite n'est affichée comme significative.

### 5.4 Modes (résolus **dans** chaque sous-pas, aperçu et îlot statique compris)
- **Supply**, un quadrant, avec x = V(pos) − V(neg). Modes ordonnés CC < CV < REV ; δ = 1e−9 V :
  - CV → CC si x < V − r_out·i_lim − δ ;
  - CC → CV si x > V − r_out·i_lim + δ ;
  - CV → REV si x > V + δ ;
  - REV → CV si x < V − δ ;
  - entre les seuils, le mode est conservé.
  - Sans i_lim : CV et REV seulement.
- **Porte** : u = V(cp) − V(cn).
  - Off → On si u > on_above ; On → Off si u < off_below.
  - Décidée seulement si cp et cn sont dans le même groupe K (§5.7). Sinon, son état cohérent est Off : aucune décision ne repose sur un potentiel artificiel.
- **Boucle** :
  1. partir des modes mémorisés ; une Supply en CC dont pos et neg ne sont reliés par aucun autre chemin de conductance passe en CV ;
  2. résoudre ;
  3. vérifier chaque élément à modes par clé croissante ;
  4. déplacer le **premier** incohérent d'**une** position, passer le sous-pas en BE, revenir en 2.
  - Au plus 4·m + 4 résolutions (m = éléments à modes de l'îlot), sinon `ModeCycle` : rien n'est publié (lectures Unavailable `mode_unresolved`), l'état n'avance pas, diagnostic `solver_mode_cycle`.
  - Test : un interrupteur fermé sur une alimentation 5 V / 1 A / 10 mΩ, dans un îlot statique, publie 1 A, jamais 83 A.
- Toute bascule est une **commutation**.
- Une Supply dont `pos` et `neg` sont sur le même nœud ne demande pas de cas particulier : elle se retrouve en CC à i_lim (diagnostic `source_short`).
- Les factorisations peuvent être mises en cache par (signature, méthode, h, vecteur de modes).

### 5.5 Transitoire et pas
- **Forêt des condensateurs** : on parcourt les C par clé croissante avec un union-find sur leurs bornes. Une C dont les bornes sont déjà reliées par des C est une **C de maille**.
- **Constantes de temps.** Pour chaque C d'arbre k, R_k est la résistance vue entre ses bornes, dans ces conditions :
  - autres C d'arbre court-circuitées, C de maille ouvertes ;
  - CV et VSource à 1/r_out ; CC, REV et portes ouvertes : ouverts ;
  - Vcvs actives, fuites incluses.
  - τ_k = C_k·R_k.
- **Minorant** : τ_lb = 1/Σ 1/τ_k sur les C d'arbre de τ_k fini ≥ 10 µs. C'est un minorant de la plus petite constante de temps pour un réseau RC passif (identité de la trace) ; c'est une heuristique documentée en présence de Vcvs.
- **Pas** : h_cible = clamp(τ_lb/20, 10 µs, 1 ms), puis h = 1 ms/N, N étant la première valeur de `SUBSTEP_COUNTS` (diviseurs de 10⁶ jusqu'à 100) telle que 1 ms/N ≤ h_cible.
  - Chaque instant de sous-pas est donc un entier exact de ns.
  - Le pas est choisi à chaque frontière de frame, d'après les modes à cet instant.
  - Un îlot sans condensateur est statique : N = 1, même boucle de modes.
- **Méthode par condensateur** :
  - BE pour une C de maille, pour une C de τ_k < h (quasi statique), et pour toutes les C d'un îlot pendant les 2 sous-pas qui suivent une commutation (chargement d'un îlot modifié, RESET, changement de mode) ;
  - trapèzes sinon.
- **C modifiée en RUN** : sa tension est conservée, pas sa charge. Une C supprimée puis rétablie est déchargée.
- **État du solveur** :
  - t ;
  - par C : (v, i) ;
  - par élément à modes : mode ; par porte : état ;
  - par îlot : sous-pas BE restants.
  - `state_digest()` le hache (FNV-1a sur `to_bits`).
- **Limite documentée** :
  - les dynamiques plus rapides que 10 µs sont traitées en quasi statique : régime établi exact, erreur ≤ τ_k/h sur le premier sous-pas après une commutation ([écart] §1.8) ;
  - le transitoire fin relèvera de la MNA à la demande (architecture §10.3).

### 5.6 STOP : aperçu
- Après une édition ou une interaction en STOP : un pas BE de h = 1 ns depuis l'état mémorisé, avec la boucle de modes.
  - Les condensateurs restent tenus à h/τ_k près ; deux C en parallèle à des tensions différentes montrent leur redistribution de charge.
  - Les résultats sont publiés avec `preview` = vrai.
  - L'état mémorisé (t, v et i des C, modes, portes) n'est **pas** modifié.
- Au retour en RUN, la première frame part de l'état figé, avec les 2 sous-pas BE de commutation si quelque chose a changé.

### 5.7 Classification et lectures
- **Graphe K**, sur la solution acceptée :
  - sommets : tous les nœuds, masse **comprise** ;
  - arêtes : R non fuite, C, VSource, Supply en CV, sorties de Vcvs (éléments à porte seulement s'ils sont actifs) ;
  - exclus : fuites, Supply en CC ou REV, entrées de commande des Vcvs.
- **Classe d'un nœud** : Referenced si son groupe K contient la masse ; Relative si son groupe compte au moins 2 nœuds ; Floating sinon ; Unused s'il n'appartient à aucun élément.
- **Lectures** (§6), toutes de type `Reading` :
  - `voltage_at(pin)` : Valid (Referenced), NoReference (Relative), Floating, ou Unavailable si l'îlot est en erreur ;
  - `voltage_between_pins(a, b)` : Valid si et seulement si a et b sont dans le même groupe K. Sinon Floating si l'une est flottante, NoReference sinon ;
  - courants et puissances : Valid dès que l'îlot est résolu, car ils ne dépendent pas de la référence ;
  - ADC et niveaux logiques : calculés par `voltage_between_pins(broche, masse locale du composant)`. Une lecture non Valid donne Floating ou NoReference, avec `adc_floating` ou `logic_floating`.

### 5.8 Déterminisme
- Même réseau, même état, mêmes appels ⇒ résultats et `state_digest` identiques au bit près.
- Ordres fixes :
  - nœuds d'un îlot croissants ;
  - inscriptions dans l'ordre du réseau puis des branches ;
  - boucles scalaires.
- Interdits : itération de HashMap, threads, BLAS/LAPACK/SIMD dynamique, `mul_add`, `powi`, fonctions transcendantes.
  - 2^bits s'écrit `(1u32 << bits) as f64`.
- La garantie porte sur « même circuit et même journal ». Deux circuits équivalents mais de NetKey différents peuvent différer à l'ulp.
- Le hachage identique sur les trois OS (architecture §10.4) est garanti par ces règles. Il n'est **mesuré** que sous Windows dans cet incrément.

### 5.9 Résistance hors tension (tests, mode exploration, multimètre banc coupé)
- `unpowered_resistance(réseau, a, b)` : le circuit éteint, comme un ohmmètre sur un banc hors tension (contrôles M1 et M2 de hardware-wiring.md, document du dépôt encadrant) :
  - seules les résistances sans porte sont gardées, fuites, R_DD, pull-ups et réseaux d'entrée compris ;
  - sources, Vcvs, éléments à porte et condensateurs sont retirés ;
  - calcul sur tout le réseau, masse comprise.
- On injecte +1 A en a et −1 A en b, puis R = V(a) − V(b).
- Résultats particuliers :
  - même nœud : 0 ;
  - aucun chemin : +∞ côté solveur, lecture `Open` côté sonde (aucune valeur non finie ne sort du cœur) ;
  - système singulier : `None`, lecture Unavailable.
- Amendement 0.3 (multimètre du banc) : la fonction Ω du multimètre utilise ce calcul, et seulement quand toutes les alimentations du banc sont coupées (sortie `on` de chaque alimentation continue à faux, carte sans USB). Sur un banc alimenté, l'afficheur montre « — », car un ohmmètre réel n'y mesure rien de fiable. La mesure porte sur le réseau simulé tel quel. Une ligne SENT active se lit à sa moyenne sur une trame : 232/282 du niveau haut (fiche TLE5014 rév. 1.0 p. 33 : 282 UT avec l'impulsion de pause, 10 impulsions basses de 5 UT), soit environ 4,1 V sous 5 V ; le générateur de trames, lui, résout les états bas et haut. La fonction V continu affiche la moyenne de V(VΩ) − V(COM) sur les 100 dernières trames. Les deux lectures restent visibles en mode mission.
- Hors du multimètre, jamais exposée en mode mission : la sonde de la §5.10 rend Unavailable(`hidden_mission`).

## 6. API de sonde (instruments, moteur CAN, missions)

Trait `CircuitProbe` ([phys/mod.rs](../app/crates/n7rt-lab/src/phys/mod.rs)), implémenté par `PhysFrame`. Une `PhysFrame` est l'instantané **immuable** des résultats d'une frame ou d'un aperçu, partagé en `Arc` : aucun lecteur ne verrouille le solveur.

| Méthode | Sémantique |
|---|---|
| `sim_time_ns`, `rev` | instant et révision du circuit calculé |
| `voltage_at(pin)` | V(broche) par rapport au net GND |
| `voltage_between_pins(a, b)` | V(a) − V(b), Valid ssi même groupe K |
| `pin_current(pin)` | courant **entrant** dans le composant par cette broche ; Σ des broches d'un composant = 0 |
| `power(comp)` | puissance absorbée Σ V·I entrant, négative pour une source qui fournit ; Unavailable(`ic_model`) pour l'ECU, le transceiver et le bus, dont les sources commandées ne puisent pas dans leur alimentation |
| `adc(comp, canal)` | dernier échantillon (`in` pour l'entrée ADC, `a0`/`a1` pour l'ECU) : `{volts, code, bits, vref, sampled_at_ns}` ; Saturated hors [0 ; Vref) ; Unavailable(`not_sampled_yet`) avant le premier échantillon |
| `logic(pin)` | Low / High selon les seuils du composant et sa masse locale ; Undefined entre les seuils |
| `op_state(comp)` | état de fonctionnement : alimentation (off/cv/cc/rev), contact, curseur, ECU alimenté, transceiver (alimenté, mode Rs, TXD) |
| `net_of(pin)` | affichage seulement |
| `diagnostics()` | §3.7 |
| `unpowered_resistance(a, b)` | §5.9 : Valid ou Open ; tests et mode exploration seulement |

- Les broches sont l'adresse durable. Une broche inconnue (composant supprimé) donne Unavailable(`unknown_pin`). Les nœuds internes (`bias`, `rbias`) ne sont jamais exposés.
- `Reading` : Valid, Saturated, Floating, NoReference, Undefined, Open ou Unavailable(motif). **Aucune valeur fictive** pour un état invalide, aucune valeur non finie.
- Le futur moteur CAN lira : `op_state(transceiver)`, la tension différentielle `voltage_between_pins(canh, canl)`, `logic(rxd)` et `logic(txd)`. C'est ce qui lui permet de refuser une session si le bus est invalide.

## 7. Persistance
- **Format JSON canonique**, versionné, UTF-8 sans BOM, fins de ligne LF ; seul Rust écrit ces fichiers :
  ```json
  {
    "format": "n7rt-physlab-circuit",
    "version": 1,
    "library_version": 2,
    "name": "…",
    "next_id": 42,
    "components": [
      {"id": 1, "kind": "resistor", "label": "R1", "pos": [0, 0], "rot": 0,
       "params": {"power_rating": 0.25, "resistance": 10000.0}, "state": {}}
    ],
    "wires": [
      {"id": 5, "a": {"comp": 1, "pin": "p2"}, "b": {"comp": 2, "pin": "p1"}, "bends": [[8, 0]]}
    ]
  }
  ```
  - indentation de 2 espaces, ordre des clés fixe (celui de l'exemple), clés de `params` et `state` triées ;
  - composants et fils par id croissant ; fin de ligne finale ;
  - nombres des paramètres Number en flottant au format le plus court qui relit le même `f64` (`10000.0`, `1e-12`) ; entiers en entier ; +∞ d'un paramètre `number_or_inf` écrit `"inf"` (serde_json écrirait `null`, qui ne se relit pas) ; NaN et ±∞ ailleurs : `json_invalid`.
  - `state` contient les états persistants du kind ; `pressed` n'est jamais écrit et vaut faux au chargement.
  - `library_version` autre que 1 ou 2 : `library_mismatch`, sans migration implicite ; un fichier de version 1 se charge tel quel (il ne peut pas contenir `ecu_apps` ni `mlx90377`, sinon `library_mismatch`) et s'enregistre ensuite en version 2.
  - Les paramètres cachés sont écrits dans le fichier comme les autres (clés triées).
- **Garanties testées**, sur chaque démonstration et sur 1000 circuits aléatoires à graine :
  - `load(save(c)) == c` : composants, paramètres, positions, rotations, désignateurs, fils, coudes, états persistants, `next_id` ;
  - `save(load(t)) == t` octet pour octet pour un texte canonique ;
  - les nets recalculés sont identiques ; r_in = ∞ fait l'aller-retour ;
  - chaque démonstration embarquée est canonique.
- **Chargement** : il applique les validations des éditions, plus `format_unknown`, `version_unsupported`, `library_mismatch` et `json_invalid`. Il ne corrige rien, exige des chemins canoniques, liste toutes les violations et ne charge rien partiellement (`FileError::Invalid`). Charger, nouveau ou démonstration passent en STOP, font un RESET et vident les piles d'annulation.
- **Stockage** :
  - circuits de l'utilisateur dans `<données de l'application>/circuits/<nom>.json`, nom `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`, unique sans tenir compte de la casse, noms réservés de Windows refusés (`bad_name`) ; écraser demande `overwrite` (sinon `name_exists`) ;
  - démonstrations embarquées en lecture seule (`include_str!`).

## 8. Moteur et interface

### 8.1 Moteur (`n7rt-lab::phys`)
- **`PhysCore`** : cœur déterministe, sans horloge ni thread.
  - `apply(commande)` valide la commande dans le document et la journalise avec l'indice de frame courant.
  - En STOP, il recalcule l'aperçu tout de suite. En RUN, l'effet électrique attend le prochain `step()`.
  - Abaissement :
    - au premier `step()` ou au premier aperçu qui suit un changement électrique : re-abaissement, puis `Solver::load` ;
    - géométrie seule : rien.
- **Abaissement** :
  - nets → nœuds, dans l'ordre des NetKey ; nœuds internes ensuite, par (CompId, indice local) ;
  - éléments dans l'ordre (CompId, indice local) ;
  - clés d'état = hachage stable de (CompId, nom de l'élément) ;
  - masse = net GND s'il existe.
- **RUN / STOP / RESET**, indépendants de la session N7RT-Link :
  - RUN avance par frames de 1 ms ;
  - STOP fige t et l'état, tout en acceptant éditions et interactions (aperçu) ;
  - RESET : t = 0, condensateurs déchargés, alimentations en CV, portes fermées, minuteries remises à zéro. Les états d'interaction sont conservés et l'état de marche est gardé.
- **Interactions** :
  - le document change tout de suite ; le solveur voit le dernier état à la frontière de frame suivante ;
  - plusieurs `set_pot` sur un même composant dans une frame : seul le dernier est journalisé ;
  - un relâchement de bouton `momentary` prend effet au plus tôt **après** la frame qui a vu l'appui ; en STOP, il prend effet aussitôt.
- **`PhysLab`** : thread `phys`.
  - En RUN, il calcule les frames dues d'après l'horloge monotone : au plus 20 par réveil. Au-delà, il compte le retard (`lag_frames`), se recale, et passe l'indicateur « ralenti » (architecture §14).
  - Il publie au plus toutes les 50 ms, et après chaque commande.
  - `catch_unwind` autour du cœur. En cas de panique :
    - la commande en cause est mise en quarantaine (refus `engine_error`) ;
    - le cœur est reconstruit depuis le dernier document validé, solveur réinitialisé ;
    - diagnostic `engine_restarted` ;
    - au-delà de 3 redémarrages, toute commande `phys_*` renvoie `engine_unavailable`.
- **Journal du handle** : `(frame, commande)`, au plus 10 000 entrées en RAM après chaque commande/frame. Au dépassement, les entrées les plus anciennes sont écrites par lot dans `sessions/<début>/journal.ndjson`, avec `initial.json`, puis la RAM conserve les 1 000 dernières entrées. Le cœur déterministe `PhysCore` conserve son journal intégral pour les tests de rejeu sans handle.
  - Si aucun dossier de données n'est configuré, ou si la sérialisation/écriture échoue, la même borne s'applique. Les entrées non archivées sont comptées et l'avertissement `journal_incomplete` indique explicitement la perte à l'opérateur. Ce compteur reste conservé pour la session même si le stockage fonctionne ensuite ; seul un nouveau document/session le réinitialise.
  - L'append d'un lot mémorise la longueur précédente. Un échec d'écriture, de flush ou de synchronisation tronque le fichier à cette longueur avant toute reprise. Si cette restauration échoue, l'archive est déclarée endommagée et aucun nouvel append n'y est tenté jusqu'au prochain document.
  - `incomplete.json` signale les pertes et l'éventuelle corruption lorsque le dossier est accessible. Cette écriture est elle-même susceptible d'échouer : **l'absence de ce marqueur ne prouve jamais une archive complète**. Après une perte, une reprise des appends exige d'abord l'écriture du marqueur.
  - `journal_tail(n)` fournit une fin de journal pour l'interface, jamais un export intégral. Le statut Rust `journal_status()` porte sur le préfixe disque **plus la queue encore en RAM** ; `require_lossless_session()` refuse tout export/rejeu intégral après une perte ou corruption. Ces appels restent refusés en mode mission ou sur un banc scellé, dont le journal est masqué et jamais persisté.
  - Le fichier disque contient un **préfixe**, tandis que la queue et les métadonnées nécessaires au rejeu final restent en mémoire. La fermeture ne finalise pas une archive autonome et peut donc perdre cette queue ; aucun export de session complet ni manifeste de finalisation n'est fourni en 0.2. Ne pas présenter ce fichier isolé, ni un tail, comme une preuve complète de rejeu.

### 8.2 Déterminisme et rejeu (architecture I3)
- `replay(circuit initial, journal, frames)` donne le même `digest()` que le cœur vivant.
- `digest` = FNV-1a de : texte canonique du circuit, états d'interaction, état de marche, indice de frame, `state_digest` du solveur.
- Tests de rejeu :
  - (a) sans thread : 200 éditions et 500 interactions aléatoires pendant 60 s de RUN, entrelacées d'annuler/rétablir ⇒ digest du rejeu = digest vivant, et invariants (§3.5) vrais après chaque pas ;
  - (b) le thread réel tourne 10 s avec des commandes scriptées, puis son journal est rejoué sans thread : même digest.

### 8.3 Commandes Tauri (`app/src-tauri/src/phys.rs`)
Commandes `async`, réponses en JSON selon [phys/mod.rs](../app/crates/n7rt-lab/src/phys/mod.rs). Les rejets sont un `PhysError` : `refused{refusal}`, `file{error}` ou `engine_unavailable`.

| Commande | Arguments | Réponse |
|---|---|---|
| `phys_library` | — | `KindDef[]` |
| `phys_snapshot` | — | `SnapshotDto` |
| `phys_preview` | `baseRev`, `edit` | `PreviewReply` |
| `phys_command` | `command` (`PhysCommand`) | `CommandReply` |
| `phys_values` | `sinceSeq` | `ValuesDto` ou `null` si rien de neuf |
| `phys_journal` | `n` | `JournalEntry[]` |
| `phys_new`, `phys_save`, `phys_load` | `name` (save/load) | `SnapshotDto` |
| `phys_list` | — | `FileEntry[]` |
| `phys_demos`, `phys_load_demo` | `id` (load) | `DemoEntry[]`, `SnapshotDto` |

- **Ordre** : commandes en FIFO, un appel en vol ; aperçus et glissements du curseur en « dernier seulement » ([api.ts](../app/ui/src/lab/api.ts)).
- **Valeurs** : tirées toutes les 50 ms quand la vue est active, et une fois après chaque réponse de commande, comme `lab_drain`.
- Aucun `emit` : l'expérience S1.4 montre que l'émission fait fuir le WebView2.

### 8.4 Interface
- **ViewHost** (`main.ts`) : vues cockpit et Physical Lab, une seule active ; en mode e2e, la vue de départ est le cockpit.
  - La boucle rAF ne dessine que la vue active.
  - Le Store du cockpit continue d'absorber ses lots quand il est caché, puis il est redessiné en entier.
  - Le canvas du Physical Lab ne dessine rien quand il est caché.
- **Coque** :
  - les trois emplacements de la grille sont dupliqués, marqués `[data-view]` ; les inactifs sont `hidden`, ce qui laisse le balisage du cockpit inchangé ;
  - `Shell` est limité à la racine de sa vue (`root.querySelectorAll`) ;
  - clés de disposition : `n7rt-can-lab.layout.v1` (cockpit, inchangée) et `n7rt-can-lab.layout.v1.phys` ;
  - onglets préfixés `phys-diag`, `phys-nets`, `phys-journal` ;
  - le bouton Physical Lab de la barre d'activité est activé ;
  - les titres des bascules de panneaux suivent la vue : « Bibliothèque », « Inspecteur », « Console ».
- **Panneau gauche** :
  - Bibliothèque : liste des kinds plaçables ; un appui arme le placement ;
  - Fichier : nouveau, enregistrer, ouvrir, démonstrations ; nom et indicateur de modification. Abandonner des changements non enregistrés demande un second clic sur un `.btn--danger`.
- **Barre de l'espace de travail** :
  - outils Sélection (Échap) et Fil (W) ;
  - annuler, rétablir ;
  - zoom (−, %, +, ajuster) ;
  - « Simulation : RUN / STOP / RESET », en texte, distincte du Start de la session STM32 ;
  - temps simulé en mono, à largeur fixe ;
  - indicateur « ralenti » ;
  - ligne de message (motif du dernier refus).
- **Inspecteur** (panneau droit), pour le composant sélectionné :
  - désignateur et kind ;
  - paramètres : valeur, unité, plage en infobulle, saisie selon §4 ;
  - états d'interaction : case pour l'interrupteur, bouton maintenu pour le poussoir, `.range` et champ numérique pour le curseur, case « sortie active » ou « carte alimentée » ;
  - **brochage** : broche, nom complet (connecteur), net, tension, courant ;
  - mesures (puissance, code ADC…) ;
  - état de fonctionnement (CV/CC/REV, contact, POR, mode Rs, TXD) ;
  - sources : `sources_fr`, avec les classes de provenance.
  - Pour un fil : son net et la tension du net.
- **Console** :
  - Diagnostics (gravité, code, message ; un clic sélectionne l'item) ;
  - Nets (nom, broches, tension) ;
  - Journal (300 dernières entrées) ;
  - lignes de 20 px, recyclées et virtualisées, mises à jour par `textContent` dans le rAF.
- **Clavier**, sur la racine de la vue, ignoré dans les champs de formulaire et quand le cockpit est actif :
  - Suppr ; Ctrl+Z ; Ctrl+Y ou Ctrl+Maj+Z ;
  - R (tourner) ; W (outil fil) ; Échap ;
  - F (ajuster), 1 (100 %) ;
  - Ctrl+A ;
  - Espace sur un bouton sélectionné, sans tenir compte de la répétition ;
  - flèches sur un potentiomètre sélectionné : ±0,01, Maj ±0,1.

### 8.5 Canvas (`app/ui/src/lab/canvas/`)
- **Deux canvas.**
  - Scène : fond, grille, symboles, fils, jonctions, textes ; `alpha: false`.
  - Superposition : sélection, survol, aperçus, parties dynamiques (lame, curseur, poussoir), valeurs vivantes, marqueurs de diagnostic.
- **Dessin** :
  - les gestionnaires d'entrée changent l'état et marquent du sale ; tout le dessin se fait dans la boucle rAF unique de `main.ts`, au plus une fois par couche et par image ;
  - **rectangles sales** : une édition repeint l'union des limites de dessin anciennes et nouvelles des items du correctif, plus le trait/2 + 1 px physique. On repeint le fond, la grille, puis les items de l'index dans ce rectangle, dans l'ordre fils, symboles, jonctions, textes ;
  - repeint complet seulement si le rectangle dépasse 50 % de la vue, ou au changement de vue.
- **Zoom** :
  - échelle z = 1,25^k, k ∈ [−6 ; 6] ; une unité vaut u = round(10·z·dpr) px physiques ;
  - décalages de pan entiers ;
  - trait w = max(1, round(1,5·dpr)), avec +0,5 si w est impair : traits nets à tout zoom ;
  - molette : un cran de l'échelle autour du curseur (écouteur `passive: false`) ; Ctrl+molette aussi.
  - Pan : bouton du milieu, Espace+glisser, Maj+molette.
- **Niveaux de détail** :
  - grille mineure si u_css ≥ 8, lignes majeures toutes les 10 unités, motif ou chemin groupé (jamais un `fillRect` par point) ;
  - textes si 11·z ≥ 7 px ;
  - symboles réduits à leur corps sous u_css = 3 ;
  - fils groupés en un seul chemin et un seul `stroke`.
- **DPR et taille** :
  - `matchMedia('(resolution: Xdppx)')` → réallocation ;
  - `ResizeObserver` regroupé au rAF, réallocation seulement si la taille varie de plus de 25 %.
- **Index spatial** : hachage creux de cellules de 16 × 16 unités (limites de dessin, segments, broches, jonctions), mis à jour par correctif. Il sert au pointage (tolérance 5 px CSS), à l'élimination et aux rectangles sales.
- **Pointage, par priorité** :
  1. actionneur ;
  2. broche ;
  3. jonction (sélectionne son fil) ;
  4. intérieur de fil ;
  5. corps.
- **Gestes** :
  - Placement :
    - un appui sur la bibliothèque arme le placement : silhouette sous le pointeur, aperçu validé en direct, rouge si refusé ;
    - un clic pose (`add_component`) ; R tourne la silhouette ; Échap annule ;
    - pas de glisser-déposer HTML5 (désactivé par Tauri sous Windows).
  - Fil :
    - depuis une broche, une jonction ou un fil (outil Fil), l'aperçu en L part dans la direction sortante de la broche ; `/` inverse le coude ;
    - un clic dans le vide fixe un coin ; Retour arrière retire le dernier ;
    - un clic sur une broche ou un fil termine (`add_wire` avec `Endpoint`) ; Échap annule ;
    - jamais de fil pendant.
  - Déplacement : glisser un corps ou la sélection → `move` au relâchement.
  - Segment : glisser un segment de t unités le long de sa normale n → `set_bends`. Règles :
    - segment intérieur k : Q_k += t·n, Q_{k+1} += t·n ;
    - segment qui touche une broche : dupliquer d'abord ce point (amorce perpendiculaire) ;
    - fil droit : [Q0, Q0 + t·n, Q1 + t·n, Q1] ;
    - puis canonisation par le cœur.
  - Extrémité d'un fil sélectionné : la glisser → `move_wire_end`.
  - Rectangle de sélection : de gauche à droite, items entièrement inclus ; de droite à gauche, items touchés. Maj inverse.
  - Actionneurs :
    - interrupteur : clic → `set_switch` ;
    - bouton : appui → `pressed` vrai ; relâchement, `pointercancel`, `lostpointercapture`, perte de focus de la fenêtre ou changement de vue → faux ;
    - curseur : le pointeur, ramené dans le repère du symbole, donne α = clamp((x − x_A)/(x_B − x_A), 0, 1), envoyé au plus une fois par image (« dernier seulement »).
  - Le canvas montre l'état **renvoyé** par le cœur, sans état optimiste.
- **Valeurs vivantes** :
  - seulement pour les broches du composant sélectionné et pour le net survolé, dans un cartouche (`--sch-live` sur `--sch-live-bg`) ;
  - redessinées seulement si la chaîne change : un régime établi ne redessine rien ;
  - jamais les valeurs de tous les nets.
- **Marqueurs de diagnostic** : petits carrés `--sch-invalid` ou `--sch-warn` sur l'item en cause.
- **Polices** : attendre `document.fonts.load('400 11px "IBM Plex Mono"')` avant le premier dessin, et vider le cache des largeurs de texte sur `loadingdone`.
- **Repli**, si le repeint complet mesuré dépasse 6 ms au p95 : pan par transformation CSS des deux canvas pendant le geste, repeint à la fin.

### 8.6 Budgets et protocole de mesure
- **Gabarit** : démonstration cachée `perf-150-300`, générée de façon déterministe et validée par `check()` :
  - 150 composants hors jonctions, ≥ 50 jonctions, 300 fils, sur environ 300 × 200 unités ;
  - fait de copies de sous-circuits réalistes (diviseurs, RC, contacts, ECU, transceiver, bus), en une dizaine d'îlots, chacun avec un 10 µF branché directement sur une alimentation de 10 mΩ (condensateur quasi statique : charge du solveur).
- **Scénario `--e2e phys-perf`**, sans carte : chargement (≤ 200 ms jusqu'à la première image), puis 10 s pour chaque phase :
  - pan en vue ajustée ;
  - pan au zoom 1 ;
  - balayage de zoom à la molette ;
  - glissement de l'ECU avec ses fils ;
  - glissement d'une sélection de 20 items ;
  - RUN avec glissement d'un curseur et bascule d'un interrupteur toutes les 500 ms.
  - Entrées synthétiques : un `PointerEvent` ou un `WheelEvent` par image, positionné par `Editor.worldToClient`.
- **Métriques** (T = médiane de l'écart rAF sur 2 s au repos) :

  | Métrique | Budget |
  |---|---|
  | écart entre images (`frame_gap`) | p95 ≤ 1,1·T, p99 ≤ 2·T + 1 ms, max ≤ 50 ms |
  | `render_ms` (rappel JS de l'image, dessin compris) | p95 ≤ 8 ms, p99 ≤ 12 ms |
  | gels : entrées `long-animation-frame` > 50 ms (repli `longtask`, puis écarts rAF) | 0 |
  | `input_to_draw` (horodatage de l'événement → fin du rAF qui l'a dessiné) | p95 ≤ 50 ms |
  | `edit_to_values` (début d'`invoke` → première image dessinant `rev` ≥ celui de l'édition) | p95 ≤ 100 ms |
  | retard du moteur | p99 ≤ 20 ms |
  | CPU (échantillonneur `TreeSampler`) | thread phys ≤ 5 % du total (proposé), processus mesurés et documentés |
  | mémoire, delta par rapport au cockpit seul | ≤ +40 MB, dérive ≤ 2 MB sur 10 min de RUN (à confirmer à la première mesure) |
  | solveur | sous-pas par îlot et par ms, µs par frame (p95), îlots |

Ces budgets restent ceux du protocole court Physical Lab V1. Ils ont un périmètre distinct des critères de clôture 0.2 : 300 MB absolus, dérive de 20 MB/h et deux heures de RUN représentatif sur l'exécutable final, avec provenance et continuité de collecte vérifiées. Le rapport `docs/release-0.2-closure-r1.md` du clone actif `N7-Racing-Team/n7rt-canlab` consigne cette campagne ; les anciens relevés ne valident pas le candidat courant et n'accordent aucune dérogation.

- **Précisions du protocole de mesure** :
  - `frame_gap` p99 : une synchronisation verticale manquée plus la gigue d'horodatage, d'où 2·T + 1 ms (à 144 Hz, 2·T seul échoue pour 0,1 ms de gigue) ;
  - `input_to_draw` ne compte que les entrées qui changent ce qui est dessiné : un déplacement de pan, un cran de molette qui change le zoom (pas un cran au-delà du bout de l'échelle), un pas de glissement qui change le fantôme (déplacement d'une unité de grille), un mouvement du curseur ou une bascule ; la mesure va de l'envoi de l'entrée à la fin de la première image qui dessine ensuite ;
  - glissements : le point de prise est un point entier du corps, loin des pattes, des fils et des actionneurs, au départ comme à l'arrivée, et dans la vue (même pointage que l'éditeur) ; le déplacement est accepté au préalable par l'essai à blanc du cœur ; chaque relâcher doit valider ce déplacement (sinon la phase échoue) et compte dans `edit_to_values` ;
  - mémoire : la référence « cockpit seul » est prise par le banc à l'étape `mem:cockpit`, avant l'ouverture du Physical Lab, quand la mémoire privée de l'arbre de processus est stable (2 MB sur 2 s) ; le delta est le maximum mesuré ensuite ; le banc relève aussi, sans les juger, la mémoire stabilisée au repos avec le gabarit chargé (`mem:lab`) et après les phases puis 12 s d'inactivité (`mem:fin`, le temps que Chromium réduise ses caches), ainsi que la croissance de chaque processus WebView2 au pic (processus GPU, rendu, navigateur), pour séparer l'empreinte du Physical Lab des caches de rendu de Chromium ;
  - chaque gel est rapporté avec son attribution (`long-animation-frame` : début, durée, scripts) pour en trouver la cause.

- Les résultats vont dans le rapport de validation [physical-lab-v1-validation.md](physical-lab-v1-validation.md), avec date, OS et versions. Seul Windows est mesuré dans cet incrément ; macOS et Linux sont « non mesurés ».
- **Scénario fonctionnel `--e2e phys`**, sans carte : il rejoue le test manuel de 3 minutes (§13) et compare les valeurs affichées aux attendus du §9.

## 9. Démonstrations (vérifiées analytiquement)

Les valeurs sont calculées exactement (analyse nodale en fractions rationnelles des réseaux abaissés, avec les défauts du catalogue ; script joint au rapport de démonstration). Règle : un oracle de code ADC est toujours à ≥ 0,1 LSB d'une frontière. La position α = 0,3 est retenue ; α = 0,25, 0,5 et 0,75 tombent à moins de 0,01 LSB d'une frontière et sont exclues des tests de code.

### 9.1 `ref-pot-rc` (démonstration pédagogique)
- **Netlist** :
  - V1 (5 V, 0,1 A, 10 mΩ, on) : V1.pos → S1.p1 ; S1.p2 → RV1.b ; RV1.a → GND ; V1.neg → GND ;
  - S1 : interrupteur côté haut, fermé, 10 mΩ / 1 GΩ ;
  - RV1 : 10 kΩ, α = 0,3 ;
  - RV1.w → ADC1.in ; RV1.w → R1.p1 (10 kΩ) ; R1.p2 → X ; C1 (10 µF, non polarisé) : C1.p1 = X, C1.p2 = GND ; X → ADC2.in ;
  - ADC1 et ADC2 : vref 5 V, 12 bits, r_in ∞, 1000 Hz ; `ref` → GND ;
  - un ou plusieurs symboles de masse.
  - Au chargement : S1 fermé, RV1 à 0,3, C1 déchargé. A étant à la masse, V(W) croît avec la position.
- **Formules** (R_s = r_out + r_on = 20 mΩ) : V(W) = 5·R_AW/(R + R_s) ; τ fermé = C1·(R1 + R_AW ∥ (R_WB + R_s)) ; τ ouvert = C1·(R1 + R_AW ∥ (R_WB + r_off + r_out)).
- **Contrôles calculés à partir du montage courant**, sans table de réponses fixes :
  - En régime établi, calculer V(W) par le diviseur avec les résistances réelles ; courant et puissance découlent de la loi d'Ohm et de P = V·I.
  - Calculer chaque code ADC avec la tension mesurée, `bits` et `vref`, selon la loi de quantification du §9.3 ; la tension et le code doivent varier dans le même sens que le curseur.
  - Après RESET, vérifier la continuité de la tension du condensateur et la convergence monotone vers le régime établi du nouveau réseau.
  - Calculer τ à partir de la résistance de Thévenin vue par C1, puis comparer la trajectoire à la solution du schéma discret avec le pas réel.
  - Ouvrir S1 doit supprimer l'alimentation côté haut ; W reste Referenced grâce au chemin résistif vers la masse. La décharge suit la nouvelle résistance de Thévenin.
  - Modifier R1 ou C1 doit modifier τ selon les lois du circuit, sans redémarrage moteur ni saut de tension du condensateur.

- **Solution discrète calculée** pour les tests, n ≥ 2 : v_n/V∞ = 1 − (1 + r)^−2·((1 − r/2)/(1 + r/2))^(n−2), r = h/τ, tolérance 1e−12. Les paramètres sont ceux du circuit construit pendant le test, pas des réponses tabulées.

### 9.2 `tp1-capteur-can` (base du premier TP)
Libellé : « câblage CAN du banc réel (hardware-wiring) + entrées analogiques ajoutées, absentes du banc » (hardware-wiring.md est un document du dépôt encadrant).
- **Netlist** :
  - U1 ECU (`usb` vrai, 12 bits, `can`) ;
  - U1.v3v3 → RV1.b ; RV1.a → U1.gnd ; RV1.w → U1.a0 (A0) ; RV1.w → R1 (10 kΩ) → X ; C1 10 µF de X à U1.gnd ; X → U1.a1 (A1) ;
  - U1.v5 → U2.vdd (MCP2551) ; U2.vss → U1.gnd ; U2.rs → U1.gnd ;
  - U1.can_tx (PD1) → U2.txd ; U2.rxd → U1.can_rx (PD0) ;
  - U2.canh → RT1.p1 et BUS1.canh ; U2.canl → RT1.p2 et BUS1.canl ; BUS1.gnd → U1.gnd ;
  - BUS1 : 1 nœud distant, terminaison distante ;
  - symbole de masse sur U1.gnd ;
  - RV1 α = 0,3.
- **Propriétés du montage** :
  - Les rails sont calculés sous la consommation réellement raccordée ; A0 est ratiométrique au rail de référence et A1 suit le filtre RC.
  - Le transceiver devient alimenté lorsque VDD franchit son seuil POR ; Rs relié à VSS sélectionne la haute vitesse.
  - TXD et RXD sont comparés aux seuils logiques et aux limites électriques de leurs composants, selon les paramètres normatifs du modèle.
  - Au repos récessif, CANH et CANL ont le même potentiel et leur mode commun suit le modèle de bias.
  - Hors tension, calculer la résistance entre CANH et CANL par la mise en parallèle des terminaisons et des résistances différentielles des nœuds réellement présents. L'absence de chemin résistif vers GND doit produire Open.
  - Le câblage nominal reste sans diagnostic d'erreur ni d'avertissement. Les contrôles matériels correspondants et leurs limites restent dans hardware-wiring.md du dépôt encadrant.

### 9.3 Lois calculées et propriétés du solveur (circuits construits par les tests)
- **RC** : V → R → C → masse, source de r_out 10 mΩ.
  1. Solution discrète du schéma (2 BE puis trapèzes), calculée à partir des paramètres du circuit, à 1e−12 près.
  2. Tolérance physique ≤ 0,1 point à t = τ pour r ≤ 0,05.
  - Varier R, C et le pas h ; calculer τ avec la résistance de sortie et vérifier convergence, monotonie, bornes et réduction de l'erreur lorsque h/τ diminue.
  - La tolérance de discrétisation est plus précise que celle du plan historique ; elle doit distinguer les schémas sans comparer à une table fixe de pourcentages.
- **ADC** : code = clamp(floor(V/vref·2^bits), 0, 2^bits − 1), avec les paramètres de l'entrée construite.
- **Bus** : calculer la conductance totale des terminaisons et transceivers présents ; l'ajout d'une branche parallèle positive ne peut augmenter la résistance équivalente. En récessif, respecter les seuils normatifs de mode commun et de tension différentielle du modèle.
- **Diviseur et potentiomètre** : V(W) = V·R_AW/(R_AW + R_WB + r_on + r_out) à 1e−12 relatif.
- **Contacts** : ouvert/fermé selon §9.1 ; bouton NC inverse ; bouton `latching` bascule sur front montant.
- **Continuité** : nets attendus après chaque édition des démonstrations (tests du modèle).
- **CV/CC** :
  - rampe en CC exacte (v = i_lim·t/C jusqu'à V − r_out·i_lim) ;
  - bande R_L ∈ (V/i_lim − r_out ; V/i_lim] sans faux CC ;
  - alimentation opposée à un rail de l'ECU : modes stables (pas d'oscillation), REV d'un côté.

## 10. Plan de tests

| Lot | Tests (lois, propriétés et tolérances) |
|---|---|
| S solveur (`cargo test -p n7rt-solver`) | §9.3 ; îlots (masse exclue, couplage par fuite et par Vcvs) ; régularité sans gmin (îlot sans masse : différences exactes, classe Relative) ; singulier et non fini → Unavailable ; table des pas (N ∈ SUBSTEP_COUNTS, instants entiers) ; forêt de C et τ_lb ≤ vrai τ_min sur réseaux aléatoires (comparés aux valeurs propres calculées par puissance itérée dans le test) ; aperçu (état inchangé, redistribution de charge à 1e−6) ; portes (hystérésis) ; `unpowered_resistance` calculée à partir du réseau, court-circuit et Open ; CV/CC/REV (§9.3, courant limité conforme à la consigne dans un îlot statique) ; déterminisme (`state_digest` identique sur deux exécutions de 10⁴ frames) |
| M modèle (`cargo test -p n7rt-circuit`) | Catalogue cohérent (broches sur la grille, à 1 unité du corps dans leur direction, ids uniques, défauts dans les plages) ; chaque édition et chaque refus (code et sous-code) ; jonctions (création, ordre des ids, nettoyage, doublons) ; réparation et auto-routage ; ambiguïtés ; nets et noms ; annuler/rétablir exacts ; propriété : 1000 séquences aléatoires de toutes les éditions, interactions et annuler/rétablir → invariants vrais à chaque pas, tout annuler = état initial, tout rétablir = état final ; persistance (aller-retour, octet pour octet, refus listés) ; démonstrations valides |
| E moteur (`cargo test -p n7rt-lab`) | Abaissement de chaque kind (éléments, valeurs, portes) ; démonstrations : lois et propriétés du §9 aux tolérances indiquées (1e−6 relatif sur les tensions, quantification ADC calculée) ; diagnostics (un circuit minimal par code) ; lectures invalides (flottant, sans référence, indéterminé, saturé, indisponible) ; mode mission filtré ; RUN/STOP/RESET ; édition en RUN sans redémarrage (état des C conservé) ; coalescence des interactions et relâchement différé ; I3 rejeu ; panique simulée → redémarrage puis `engine_unavailable` au 4e ; tests existants verts |
| C canvas (`npm test`) | Échelle de zoom et monde ↔ écran ; pointage par priorité ; glissement de segment ; rectangles sales ; index spatial ; machine d'états des gestes (placement, fil, glissement, actionneurs, Échap) |
| P panneaux (`npm test`) | `units.ts` : saisie (préfixes, virgule, RKM, unités, erreurs) et affichage (4 chiffres, lectures non Valid) ; couverture des glyphes (`fontTools` dans pytest, ou table de glyphes exportée) ; miroir (application de correctifs et d'instantanés) ; ViewHost (bascule cockpit → phys → cockpit sans perte d'état) |
| Intégration | `cargo test -p n7rt-link -p n7rt-lab -p n7rt-circuit -p n7rt-solver` (pas `--workspace`, qui compilerait src-tauri) ; `npm run check` (gardes Canvas compris) ; `npm test` lancé par pytest ; fixtures JSON des DTO (§11) décodées par vitest ; `--e2e phys` et `--e2e phys-perf` ; scénarios cockpit existants toujours verts, plus une bascule cockpit → phys → cockpit |

## 11. Lots de mise en œuvre et propriété des fichiers

| Lot | Fichiers (écriture exclusive) | Dépend de |
|---|---|---|
| S | `app/crates/n7rt-solver/**` | — |
| M | `app/crates/n7rt-circuit/**` (le catalogue ne change qu'avec ce document) | — |
| E | `app/crates/n7rt-lab/src/phys/**`, `app/crates/n7rt-lab/tests/phys*.rs`, `app/src-tauri/src/phys.rs`, l'enregistrement des commandes dans `app/src-tauri/src/lib.rs` | S, M (interfaces compilées) |
| C | `app/ui/src/lab/canvas/**` | `types.ts`, `state.ts`, `editor.ts` |
| P | `app/ui/src/lab/view.ts`, `app/ui/src/lab/panels/**`, `app/ui/src/lab/units.ts`, `app/ui/src/lab/e2e-phys.ts`, `app/ui/styles/lab.css`, `app/ui/index.html`, `app/ui/src/main.ts`, `app/ui/src/shell.ts` | `types.ts`, `state.ts`, `editor.ts` |
| Intégration | `tools/stm32/**`, `docs/**`, `app/scripts/**`, `app/README.md` | tous |

- Les contrats compilés (§ en-tête) ne changent que par décision d'intégration.
- Accord Rust ↔ TypeScript : le lot E génère, par un test, des fixtures JSON de chaque DTO réel (`app/ui/src/lab/__fixtures__/*.json`) ; un test vitest les décode et vérifie leurs champs face à `types.ts`.
- Chaque lot livre ses tests et passe `cargo check` / `tsc` sur tout l'espace de travail avant de rendre la main.

## 12. Traçabilité des critères d'acceptation

| Critère | Où |
|---|---|
| Espace de travail dans l'app Tauri réelle | §8.4 (ViewHost, vue Physical Lab) |
| Canvas2D : pan, zoom, sélection, glissement, multi-sélection, suppression, annuler/rétablir | §8.5, §3.2 ; tests C et M |
| Grille discrète, routage orthogonal propre, connexions et jonctions non ambiguës | §1.3, §3.4 ; tests M |
| Modèle de graphe séparé du rendu, l'UI jamais source de vérité | §3.1, §3.3 (miroir en lecture seule) |
| Bibliothèque des 12 composants | §2, catalog.rs |
| Bibliothèque version 2 : `ecu_apps`, `mlx90377`, paramètres cachés, banc de mission jamais enregistré | §2 ; apps-mission-slice.md §5 (dépôt encadrant) ; tests `n7rt-circuit/tests/hidden.rs`, `n7rt-lab/tests/phys_apps.rs` |
| Symboles propres, convention documentée | §1.6, §2.2, DESIGN.md §7 |
| Broches explicites, noms, orientation, propriétés cohérentes | catalog.rs ; §2.2 ; test de cohérence du catalogue |
| Connexions pin→pin ou pin→net, refus propres | §3.4.6, §3.6 |
| Édition en RUN sans redémarrage | §1.2, §8.1 ; test E |
| Potentiomètre réel à 3 bornes, position du curseur | §2.1, §8.5 (actionneur) |
| Bouton et interrupteur : état de contact réel | §2.1, §2.2 (dessin selon l'état) |
| Inspecteur professionnel (valeurs, unités, brochage, état) | §8.4 |
| Unités explicites et vérifiées ; aucune grandeur inventée | §1.1, §4 ; tests P |
| API pour oscilloscope, multimètre et moteur CAN, indépendante du Canvas | §6 |
| Solveur déterministe spécialisé, pas SPICE | §5 |
| Cas de référence source → potentiomètre → ADC, puis RC → ADC | §9.1 |
| Contrôles numériques analytiques : diviseur, RC, contacts, continuité | §9.3 ; tests S, M, E |
| États invalides pédagogiques jamais masqués | §3.7, §5.7, §6 |
| RUN / STOP / RESET du moteur de simulation, pas de la session STM32 ; aucun bouton « injecter une panne » | §8.1, §8.4 |
| Pas de saut de mise en page ni de reconstruction intégrale du Canvas ; couches, redessins limités, élimination, pas de DOM massif | §8.5, §8.4 (console virtualisée) |
| Mesures CPU, mémoire, fluidité ; aucun gel > 50 ms | §8.6 |
| Design system : sans arrondi, ombre, animation décorative ni composant SaaS ; `npm run check` vert | §1.6, DESIGN.md §7, garde Canvas de `check-ui` |
| Tests du modèle, connexions, valeurs, solveur, sérialisation | §10 |
| Sauvegarde et rechargement exacts | §7 ; tests M |
| Démonstration vérifiée, base du premier TP, revue explicite | §9.2 ; revue de démonstration (rapport) |
| Hors périmètre respecté ; protocole, runtime et SDK intacts ; suite existante verte | §0 ; §10 intégration |
| Commande de lancement et test manuel de 3 minutes | §13 ; rapport final |
| Captures, métriques, composants, limites du solveur, suite avant les instruments | rapport final ; §13 |

## 13. Lancement, test manuel de 3 minutes, limites et suite
- **Lancement** (depuis `app/`, environnement de `scripts/windows-gnu-env.ps1` sous Windows) : `npm run tauri dev` ; exécutable autonome : `npm run tauri build -- --no-bundle`.
- **Test manuel**, aussi rejoué par `--e2e phys` (lois et propriétés du §9.1) :
  1. Ouvrir la vue Physical Lab.
  2. Placer : alimentation DC, interrupteur, potentiomètre, résistance, condensateur, deux entrées ADC (vref 5 V), masse.
  3. Câbler selon §9.1, avec une jonction sur le fil du curseur.
  4. RUN : ADC2 converge vers la lecture d'ADC1 avec le τ calculé pour le montage courant.
  5. Déplacer le curseur dans l'inspecteur ou le canvas : la lecture ratiométrique varie dans le même sens ; ADC2 suit la réponse RC calculée.
  6. Ouvrir l'interrupteur : vérifier la décharge par le chemin résistif restant, puis la reprise après fermeture.
  7. Modifier R1 puis C1 en RUN : le τ suit les nouvelles valeurs, sans redémarrage moteur ni saut de tension de C1.
  8. Observer les tensions, courant et puissance de RV1 ; vérifier les lois du diviseur, d'Ohm et de puissance. Inspecter le brochage, l'état du curseur, les nets et les diagnostics.
  9. STOP, enregistrer, recharger : circuit identique.
- **Limites V1** (reprises dans le rapport) :
  - pas de dominant ni de dynamique du bus ;
  - pas de bruit ;
  - linéaire par morceaux, sans diode ni transistor ;
  - quasi statique sous 10 µs ;
  - consommation des circuits intégrés par une résistance équivalente (valeur max) ;
  - courant interne de Rs non modélisé ;
  - VREF+ de l'ECU supposé égal au 3V3 ;
  - mesures de performance sous Windows seulement.
- **Avant l'oscilloscope et le multimètre** :
  - sondes posées sur des broches (adresse `PinRef`) ;
  - ohmmètre par courant de test injecté dans le réseau vivant ;
  - mémoire des échantillons par frame et sous-pas (déclenchement, base de temps) ;
  - mode mission activé par le moteur de missions ;
  - pas-à-pas ;
  - couche L3 du bus pour le dominant.
