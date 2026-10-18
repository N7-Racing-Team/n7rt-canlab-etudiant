# Application étudiant N7RT CAN Lab v0 (S1.4) : cockpit Virtual CAN

**Résultat.** Une vraie fenêtre Tauri 2 « N7RT CAN Lab » fait tout le parcours sur la NUCLEO-H753ZI :
1. elle détecte la carte, sans port COM codé en dur ;
2. elle établit N7RT-Link v0 et vérifie le runtime par son défi SHA-256 ;
3. elle pilote une session Virtual CAN ;
4. elle affiche en direct les `RESULT`, `STUDENT_LOG` et `TICK_RESULT` produits par `student_decoder.c` sur la carte.

Ce document se lit avec [app/README.md](../app/README.md) (build, prérequis, règles de l'UI) et [n7rt-link-v0.md](n7rt-link-v0.md) (protocole).

Le parcours de code intégré ajouté le 2026-10-08 est documenté dans [le tutoriel du mini-IDE](inapp-student-ide-tutorial.md) et [son rapport R1](inapp-student-ide-r1.md). Les résultats ci-dessous décrivent le cockpit historique ; les preuves du mini-IDE sont consignées séparément.

## 1. Ce que fait l'application

| Zone du cockpit | Contenu |
|---|---|
| Barre de connexion | Liste des ports (ST-LINK en tête, identifiés par VID/PID et numéro de série), Connecter / Déconnecter, état du lien (`READY`, `SESSION`, `PORT_BUSY`, `LOST`…) et surcouches `STALLED` / `STUDENT_BUSY` |
| Carte | Port, sonde, runtime + build + ABI, **défi SHA-256 vérifié**, statut et CRC du code étudiant, UID MCU, cause du dernier reset, dernière faute |
| Session | Période de tick, Start / Stop, identifiant, durée, temps bus |
| Stimuli (ECU virtuel) | Potentiomètre, bouton, format (2.0A, 2.0B, FD, FD+BRS, RTR), identifiant, DLC, cadence 0–1000 trames/s, envoi unitaire |
| Compteurs | Trames/s émises et reçues, envoyées, `RESULT`, en vol / fenêtre, aller-retour p50/p99, NACK, perdues |
| Trames CAN | Session, n°, t, ID, type, DLC, données brutes, résultat (`OK`, `NACK E_…`, perdue), publications du code étudiant, cycles, aller-retour |
| Résultats étudiants | Dernière valeur par slot (trame ou tick) |
| Logs | `n7rt_log()` du code étudiant |
| Erreurs | Événements de liaison, compteurs d'erreurs de la carte (CRC, tramage, trous de seq, débordements UART, logs perdus) |
| Diagnostic | Files, contre-pression, temps de boucle, latences UI |

**Pas de décodage dans l'UI.** Elle montre les octets bruts et ce que le code étudiant a publié.
- L'ECU virtuel suit un encodage d'entraînement public et fixe : octet 0 = potentiomètre, 1 = bouton, 2 = compteur. Il vit dans le cœur (`crates/n7rt-lab/src/sim.rs`) et n'est jamais affiché.
- Aucun mapping, DBC ou secret de mission ne se trouve dans l'application.

## 2. Architecture

```text
UI (WebView, TypeScript) ── invoke (commandes, lab_drain 20 Hz) ──► src-tauri (coquille)
                                                                       │
                                     n7rt-lab (cœur, aucun code UI) ◄──┘
           thread sim  ── file bus (1024, contre-pression 512) ──► thread link ── série 921 600 Bd ──► STM32
           (ECU virtuel, ticks)                                    (N7RT-Link : crédits W=32, RTT, pertes, reconnexion)
                                                     n7rt-link (codec : COBS, CRC, messages, récepteur)
```

- **`n7rt-link`** : codec Rust sans dépendance.
- **`n7rt-lab`** :
  - découverte des ports ;
  - identification de la carte : runtime épinglé par `runtime-release.json`, image extraite au build du SDK pour vérifier le défi ;
  - moteur de liaison et générateur ;
  - métriques ;
  - vérification e2e ;
  - fausse carte pour les tests.
- **Thread UI.** Il ne bloque jamais. Les commandes ne font que poster dans une file. L'UI tire les changements par `lab_drain` toutes les 50 ms et rend au `requestAnimationFrame`, uniquement si quelque chose a changé.
- **Panique.** Les threads `link` et `sim` sont sous `catch_unwind`, avec au plus 3 redémarrages signalés à l'UI, conformément à l'architecture §8.

### Files et tampons (définis et mesurés)

| File | Capacité | Politique | Mesure à 600 trames/s, 5 min |
|---|---|---|---|
| sim → link (trames + `SIM_TIME`) | 1 024 éléments | Au-delà de 512, l'horloge bus se met en pause : rien n'est jeté, et la pause est comptée | max 13, 0 pause, 0 rejet |
| Trames en vol vers la carte | W = `HELLO_ACK.rx_window` = 32 | Crédits du §9.2. Une trame comptée perdue libère son crédit | max 11 |
| Tampon d'émission série | 2 Ko par passe, 64 Ko max | Jamais tronqué ni dupliqué (§5.4) | 0 octet en attente |
| Lignes vers l'UI, par lot | 128 | Les plus anciennes ne sont pas affichées, et elles sont comptées | — |
| Lignes / logs / erreurs gardés par l'UI | 200 / 300 / 200 | Les `<tr>` sont recyclés : aucune création d'élément en régime établi | — |
| Enregistrement e2e (mode test) | 8 192 lignes + oracle incrémental | Mémoire constante, quelle que soit la durée | 8 Mo pour le processus après 180 000 trames |

## 3. Conformité au protocole

- **Conformité exacte historique.** Au checkpoint de ce rapport, le codec Rust passait le jeu normatif de conformité, désormais conservé dans le dépôt encadrant :
  - ancres CRC et COBS ;
  - 30 paquets, à l'octet près, dans les deux sens ;
  - 34 flux, chacun sous 6 découpages différents.
  Ces résultats ne sont pas réattribués aux tests publics actuels, qui contrôlent des propriétés sans fixture de réponse fixe. Le dépôt étudiant ne contient plus de lien actif vers l'ancien JSON.
- **Fausse carte.** Le moteur est testé contre une carte en mémoire (`crates/n7rt-lab/tests/engine.rs`, 9 tests) :
  - session et trames, redémarrage de la carte, port occupé, cœur arrêté ;
  - pertes, perte d'octets pendant un arrêt, octets résiduels à l'ouverture ;
  - écriture incertaine, débranchement.
- **Sur la carte.** Aucune erreur de tramage, aucun rejet, aucun trou de numérotation sur l'ensemble des essais ci-dessous.

## 4. Validation sur la carte réelle

Banc : carte C, NUCLEO-H753ZI, ST-LINK `001E002D3234510433353533` (STLINK-V3E, PID `0x374E` mesuré), vue comme COM9. Elle exécute l'image étudiante S1.3 (runtime 0.1.0 `142293fb7dd104fb`, CRC étudiant `0x5CBC80B3`).

Commande : `python tools/stm32/stm32lab.py app-test C`. L'application lancée avec `--e2e` clique ses propres contrôles, relit ce que sa fenêtre affiche et fait comparer ces lignes par le cœur avec les trames et réponses réellement échangées.

### Scénario fonctionnel coordonné : PASS 15/15

Build release ; rapport `data/logs/app_functional_c_20261005-004957.json`. Le même scénario passe aussi en build debug (`app_functional_c_20261005-003856.json`) ; les durées ci-dessous viennent de ce dernier.

| Étape | Résultat |
|---|---|
| Connexion (sélection par numéro de série, Connecter) | `READY` en 64 ms ; défi SHA-256 vérifié ; code étudiant chargé |
| Session | `SESSION_ACK` ; log `student_decoder ready` affiché |
| 8 variantes via le panneau de stimuli (2.0A, 2.0B, RTR, DLC 0, FD+BRS 64 o, FD 12 o, DLC 15 classique, ID 0) | `RESULT` exact pour chacune |
| Rafale 100 trames/s, potentiomètre variable | 211 trames, 0 perte |
| Stop puis Start | Nouvelle session, trames OK |
| Déconnecter puis Connecter | Poignée de main en 62 ms ; trames OK |
| `RESET_REQ` (redémarrage de la carte) | `HELLO_ACK(boot)` détecté ; session relancée automatiquement en 107 ms |
| Arrêt du cœur 6 s par SWD (équivalent d'un point d'arrêt CubeIDE) | `STALLED` détecté après 921 ms, envois suspendus ; à la reprise, les 32 trames englouties par la carte (débordement UART, cœur arrêté) sont comptées perdues en un message ; le trafic reprend dans la même session |
| Port occupé par un autre programme | État `PORT_BUSY` avec « Le port COM9 est déjà ouvert par une autre application (console série de STM32CubeIDE…). Fermez-la : la connexion reprendra seule. » ; reconnexion 1,07 s après libération |
| Vérifications de l'affichage (5 fois) | 200/200 lignes affichées = réponses réelles de la STM32 (ID, type, DLC, données, résultat, publications au bit près) ; oracle du modèle exemple vérifié sur 483 trames |

### Charge au plafond V1 : 600 trames/s pendant 5 min

Build release ; rapport `data/logs/app_perf_c_20261005-005013.json`.

| Mesure | Résultat | Budget (architecture §14) |
|---|---|---|
| Trames | 180 106 envoyées, 180 106 `RESULT` ; **0 perte, 0 NACK, 0 erreur de liaison** | 0 perte |
| Affichage | 200/200 lignes = réponses de la carte ; oracle vérifié sur les 180 106 trames | — |
| Aller-retour trame → `RESULT` | p50 3,10 ms, p99 6,17 ms, max 27 ms | p99 ≤ 10 ms : **tenu** |
| CPU (application + WebView2, part de la machine) | moyenne 4,2 %, max 8,3 % (Intel Core 5 210H, 12 threads logiques) | ≤ 25 % : **tenu** |
| Latence stimulus UI → valeur affichée, venue de la STM32 | p50 39 ms, p95 67 ms, max 82 ms (600 changements) | ≤ 100 ms : **tenu** |
| Rendu UI | 6,9 ms par mise à jour au p95 ; intervalle entre images p99 14 ms, **max 83 ms** | aucun gel > 50 ms : **dépassé ponctuellement** (un intervalle de 83 ms en 5 min, cause non identifiée : GC ou ordonnanceur) |
| Mémoire privée (application + 6 processus WebView2) | **326 Mo**, stable : application 8,7 Mo, rendu WebView 157 Mo en palier après 3 min, GPU 94 Mo, navigateur 41 Mo, utilitaires 23 Mo | ≤ 300 Mo : **dépassé de 9 %** (voir §6) |

### Après la refonte visuelle (design system N7RT, même build release, 2026-10-05)

Règles et jetons : [app/ui/DESIGN.md](../app/ui/DESIGN.md). Aperçus des écrans réels : [docs/img/can-lab-ui/](img/can-lab-ui/), capturés par `stm32lab app-test C --scenario showcase`.

| Mesure, 600 trames/s, 5 min | Avant | Après |
|---|---|---|
| Scénario fonctionnel, vérifications de l'affichage | 15/15, 5/5 | 15/15, 5/5 (`app_functional_c_20261005-011212.json`) |
| Pertes, NACK, erreurs de liaison | 0 | 0 (180 087 trames, `app_perf_c_20261005-011230.json`) |
| Aller-retour p99 | 6,17 ms | 6,20 ms |
| CPU moyen | 4,2 % | 4,2 % |
| Latence stimulus → affichage p95 | 67 ms | 67 ms |
| Rendu par mise à jour, p95 | 6,9 ms | 8,9 ms (plus d'éléments vivants : barre d'état, onglets) |
| Plus long intervalle entre images | 83 ms | 29 ms |
| Mémoire privée | 326 Mo | 311 Mo |

## 5. Défauts trouvés et corrigés pendant la validation

Chacun est couvert par un test qui échoue sans la correction : test de mutation fait pour les points 4 et 5.

1. **Octets résiduels à l'ouverture du port.**
   - Constat : un `E_CRC`, un faux trou de numérotation, un `SESSION_ACK` d'une connexion précédente.
   - Correction : purge du tampon d'entrée ; tout ce qui précède le `HELLO_ACK` de la connexion est ignoré ; un `HELLO_ACK` étranger (défi faux) n'est pas pris pour une carte incompatible.
2. **Fuite de crédits.**
   - Constat : les trames perdues pendant un arrêt du cœur gardaient leur crédit, ce qui bloquait définitivement les envois.
   - Correction : une trame résolue (répondue ou perdue) ne compte plus en vol.
3. **Faux « port perdu ».**
   - Constat : le ST-LINK cesse brièvement d'accepter des octets pendant qu'il ferme une session SWD.
   - Correction : écriture non bloquante avec reprise. Le port n'est déclaré perdu qu'après 3 s sans progrès.
4. **Duplication d'octets sous Windows.**
   - Cause : une écriture interrompue par le délai d'attente (commun à la lecture, 2 ms, dans `serialport`) peut annoncer « 0 octet écrit » alors que les octets sont partis. Les reprendre dupliquait des paquets, d'où des `E_TIME`.
   - Correction : écriture avec son propre délai de 2 s (`SetCommTimeouts`) ; une écriture incomplète réinitialise la liaison au lieu d'être rejouée.
5. **Thread `sim` arrêté à 600 trames/s.**
   - Cause : course sur le compteur d'occupation de la file. Le dépassement arithmétique faisait paniquer le thread, silencieusement en debug.
   - Correction : comptage avant publication. Le thread est sous `catch_unwind` avec redémarrage signalé.
6. **WebView qui grossissait d'environ 1,8 Mo/s.**
   - Cause : les données étaient poussées par `emit`.
   - Correction : l'UI tire les changements (`lab_drain`). La mémoire du rendu se stabilise.

## 6. Écarts et limites

- **Mémoire : 327 Mo pour 300 Mo budgétés.**
  - L'application elle-même pèse 8 Mo. Le reste revient à WebView2 : environ 250 Mo au repos (6 processus), puis le tas V8 du rendu qui se stabilise à environ 157 Mo sous charge.
  - Plafonner V8 (`--js-flags=--max-old-space-size=96`) ramène le total à 310 Mo. Ce plafond est écarté : c'est une contrainte cachée qui ferait planter un futur écran plus gourmand.
  - Chromium dimensionne ses tas selon la RAM : la mesure doit être refaite sur le PC de référence (8 Go).
  - **Le budget est à arbitrer** : réviser le chiffre, ou réduire la part fixe de WebView2.
- **Plateformes.**
  - Construit et testé avec la carte : Windows 11 seulement.
  - Préparé pour macOS et Linux, mais **ni construit ni testé** : `serialport` IOKit/udev, cibles `dmg`/`deb`/`AppImage`, `/dev/cu.*` préféré sous macOS, message `dialout` sous Linux.
  - À faire : mesurer les PID ST-LINK sous macOS et Linux.
- **Chaîne Windows du banc.**
  - Il n'y a ni MSVC ni droits admin. Rust GNU, zig 0.15.2 et `tools/zig-shims` sont installés au niveau utilisateur, et l'édition de liens passe par `rust-lld` (voir le README).
  - La chaîne MSVC standard n'a pas été essayée ici.
- **Fluidité.** Avant la refonte, un intervalle de 83 ms entre deux images était apparu en 5 min. Après la refonte (rangées recyclées, ancrage), le maximum mesuré est de 29 ms. Une seule mesure de 5 min ne suffit pas à garantir le budget « aucun gel > 50 ms » : à confirmer par le soak de 2 h.
- **CubeIDE ouvert.**
  - L'application ne partage avec CubeIDE que le port série virtuel. Les deux cas réels ont été testés :
    - un autre programme tient le port : conflit signalé, puis reprise automatique ;
    - le cœur est arrêté par le débogueur : `STALLED`, puis reprise. L'arrêt a été produit par SWD avec STM32_Programmer_CLI, pas par l'interface graphique de CubeIDE (machine à court de mémoire).
- **Débranchement physique.** Il n'est pas automatisable sur ce banc. Il est couvert par le test du moteur sur la fausse carte (reconnexion < 3 s) et doit être vérifié à la main (test de 2 minutes ci-dessous).
- **Fonctions absentes, par conception** : oscilloscope, multimètre, câblage, composants, fautes physiques, missions, serveur, CAN physique.

## 7. Lancer l'application et test manuel de 2 minutes

```powershell
# Build release, depuis la racine du dépôt (Windows sans MSVC : charger d'abord l'environnement)
cd app
. .\scripts\windows-gnu-env.ps1
npm run build; cargo build --release -p n7rt-can-lab --features tauri/custom-protocol
# Lancer
.\target\release\n7rt-can-lab.exe
```

Avec la H753ZI branchée par CN1, et l'image étudiante de S1.3 ou toute image construite depuis le projet CubeIDE :

1. **Démarrage (≤ 5 s).** La fenêtre « N7RT CAN Lab » s'ouvre et se connecte seule si une seule carte est présente : `Prête`, « défi SHA-256 vérifié », code étudiant chargé avec son CRC.
2. **Start.** L'état passe à `Session active`. Le log `student_decoder ready` apparaît.
3. **Envoyer une trame.** Une ligne `OK` s'affiche avec `s0=8 · s1=128`. Bouger le potentiomètre puis renvoyer : `s1` suit la valeur.
4. **Format « Remote (RTR) »** puis envoi : `s0=0 · s1=rejet`. Puis « CAN FD + BRS », DLC 15 : 64 octets, `s0=64`.
5. **Cadence 600.**
   - Environ 600 trames/s émises et reçues ; aller-retour p50 d'environ 3 ms.
   - NACK et perdues à 0.
   - Le tableau défile sans saccade.
   - Remettre la cadence à 0.
6. **Stop puis Start** : nouvelle session. **Déconnecter puis Connecter** : retour à `Prête`.
7. **Débrancher l'USB 5 s, puis rebrancher** :
   - au débranchement : `Liaison perdue` ;
   - au rebranchement : retour seul à `Prête`, et session relancée si elle tournait.
8. **Flasher le projet depuis CubeIDE** pendant que l'application tourne. La carte redémarre, l'application l'annonce, refait la poignée de main et affiche le nouveau CRC étudiant.

## 8. Suite

- **Physical Lab** (S1.6). Il s'ajoute comme une vue Canvas2D sur le même `Store`.
- **Mesures restant à faire** :
  - mémoire et CPU sur le PC de référence et sur les 3 OS ;
  - soak de 2 h (dérive mémoire) ;
  - 20 débranchements réels.
- **Installeurs** NSIS, deb/AppImage et dmg : à produire et valider en CI.
- **Arbitrage** du budget mémoire (§6).
