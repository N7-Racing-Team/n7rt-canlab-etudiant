# N7RT runtime v0 (S1.2) : procédure et résultats

Runtime STM32 du CAN Lab sur NUCLEO-H753ZI. Il implémente [N7RT-Link v0](n7rt-link-v0.md) sur le
VCP du ST-LINK (USART3, 921 600 Bd 8N1) et appelle le code étudiant à travers l'ABI v1 de
l'[architecture](can-lab-architecture.md) (§4). Pour cette étape, le code étudiant est un **programme
de test** dont toutes les sorties sont recalculables par le PC : ce n'est pas une mission.

## Ce qui est livré

Les sources du runtime et leurs tests hôte (toutes les lignes du tableau sauf la dernière) sont dans le projet de bring-up `firmware/stm32_h753_bringup` et dans `tools/n7rt_link/tests/test_c_runtime.py`, qui ne sont que dans le dépôt encadrant. Le dépôt étudiant n'en reçoit que le résultat compilé (image du runtime, glue) et l'en-tête public `n7rt.h`, dans le SDK ([n7rt-student-sdk-v0.md](n7rt-student-sdk-v0.md)). Les chemins `runtime/…` sont relatifs au projet de bring-up. Les outils de la dernière ligne (tests matériels) sont dans le dépôt étudiant, mais leurs étapes qui demandent l'image du runtime et son programme de test ne s'exécutent qu'avec le dépôt encadrant (procédure ci-dessous).

| Élément | Chemin |
|---|---|
| Codec C N7RT-Link v0 (portable, sans HAL) | `firmware/stm32_h753_bringup/runtime/src/n7rt_link.c/.h` |
| Cœur de session : sessions, crédits, temps bus et ticks, sorties étudiantes, erreurs (portable) | `runtime/src/n7rt_core.c/.h` |
| SHA-256 du défi d'intégrité (portable) | `runtime/src/n7rt_sha256.c/.h` |
| Cible : USART3 par interruption + FIFO, IWDG, heartbeat, faute, boucle principale | `runtime/src/n7rt_uart.c`, `n7rt_runtime_main.c` |
| ABI runtime ↔ étudiant (en-tête à `0x08040000`, table d'API à `0x08000400`) | `runtime/src/n7rt_abi.h` |
| API publique étudiante | `runtime/include/n7rt.h` |
| Glue étudiante et programme de test | `runtime/student/` |
| Linker script avec `ASSERT` d'adresses et de régions | `runtime/N7RT_H753_RUNTIME.ld` |
| Build : preset CMake `runtime`, `build_id` = hash des sources (pas de date) | `runtime/runtime.cmake`, `CMakePresets.json` |
| Tests hôte privés du code C (conformité exacte, fuzzing différentiel, états) | `tools/conformance/n7rt_link/test_exact.py`, `tools/n7rt_link/tests/test_c_runtime.py`, `runtime/tests/host/` dans le dépôt encadrant |
| Tests matériels (smoke, soak, watch) | `tools/n7rt_link/runtime_check.py`, via `tools/stm32/stm32lab.py` |

Le runtime réutilise le BSP du bring-up (`src/board.c`, `src/stm32h7xx_it.c`, `src/syscalls.c`) et les
mêmes drivers ST. Le projet CubeIDE et les presets `board-*` du bring-up sont inchangés.

## Plan mémoire vérifié à chaque build

| Région | Adresses | Contenu |
|---|---|---|
| Flash runtime | `0x0800 0000–0x0803 FFFF` | vecteurs, table d'API à `0x0800 0400`, code runtime |
| Flash étudiant | `0x0804 0000–0x080F FFFF` | en-tête étudiant à `0x0804 0000`, code étudiant |
| AXI SRAM | `0x2400 0000–0x2407 FFFF` | **pile runtime en bas** (`_estack = 0x2400 4000`), `.data`, `.bss`, anneaux UART, `.noinit` |
| DTCM | `0x2000 0000–0x2001 FFFF` | `.data` / `.bss` étudiants |

`stm32lab build --runtime` refuse une image si l'un des contrôles suivants échoue. Ils lisent l'ELF,
la map et les objets, sans carte :
1. adresses fixes (vecteurs, table d'API, en-tête étudiant, `_estack`) ;
2. taille de l'image runtime ≤ 256 Ko ;
3. symboles de `libn7rt_student.a` dans les régions étudiantes ;
4. aucune référence de l'archive étudiante vers le runtime (seulement les symboles ABI du `.ld`) ;
5. aucune référence des objets runtime vers un symbole étudiant ;
6. sections chargées de l'archive étudiante dans les régions étudiantes (map) ;
7. symboles runtime dans les régions runtime.

Le linker lui-même échoue (`ASSERT`) si une adresse ou une taille de région est violée.

## Procédure (une NUCLEO-H753ZI branchée sur CN1)

Procédure encadrant : les étapes 2 à 5 ci-dessous (`build --runtime`, `flash --runtime`, `smoke-test --runtime`, `soak`) demandent l'image du runtime et son programme de test, construits depuis le projet de bring-up, et les tests hôte du code C en compilent les sources : tout cela ne s'exécute qu'avec le dépôt encadrant. Dans le dépôt étudiant, `detect`, `watch --runtime` et `pytest tools/n7rt_link/tests` restent indépendants des fixtures privés. Les tests publics contrôlent les propriétés du codec, les layouts, le budget et la logique du contrôle matériel, sans jeu fixe de réponses ni carte. Le smoke SDK étudiant générique ne charge pas le JSON de conformité exacte ; la recette exacte reste encadrant.

Prérequis : outils STM32Cube détectés par `stm32lab` (comme pour le bring-up), `pyserial`, et pour
les tests hôte un compilateur C (`pip install ziglang`, ou `N7RT_HOST_CC`).

```text
python tools/stm32/stm32lab.py detect                 # 1. identifier la carte (SN ST-LINK, UID, port)
python tools/stm32/stm32lab.py build --runtime         # 2. build + contrôles de placement
python tools/stm32/stm32lab.py flash C --runtime       # 3. flash (SN + UID vérifiés par SWD)
python tools/stm32/stm32lab.py smoke-test C --runtime  # 4. test bidirectionnel (19 contrôles)
python tools/stm32/stm32lab.py soak C                  # 5. 10 min au plafond V1 (600 trames/s)
python tools/stm32/stm32lab.py watch C --runtime       #    surveillance passive (reboots, fautes, silences)
python -m pytest tools/n7rt_link/tests                 #    propriétés publiques du codec et du contrôle
```

Depuis le dépôt encadrant, avec le clone étudiant voisin, la vérification exacte se lance séparément :

```text
python tools/conformance/n7rt_link/vectors.py check --student ../n7rt-canlab
python -m pytest tools/conformance/n7rt_link/test_exact.py tools/n7rt_link/tests/test_c_runtime.py
```

Le compilateur C hôte est requis pour les tests C. Ces commandes et leurs fixtures restent sur le poste encadrant ; elles n'exigent pas de transporter ce dépôt sur le poste étudiant.

- **Étape 1, nouvelle carte.** Une carte absente de `boards.json` apparaît « not in boards.json ». On l'enregistre dans `tools/stm32/boards.local.json`, qui a le même schéma, ignoré par git (et par le dashboard de banc, absent du dépôt étudiant). On y recopie le SN et l'UID affichés par `detect`, avec `"images": ["runtime"]` si la carte n'a pas d'image de bring-up. Le port COM n'est jamais écrit nulle part : il est retrouvé à chaque commande à partir du SN.
- **Étape 4, ce que vérifie le smoke-test :**
  - `HELLO_ACK` au boot ;
  - identité (UID comparé au registre, DEV_ID) ;
  - défi SHA-256 égal à l'image flashée, au boot et avec un nonce aléatoire ;
  - `E_STATE` hors session ;
  - session ;
  - 21 trames de 7 variantes CAN et les ticks, au contenu et à l'ordre exacts ;
  - reprise après CRC faux, bruit, rafale de 300 octets et paquet tronqué ;
  - `E_UNKNOWN_TYPE`, `E_TIME`, `E_PAYLOAD` ;
  - compteurs d'erreurs de la carte égaux aux erreurs injectées ;
  - cadence du heartbeat ;
  - déconnexion du port au milieu d'un paquet puis reconnexion ;
  - `SESSION_STOP`, `RESET_REQ` ;
  - lien propre.
- **Étape 5, le soak (10 min) :**
  - Charge : 600 `VCAN_FRAME`/s, un `SIM_TIME` toutes les 10 ms, des heartbeats PC. Le programme de test publie 8 valeurs par trame et par tick et écrit 20 lignes de log/s. C'est le pire cas du budget V1 ([spec §13](n7rt-link-v0.md)).
  - Chaque sortie de la carte est comparée, dans l'ordre, à la prédiction du modèle (`tools/n7rt_link/selftest_model.py`).
  - PASS seulement si tous les critères sont vrais : durée ; débit ≥ 99 % ; aucune trame perdue ; sorties exactes ; aucun `NACK` ; lien PC propre ; compteurs carte (CRC, tramage, débordements, trous de seq) à 0 ; carte ayant reçu toutes les trames ; logs comptabilisés ; ni reset ni faute ; pas de silence > 750 ms ; session vivante à la fin.
- **Rapports** : `data/logs/runtime_smoke_*.json`, `runtime_soak_*.json`, `runtime_watch_*.jsonl` (dossier local, ignoré par git).
- **Récupération d'une carte** : reflasher l'image (`flash <carte> --runtime`, connect-under-reset). Aucune option byte n'est jamais touchée.

## Résultats du 2026-10-04

**Carte** (identifiée par `stm32lab detect` avant tout flash) :
- NUCLEO-H753ZI, ST-LINK `001E002D3234510433353533` (firmware V3J16M7), port `COM9` ;
- MCU DEV 0x450 rev V, UID `002D0046-3233510E-32363331` ;
- absente de `boards.json` : enregistrée comme « C » dans `boards.local.json`. Les cartes A et B n'étaient pas branchées.

**Image validée** : runtime 0.1.0, `build_id 142293fb7dd104fb`, image runtime de 20 312 o, SHA-256 `dd6adaab1ad8c106d75fc091aca7160bd0691ebb690b04a998c771685963677e`. Deux builds partant de zéro donnent ce même SHA-256.

| Test | Résultat |
|---|---|
| Contrôles de placement (`build --runtime`) | 10/10 PASS |
| Flash (SN + UID vérifiés par SWD, écriture vérifiée) | OK, 2,0 s |
| Smoke-test bidirectionnel | **PASS 19/19** : `HELLO_ACK` en 29 ms ; reboot sur `RESET_REQ` en 55 ms |
| Soak 10 min à 600 trames/s (plafond V1) | **PASS 12/12** (détail ci-dessous) |
| Sonde de capacité, 60 s à 1 000 trames/s | PASS, 59 999 trames, 0 perte |
| `watch --runtime` (passif) | 0 incident |
| Tests hôte (`pytest`, suite complète) | 90 passés, dont les 63 de départ |

**Soak V1 (`data/logs/runtime_soak_c_20261004-203957.json`)**
- **Trafic, en 600,0 s** :
  - PC → carte : 359 999 `VCAN_FRAME` (600,0/s), 59 999 `SIM_TIME` ;
  - carte → PC : 359 999 `RESULT`, 59 999 `TICK_RESULT`, 11 999 `STUDENT_LOG` ;
  - chaque réponse a le contenu et l'ordre prédits par le modèle.
- **Erreurs** : 0 perte, 0 `NACK`, 0 erreur de tramage ou de CRC (côté PC comme côté carte), 0 débordement, 0 trou de séquence, 0 log perdu.
- **Stabilité** : 0 reset, 0 faute, écart maximal entre heartbeats de 0,29 s. La file de réponses côté carte a culminé à 31 sur 32, lors d'une rafale après un hoquet du PC (223 attentes de crédit côté PC, aucun `E_CREDIT`).

**Débit : calculé et observé**

| | PC → carte | carte → PC |
|---|---|---|
| Calcul pire cas V1 ([spec §13](n7rt-link-v0.md)) | 26 064 o/s | 53 092 o/s |
| Observé sur le soak V1 | 26 064 o/s (28,3 %) | 53 092 o/s (57,6 %) |
| Observé sur la sonde à 1 000 trames/s | 42 063 o/s (45,6 %) | **81 485 o/s (88,4 %)** |

Capacité de la ligne : 92 160 o/s par sens.
- **Pourquoi l'observé égale le calcul sur le soak V1** : COBS ajoute exactement 2 octets à un paquet de moins de 254 octets, et le PC a tenu exactement les cadences prévues.
- **Ce que montre la sonde** : la ligne a réellement transporté 88 % de sa capacité sans perte. Le plafond V1 dispose donc d'une marge mesurée d'environ 1,5× en trames.

**Latence aller-retour trame → `RESULT`** (mesurée côté PC Windows, en Python) :

| Charge | p50 | p95 | p99 | max |
|---|---|---|---|---|
| Soak V1 (600 trames/s) | 2,4 ms | 6,9 ms | 15,0 ms | 155 ms |
| Sonde (1 000 trames/s) | 2,6 ms | 4,7 ms | 6,2 ms | 16,7 ms |

Les pointes viennent du PC (ordonnancement Windows, boucle Python). Côté carte, le hook le plus long dure 0,73 ms.

**Défaut trouvé et corrigé pendant la validation.** Une première image a tenu le soak V1, mais elle a été réinitialisée par l'IWDG au bout d'environ 11 s à 1 000 trames/s (`RCC_RSR = 0x04460000`).

Cause 1 : la boucle principale ne rafraîchissait l'IWDG qu'entre deux vidages complets de la file de réception. Avec la fenêtre de crédits toujours pleine, la file n'était jamais vide et le rafraîchissement n'avait plus lieu. Correction : 512 octets au plus par passe.

Cause 2 : le CPU saturait, car la flash restait à sa latence de reset (7 wait states) sans I-cache. Correction : I-cache activé ; il n'y a pas de DMA, donc aucun problème de cohérence.

Smoke, sonde à 1 000/s et soak V1 ont été refaits sur l'image corrigée, qui est celle décrite ci-dessus.

## Écarts avec l'architecture et la spec

| Point | Prévu | Réalisé | Raison |
|---|---|---|---|
| USART3 | DMA | Interruptions + FIFO matérielles 16 o + anneaux logiciels | Position exacte des octets perdus (resynchronisation), débordement explicite. Charge CPU acceptable, mesurée par la sonde à 1 000/s |
| Horloge | HSE 8 MHz | HSI 64 MHz conservée (comme le bring-up), suréchantillonnage ×8 (erreur de baud −0,08 %) | Aucune erreur de ligne en 10 min. HSE reste à faire si une dérive apparaît |
| Caches | désactivés (bring-up) | I-cache activé, D-cache désactivé | Saturation CPU mesurée sans I-cache |
| Spec v0 §9.4 | ticks sans borne | `E_TIME` au-delà de 1 000 frontières de tick | Contradiction démontrée avec l'exigence de robustesse (amendement dans la spec) |
| Isolation | V1 sans MPU | Sans MPU, IWDG gelé au débogueur, faute enregistrée puis reset | Conforme à l'architecture : MPU et SVC en I8 |
| Débit 2 MBd | à évaluer | non testé | 921 600 Bd couvre le V1 avec marge mesurée |

**Non testé sur matériel** :
- faute réelle du code étudiant : le chemin `FAULT_REPORT` n'est testé que sur hôte ;
- débranchement USB physique : seule la fermeture et la réouverture du port ont été testées ;
- build CubeIDE du bring-up : les modifications de `src/` sont gardées par `#if`, mais le build headless CubeIDE n'a pas été relancé.

## Suite : S1.3

Fait le 2026-10-04 : ce runtime, inchangé, est distribué dans le SDK étudiant. Voir [n7rt-student-sdk-v0.md](n7rt-student-sdk-v0.md).
