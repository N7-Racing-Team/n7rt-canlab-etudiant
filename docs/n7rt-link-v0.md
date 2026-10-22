# N7RT-Link v0 — Spécification normative

> **Statut** : **figé**, version de protocole `0`, 2026-10-04. Toute modification du comportement normatif de v0 impose une nouvelle version (§15).
> **Rôle** : contrat du lien série entre l'appli PC du CAN Lab et le runtime N7RT de la NUCLEO-H753ZI, pour le TP simulé. L'architecture d'ensemble est dans [can-lab-architecture.md](can-lab-architecture.md) ; en cas d'écart sur le protocole, **ce document fait foi**.
> **Vocabulaire** : DOIT / NE DOIT PAS = obligation ; DEVRAIT = recommandation dont l'écart doit être justifié ; PEUT = option.

| Artefact | Chemin | Rôle |
|---|---|---|
| Spécification | `docs/n7rt-link-v0.md` | Contrat normatif (ce document) |
| Codec public | `tools/n7rt_link/n7rt_link.py` | Implémentation du contrat (Python stdlib), sans réponse de mission |
| Outil documentaire public | `tools/n7rt_link/protocol_docs.py` | Layouts, génération des tables et calcul du budget ; aucun fixture de réponse |
| Tests publics | `tools/n7rt_link/tests/test_n7rt_link.py` | Propriétés, fragmentation, bornes et rejets ; lancés par `python -m pytest` |
| Conformité exacte encadrant | Dépôt encadrant restreint, `tools/conformance/n7rt_link/` | Fixtures et vérification des octets conservés hors dépôt/build étudiant |

**Conformité.** Une implémentation (runtime C, client Rust) est conforme si elle satisfait les DOIT de ce document et les vérifications du §14. Le dépôt étudiant expose les propriétés du contrat ; la comparaison exacte avec les fixtures reste dans le dépôt encadrant.

---

## 1. Portée

**Dans v0** :
- le tramage ;
- l'identification et la compatibilité du runtime ;
- les sessions ;
- la livraison de trames CAN virtuelles au code étudiant et le retour de ses publications ;
- le temps bus ;
- les journaux étudiants ;
- les rapports de faute ;
- la supervision du lien.

**Hors v0** (types réservés, §6) :
- l'état du contrôleur CAN virtuel (`VCAN_STATUS`, incrément I4) ;
- l'émission de trames par le code étudiant (`VCAN_TX`, V2) ;
- le CAN physique ;
- le protocole avec le serveur de missions.

---

## 2. Couche physique

- Liaison : USART3 de la NUCLEO (PD8 TX, PD9 RX) ↔ port série virtuel du STLINK-V3E, vu par le PC comme `COMx`, `/dev/cu.usbmodem*` ou `/dev/ttyACM*`.
- Paramètres : **921 600 Bd, 8 bits de données, sans parité, 1 bit de stop (8N1), sans contrôle de flux**, full duplex.
- Débit utile par sens : 92 160 octets/s (10 bits par octet).

Un débit de 2 000 000 Bd n'est pas couvert par v0. Il faudra le valider sur les trois OS (tâche S1.2 de l'architecture), puis le négocier dans une version ultérieure.

---

## 3. Représentation des données

- Tous les entiers multi-octets sont en **little-endian**.
- `f32` = IEEE 754 binary32, little-endian.
- `u8[N]` = octets bruts, dans l'ordre.
- Texte = UTF-8 **valide**, **sans octet nul**, de longueur explicite.
- Les structures sont **sans padding**. Les offsets des tableaux du §7 partent du début de la charge utile.

---

## 4. Paquet et tramage

### 4.1 Paquet

`paquet = en-tête (6 o) ‖ charge utile (len o) ‖ CRC (4 o)`

| Offset | Champ | Type | Règle |
|---:|---|---|---|
| 0 | `proto` | u8 | Version de protocole : `0` en v0 |
| 1 | `type` | u8 | Type de message (§6, §7) |
| 2 | `seq` | u16 | Numéro de paquet de l'émetteur (§9.1) |
| 4 | `len` | u16 | Longueur de la charge utile, **≤ 255** |
| 6 | charge utile | u8[len] | Selon `type` |
| 6 + len | `crc` | u32 | CRC-32 de l'en-tête et de la charge utile |

Limites : `MIN_PACKET` = 10, `MAX_PAYLOAD` = 255, `MAX_PACKET` = 265 octets.

### 4.2 CRC
- **CRC-32/ISO-HDLC**, identique à `zlib.crc32` :
  - polynôme 0x04C11DB7, en forme réfléchie (0xEDB88320) ;
  - valeur initiale 0xFFFFFFFF ; entrée et sortie réfléchies ; XOR final 0xFFFFFFFF.
- Couverture : octets 0 à 5 + len. Le CRC est stocké en little-endian.
- *Informatif — unité CRC du STM32H7* : `POL` 0x04C11DB7 (défaut), `INIT` 0xFFFFFFFF, inversion d'entrée par octet, inversion de sortie, puis XOR 0xFFFFFFFF en logiciel. La conformité exacte des octets est vérifiée par les tests encadrant.

### 4.3 COBS et délimiteur
- `fil = COBS(paquet) ‖ 0x00`. L'encodage ne contient jamais `0x00` ; `0x00` sert de délimiteur.
- **L'encodeur DOIT être canonique** (Cheshire & Baker) :
  - chaque bloc fait au plus 254 octets non nuls, précédés d'un octet de code égal à (taille + 1) ;
  - un zéro termine un bloc ;
  - un bloc plein de 254 octets a pour code `0xFF` et ne consomme pas de zéro ;
  - **aucun bloc vide n'est ajouté** après un bloc final de 254 octets.
  Les tests publics contrôlent ces règles, les bornes et l'aller-retour sur des entrées construites pendant le test ; les fixtures exacts sont réservés aux encadrants.
- Le décodeur DEVRAIT accepter aussi la variante non canonique (bloc vide final `01`). Il DOIT refuser :
  - un octet de code `0x00` ;
  - un bloc qui dépasse la fin de la trame.
- Taille maximale encodée : `MAX_ENCODED` = **267 octets**, délimiteur exclu (= 265 + 2). Taille maximale sur le fil : 268 octets.
- **Émission** :
  - Un émetteur DOIT envoyer un `0x00` avant son premier paquet après l'ouverture du port (PC) ou le boot (carte). Cela purge un éventuel reste de trame chez le récepteur.
  - Un émetteur PEUT envoyer des `0x00` supplémentaires à tout moment (trames vides).

---

## 5. Réception, erreurs de tramage et resynchronisation

Le récepteur accumule les octets jusqu'à un `0x00`, puis traite la trame accumulée. **Il DOIT appliquer les contrôles dans cet ordre** ; le premier qui échoue détermine l'événement :

| # | Contrôle | Événement si échec |
|---|---|---|
| 1 | Plus de 267 octets reçus sans délimiter. Les octets suivants sont ignorés jusqu'au prochain `0x00`, et un seul événement est émis à ce `0x00` | `frame_error E_OVERFLOW` |
| 2 | Trame vide (deux `0x00` consécutifs) | *aucun événement* : ignorée |
| 3 | Décodage COBS (§4.3) | `frame_error E_COBS` |
| 4 | Paquet décodé ≥ 10 octets | `frame_error E_SHORT` |
| 5 | CRC (§4.2) | `frame_error E_CRC` |
| 6 | `proto` = 0 | `reject E_VERSION` |
| 7 | `len` ≤ 255 **et** `len` = taille décodée − 10 | `reject E_LENGTH` |
| 8 | `type` accepté dans ce sens (§6) | `reject E_UNKNOWN_TYPE` |
| 9 | Charge utile valide pour ce type (§7, §8) | `reject E_PAYLOAD` |
| — | Tous les contrôles passent | `packet` |

Que faire de chaque événement :
- **`frame_error`** : rien n'est fiable, pas même `seq` ni `type`. Le paquet est ignoré **sans réponse**, et l'erreur est comptée (champs `rx_framing_errors` et `rx_crc_errors` du `HEARTBEAT` de la carte).
- **`reject`** : l'en-tête est fiable (le CRC est bon). La carte DOIT répondre `NACK(ref_seq, ref_type, code)`. Le PC PEUT répondre `NACK` ; la carte se contente de le compter.
- **Pas de boucle de NACK** : un `NACK` reçu ne DOIT jamais déclencher de `NACK` en retour.
- **Resynchronisation** : elle est garantie par le délimiteur. Après n'importe quelle erreur, le paquet qui suit le prochain `0x00` valide est reçu normalement ; cette propriété est testée après corruption et fragmentation.
- **Découpage indifférent** : les événements ne DOIVENT pas dépendre de la manière dont les octets arrivent (octet par octet, par blocs DMA…). Des octets en attente sans délimiteur final ne produisent aucun événement.

---

## 6. Versionnement, espace des types, extension

- **Invariants de toutes les versions** :
  - en-tête de 6 octets ;
  - tramage COBS + `0x00` ;
  - CRC-32/ISO-HDLC ;
  - plages de types.
- **Négociation**
  - `HELLO` et `HELLO_ACK` sont **toujours** émis avec `proto = 0` dans l'en-tête, quelle que soit la version de l'émetteur. Ainsi toute carte peut décoder la négociation.
  - `HELLO` annonce `[proto_min, proto_max]` ; `HELLO_ACK.proto` donne la version de la carte. Le lien est compatible si `proto_min ≤ HELLO_ACK.proto ≤ proto_max`. Sinon, le PC passe en `INCOMPATIBLE` et n'envoie plus que des `HELLO`.
  - Ensuite, tous les paquets portent la version négociée, c'est-à-dire `0` en v0.
- **Types** :
  - `0x01–0x7F` : PC → carte ; `0x80–0xFF` : carte → PC ;
  - un type d'un autre sens, ou absent du §7, donne `E_UNKNOWN_TYPE` ;
  - **types réservés**, refusés en v0 : `0x11 VCAN_STATUS` (I4), `0x93 VCAN_TX` (V2).
- **Extension compatible**
  - Une version future PEUT **ajouter des octets à la fin** d'une charge utile. Un récepteur v0 DOIT ignorer les octets qui suivent la partie définie ; les tests construisent des extensions pour vérifier cette propriété.
  - Aucune version ne DOIT modifier la taille, la position ou le sens d'un champ existant.
  - Les bits et valeurs réservés DOIVENT valoir 0 et sont vérifiés (`E_PAYLOAD`) : les utiliser exige une nouvelle version.

---

## 7. Messages

### 7.1 Vue d'ensemble

| Type | Nom | Sens | Réponse attendue |
|---|---|---|---|
| `0x01` | HELLO | PC → carte | `HELLO_ACK(reason = 0)` |
| `0x02` | SESSION_START | PC → carte | `SESSION_ACK(op = 1)` |
| `0x03` | SESSION_STOP | PC → carte | `SESSION_ACK(op = 2)` |
| `0x04` | RESET_REQ | PC → carte | reset, puis `HELLO_ACK(reason = 1)` |
| `0x05` | NACK | PC → carte | aucune |
| `0x10` | VCAN_FRAME | PC → carte | **exactement un** `RESULT` ou `NACK` (§9.2) |
| `0x12` | SIM_TIME | PC → carte | un `TICK_RESULT` par tick franchi (§9.4) |
| `0x7F` | HEARTBEAT | PC → carte | aucune |
| `0x81` | HELLO_ACK | carte → PC | — |
| `0x82` | SESSION_ACK | carte → PC | — |
| `0x83` | FAULT_REPORT | carte → PC | — |
| `0x84` | NACK | carte → PC | — |
| `0x90` | RESULT | carte → PC | — |
| `0x91` | TICK_RESULT | carte → PC | — |
| `0x92` | STUDENT_LOG | carte → PC | — |
| `0xFF` | HEARTBEAT | carte → PC | — |

### 7.2 Disposition des charges utiles

Les tableaux suivants sont générés depuis `tools/n7rt_link/n7rt_link.py`. Un test vérifie qu'ils sont identiques au code.

<!-- BEGIN GENERATED: layouts -->
#### `0x01` HELLO — PC → carte

Ouvre ou rouvre le dialogue ; autorisé dans tout état.

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `proto_min` | u8 | plus petite version de protocole acceptée par le PC |
| 1 | `proto_max` | u8 | plus grande version acceptée (≥ proto_min) |
| 2 | `nonce` | u8[16] | défi aléatoire, renvoyé haché dans HELLO_ACK.challenge |

Charge utile définie : 18 octets.

#### `0x02` SESSION_START — PC → carte

Réinitialise la RAM étudiante et appelle student_init().

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `session_id` | u32 | identifiant de session ≠ 0 |
| 4 | `mode` | u8 | 1 = exploration, 2 = qualification |
| 5 | `tick_period_ms` | u16 | période de student_on_tick en temps bus, 0 = pas de tick, ≤ 1000 |

Charge utile définie : 7 octets.

#### `0x03` SESSION_STOP — PC → carte

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `session_id` | u32 | session à fermer (≠ 0) |

Charge utile définie : 4 octets.

#### `0x04` RESET_REQ — PC → carte

La carte se réinitialise en ≤ 50 ms ; aucune réponse avant HELLO_ACK(boot).

| Offset | Champ | Type | Description |
|---:|---|---|---|

Charge utile définie : 0 octets.

#### `0x05` NACK — PC → carte

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `ref_seq` | u16 | seq de l'en-tête du paquet refusé |
| 2 | `ref_type` | u8 | type du paquet refusé |
| 3 | `code` | u8 | code d'erreur (§11) |

Charge utile définie : 4 octets.

#### `0x10` VCAN_FRAME — PC → carte

VirtualCanFrame : une trame reçue par le nœud étudiant.

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `frame_seq` | u32 | numéro de trame dans la session, 1, 2, 3… (≠ 0) |
| 4 | `t_us` | u64 | instant de réception, µs de temps bus depuis le début de session |
| 12 | `can_id` | u32 | identifiant 11 bits, ou 29 bits si CAN_EXT |
| 16 | `flags` | u8 | CAN_EXT 0x01 · CAN_RTR 0x02 · CAN_FD 0x04 · CAN_BRS 0x08 · CAN_ESI 0x10 |
| 17 | `dlc` | u8 | code DLC tel que transmis sur le bus (0..15) |
| 18 | `len` | u8 | nombre d'octets de data (fonction de dlc et flags, §8.2) |
| 19 | `bus` | u8 | 0 en v0 |
| 20 | `data` | u8[len] | octets de données de la trame, dans l'ordre du bus |

Charge utile définie : 20 à 84 octets.

#### `0x12` SIM_TIME — PC → carte

Fait avancer le temps bus sans trame (déclenche les ticks).

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `t_us` | u64 | temps bus courant en µs (non décroissant dans la session) |

Charge utile définie : 8 octets.

#### `0x7F` HEARTBEAT — PC → carte

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `host_uptime_ms` | u32 | horloge monotone du PC, diagnostic seulement |

Charge utile définie : 4 octets.

#### `0x81` HELLO_ACK — carte → PC

Identité, compatibilité et défi d'intégrité du runtime.

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `reason` | u8 | 0 = réponse à HELLO, 1 = émis spontanément au boot |
| 1 | `proto` | u8 | version de protocole parlée par la carte (0) |
| 2 | `rt_major` | u8 | version du runtime (semver) |
| 3 | `rt_minor` | u8 |  |
| 4 | `rt_patch` | u8 |  |
| 5 | `abi_major` | u16 | ABI runtime ↔ code étudiant offerte par le runtime |
| 7 | `abi_minor` | u16 |  |
| 9 | `build_id` | u8[8] | identifiant de build du runtime (hash de commit tronqué) |
| 17 | `challenge` | u8[32] | SHA-256(nonce ‖ image runtime) ; nonce = 16 × 0x00 si reason = 1 |
| 49 | `uid` | u8[12] | UID du MCU (H753 : 0x1FF1E800, G0B1 : 0x1FFF7590 ; 3 mots little-endian) |
| 61 | `dev_id` | u16 | DBGMCU_IDCODE.DEV_ID (0x450 NUCLEO-H753ZI, 0x467 NUCLEO-G0B1RE) |
| 63 | `rev_id` | u16 | DBGMCU_IDCODE.REV_ID |
| 65 | `reset_flags` | u32 | registre des causes de reset capturé au boot (RCC_RSR sur H753, RCC_CSR sur G0B1) |
| 69 | `student_status` | u8 | 0 absent · 1 ok · 2 magic · 3 ABI · 4 bornes · 5 désactivé après faute |
| 70 | `student_abi_major` | u16 | ABI déclarée par l'en-tête étudiant (0 si absent) |
| 72 | `student_abi_minor` | u16 |  |
| 74 | `student_crc32` | u32 | CRC-32 de la zone étudiante (0 si absente) |
| 78 | `rx_window` | u8 | fenêtre de crédits W en VCAN_FRAME (≥ 1, 32 recommandé) |
| 79 | `fault_pending` | u8 | 1 si un FAULT_REPORT suit immédiatement |

Charge utile définie : 80 octets.

#### `0x82` SESSION_ACK — carte → PC

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `session_id` | u32 | session concernée (≠ 0) |
| 4 | `op` | u8 | 1 = réponse à SESSION_START, 2 = réponse à SESSION_STOP |
| 5 | `status` | u8 | 0 ok · 1 pas de code étudiant valide · 2 session inconnue |

Charge utile définie : 6 octets.

#### `0x83` FAULT_REPORT — carte → PC

Dernière faute enregistrée (envoyée après HELLO_ACK si fault_pending = 1).

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `cause` | u8 | 1 = faute processeur, 2 = watchdog |
| 1 | `hook` | u8 | 0 aucun · 1 student_init · 2 student_on_frame · 3 student_on_tick |
| 2 | `frame_seq` | u32 | trame en cours si hook = 2, sinon 0 |
| 6 | `cfsr` | u32 | SCB->CFSR |
| 10 | `hfsr` | u32 | SCB->HFSR |
| 14 | `mmfar` | u32 | SCB->MMFAR |
| 18 | `bfar` | u32 | SCB->BFAR |
| 22 | `pc` | u32 | PC empilé |
| 26 | `lr` | u32 | LR empilé |
| 30 | `xpsr` | u32 | xPSR empilé |

Charge utile définie : 34 octets.

#### `0x84` NACK — carte → PC

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `ref_seq` | u16 | seq de l'en-tête du paquet refusé |
| 2 | `ref_type` | u8 | type du paquet refusé |
| 3 | `code` | u8 | code d'erreur (§11) |

Charge utile définie : 4 octets.

#### `0x90` RESULT — carte → PC

Réponse à exactement une VCAN_FRAME acceptée.

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `frame_seq` | u32 | VCAN_FRAME traitée (≠ 0) |
| 4 | `status` | u8 | 0 = livrée au code étudiant · 1 = non livrée (code étudiant désactivé) |
| 5 | `flags` | u8 | 0x01 publications perdues (> 8) · 0x02 budget de temps dépassé |
| 6 | `cycles` | u32 | durée du hook en cycles DWT (0 si status = 1) |
| 10 | `n` | u8 | nombre d'entrées (≤ 8 ; 0 si status = 1) |
| 11 | `entries` | n × {slot u8, kind u8, value f32} | slot de la mission ; kind 0 = valeur publiée, 1 = rejet (value = +0.0) |

Charge utile définie : 11 à 59 octets.

#### `0x91` TICK_RESULT — carte → PC

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `t_us` | u64 | frontière de tick exécutée (multiple de tick_period_ms × 1000) |
| 8 | `status` | u8 | comme RESULT.status |
| 9 | `flags` | u8 | comme RESULT.flags |
| 10 | `cycles` | u32 | durée de student_on_tick |
| 14 | `n` | u8 | nombre d'entrées (≤ 8) |
| 15 | `entries` | n × {slot u8, kind u8, value f32} | slot de la mission ; kind 0 = valeur publiée, 1 = rejet (value = +0.0) |

Charge utile définie : 15 à 63 octets.

#### `0x92` STUDENT_LOG — carte → PC

Texte de n7rt_log, UTF-8 valide sans octet nul, 20 lignes/s max.

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `t_us` | u64 | temps bus à l'appel de n7rt_log |
| 8 | `text_len` | u8 | longueur du texte en octets (≤ 120) |
| 9 | `text` | u8[text_len] | UTF-8 valide, sans octet nul |

Charge utile définie : 9 à 129 octets.

#### `0xFF` HEARTBEAT — carte → PC

Émis par interruption timer toutes les 250 ms, même pendant un hook.

| Offset | Champ | Type | Description |
|---:|---|---|---|
| 0 | `uptime_ms` | u32 | temps depuis le boot |
| 4 | `acked_frame_seq` | u32 | plus grand frame_seq déjà répondu (RESULT ou NACK), 0 si aucun |
| 8 | `hook_running_ms` | u16 | durée du hook en cours, 0 si aucun (saturé à 65535) |
| 10 | `state` | u8 | 0 repos · 1 session exploration · 2 session qualification |
| 11 | `student_disabled` | u8 | 1 si le code étudiant est désactivé après une faute |
| 12 | `rx_frames` | u32 | VCAN_FRAME reçues depuis le boot |
| 16 | `rx_seq_gaps` | u16 | trous de seq constatés côté carte |
| 18 | `rx_framing_errors` | u16 | E_OVERFLOW + E_COBS + E_SHORT |
| 20 | `rx_crc_errors` | u16 | E_CRC |
| 22 | `rx_overruns` | u16 | débordements UART/DMA |
| 24 | `log_dropped` | u16 | lignes STUDENT_LOG perdues (limite de débit) |
| 26 | `queue_high_water` | u8 | occupation maximale de la file de VCAN_FRAME |
| 27 | `max_cycles` | u32 | plus longue exécution de hook observée |

Charge utile définie : 31 octets.
<!-- END GENERATED: layouts -->

### 7.3 Règles de validité (`E_PAYLOAD`)

En plus des tailles minimales du §7.2 :

| Message | Règle |
|---|---|
| HELLO | `proto_min ≤ proto_max` |
| SESSION_START | `session_id ≠ 0` ; `mode ∈ {1, 2}` ; `tick_period_ms ≤ 1000` |
| SESSION_STOP | `session_id ≠ 0` |
| VCAN_FRAME | règles du §8.2 |
| HELLO_ACK | `reason ≤ 1` ; `student_status ≤ 5` ; `fault_pending ≤ 1` ; `rx_window ≥ 1` |
| SESSION_ACK | `session_id ≠ 0` ; `op ∈ {1, 2}` ; `status ≤ 2` |
| FAULT_REPORT | `cause ∈ {1, 2}` ; `hook ≤ 3` ; `frame_seq ≠ 0` si et seulement si `hook = 2` |
| RESULT | `frame_seq ≠ 0`, plus les règles TICK_RESULT ci-dessous |
| RESULT / TICK_RESULT | `status ≤ 1` ; seuls les bits `0x01` et `0x02` de `flags` peuvent valoir 1 ; `n ≤ 8` ; si `status = 1` alors `n = 0` et `cycles = 0` ; `kind ≤ 1` ; si `kind = 1`, `value` = +0.0 (4 octets nuls) |
| STUDENT_LOG | `text_len ≤ 120` ; texte UTF-8 valide sans octet nul |
| HEARTBEAT (carte) | `state ≤ 2` ; `student_disabled ≤ 1` |
| NACK, HEARTBEAT (PC), SIM_TIME, RESET_REQ | aucune règle supplémentaire. Un `code` de NACK inconnu est accepté et journalisé |

`RESULT.status = 1` (non livrée) n'est émis qu'une fois l'isolation I8 en place. Un runtime V1 désactive le code étudiant en passant par un reset (§9.6).

---

## 8. VirtualCanFrame (`VCAN_FRAME`)

### 8.1 Définition
Une `VCAN_FRAME` est **une trame CAN que le nœud étudiant a reçue**, telle qu'un contrôleur FDCAN la remettrait au logiciel :
- elle est horodatée en temps bus ;
- elle n'est jamais une trame d'erreur : les trames d'erreur ne sont pas transportées, comme avec un vrai contrôleur ;
- elle porte le **DLC brut** et la longueur de données qui en découle.

Le runtime la remet telle quelle au code étudiant sous forme de `n7rt_can_frame_t`.

### 8.2 Contraintes (sinon `E_PAYLOAD`)
| Champ | Règle |
|---|---|
| `frame_seq` | ≠ 0 |
| `flags` | bits 5 à 7 nuls ; `CAN_RTR` et `CAN_FD` exclusifs ; `CAN_BRS` et `CAN_ESI` exigent `CAN_FD` |
| `can_id` | ≤ `0x7FF` sans `CAN_EXT` ; ≤ `0x1FFFFFFF` avec `CAN_EXT` |
| `dlc` | ≤ 15 |
| `len` | `0` si `CAN_RTR` ; sinon `min(dlc, 8)` en CAN classique ; table FD `0–8, 12, 16, 20, 24, 32, 48, 64` (index = DLC) avec `CAN_FD` |
| `bus` | `0` en v0 |
| `data` | exactement `len` octets |

Ces règles sont celles de `CanFrame.__post_init__` dans `pc/dashboard/can_model.py`, le dashboard de banc, qui n'est pas dans le dépôt étudiant. Un test vérifie que les tables DLC et les bornes d'ID sont identiques ; il est ignoré quand ce module est absent.

### 8.3 Correspondances (une seule définition de trame)

| `VCAN_FRAME` | `n7rt_can_frame_t` (C, `n7rt.h`) | `CanFrame` (Python, dashboard) | FDCAN H7 (futur CAN physique) |
|---|---|---|---|
| `t_us` | `timestamp_us` | `timestamp` (µs → s) | horodatage RX étendu par le runtime en µs depuis le début de session |
| `can_id` | `id` | `can_id` | `Identifier` |
| `flags & CAN_EXT` | `flags & N7RT_CAN_EXT` (0x01) | `ext` | `IdType = FDCAN_EXTENDED_ID` |
| `flags & CAN_RTR` | `N7RT_CAN_RTR` (0x02) | `rtr` | `RxFrameType = FDCAN_REMOTE_FRAME` |
| `flags & CAN_FD` | `N7RT_CAN_FD` (0x04) | `fd` | `FDFormat = FDCAN_FD_CAN` |
| `flags & CAN_BRS` | `N7RT_CAN_BRS` (0x08) | `brs` | `BitRateSwitch = FDCAN_BRS_ON` |
| `flags & CAN_ESI` | `N7RT_CAN_ESI` (0x10) | `esi` | `ErrorStateIndicator = FDCAN_ESI_PASSIVE` |
| `dlc` | `dlc` | `dlc` | `DataLength` (code DLC, décalé de 16 bits dans la HAL H7 1.11) |
| `len` | `len` | `len(payload)` | déduit du DLC |
| `bus` | `bus` | `bus` (nom logique) | instance FDCAN |
| `data` | `data[0..len-1]` | `payload` | tampon RX |
| — | — | `err` | **non transporté** : une trame d'erreur n'est jamais livrée |

`frame_seq` sert au transport (crédits, appariement avec `RESULT`). Le code étudiant ne le voit pas.

---

## 9. Séquencement, crédits, temps et sessions

### 9.1 `seq` (en-tête)
- Chaque émetteur numérote **tous** ses paquets.
  - Le compteur démarre à 0 à l'ouverture du port (PC) ou au boot (carte) et augmente de 1 modulo 65 536 à chaque paquet.
  - Une réémission (`HELLO`, `SESSION_START`) est un nouveau paquet, avec un nouveau `seq`.
- Le récepteur compte les trous (`rx_seq_gaps`), mais **ne rejette, ne réordonne et ne déduplique jamais** sur la base de `seq`. La liaison série préserve l'ordre.
- `NACK.ref_seq` reprend le `seq` du paquet refusé.

### 9.2 Trames, réponses et crédits
- **`frame_seq`** : il vaut 1 pour la première `VCAN_FRAME` d'une session, puis augmente de 1 à chaque trame envoyée dans la session. **Le PC ne retransmet jamais une `VCAN_FRAME`** : une trame perdue est perdue, comme lors d'un débordement de FIFO CAN.
- **Invariant de réponse** : toute `VCAN_FRAME` reçue sans `frame_error` reçoit **exactement une** réponse.
  - `RESULT(frame_seq)` si elle est livrée, ou non livrée quand le code étudiant est désactivé.
  - Sinon `NACK(ref_type = 0x10)`, avec n'importe quel code de rejet du §11. En pratique :
    - `E_PAYLOAD` : trame invalide ;
    - `E_STATE` : hors session ;
    - `E_CREDIT` : file pleine ;
    - `E_TIME` : temps décroissant.
  - Le `NACK` ne porte que `ref_seq` : le PC DOIT conserver la correspondance `seq → frame_seq` de ses trames en vol pour savoir quelle trame il acquitte.
- **Ordre des réponses** : les `RESULT` et `TICK_RESULT` sont émis dans l'ordre d'exécution des hooks.
- **`acked_frame_seq`** (dans `HEARTBEAT` de la carte) = le plus grand `frame_seq` de la session ayant déjà reçu sa réponse ; 0 si aucun.
- **Crédits**
  - `en_vol = dernier frame_seq envoyé − max(frame_seq acquittés via RESULT, NACK ou HEARTBEAT)`.
  - Le PC NE DOIT PAS envoyer de `VCAN_FRAME` si `en_vol ≥ W`, où `W` = `HELLO_ACK.rx_window`.
  - La carte DOIT pouvoir mettre en file `W` trames. Une trame reçue file pleine reçoit `NACK E_CREDIT`.
  - Une trame perdue par bruit (sans réponse) est rattrapée par `acked_frame_seq` au heartbeat suivant.
- **Exigences annexes** :
  - `W` DEVRAIT valoir 32 ;
  - l'anneau de réception DMA de la carte DOIT contenir au moins `W × 268` octets ;
  - `RESULT` et `TICK_RESULT` portent au plus 8 entrées ; au-delà, les publications sont perdues et le bit `0x01` de `flags` est levé.

### 9.3 Sessions (état de la carte)

| État de la carte | Reçu | Action et réponse |
|---|---|---|
| tout état | `HELLO` | `HELLO_ACK(reason = 0)` décrivant l'état courant, **sans changer d'état** |
| tout état | `RESET_REQ` | reset en ≤ 50 ms ; aucune réponse avant `HELLO_ACK(reason = 1)` |
| `IDLE` | `SESSION_START(id)` | si le code étudiant est valide : réinitialise la RAM étudiante, appelle `student_init()`, réactive un code étudiant désactivé, répond `SESSION_ACK(id, 1, 0)` et passe en `SESSION(id, mode)`. Sinon : `SESSION_ACK(id, 1, 1)` et reste en `IDLE` |
| `SESSION(id)` | `SESSION_START(id)` (même id) | `SESSION_ACK(id, 1, 0)` **sans réinitialiser** (réémission idempotente) |
| `SESSION(id)` | `SESSION_START(id')`, id' ≠ id | ferme la session, puis traite comme depuis `IDLE` |
| `SESSION(id)` | `SESSION_STOP(id)` | `SESSION_ACK(id, 2, 0)`, passe en `IDLE` |
| tout état | `SESSION_STOP(x)` sans session `x` | `SESSION_ACK(x, 2, 2)` |
| `IDLE` | `VCAN_FRAME`, `SIM_TIME` | `NACK E_STATE` |
| `SESSION` | `VCAN_FRAME`, `SIM_TIME` | §9.2 et §9.4 |

### 9.4 Temps bus et ticks
- `t_us` (dans `VCAN_FRAME` et `SIM_TIME`) DOIT être **non décroissant** dans une session ; l'égalité est permise. Sinon : `NACK E_TIME`, et la trame n'est pas livrée.
- Le temps bus de la carte vaut le dernier `t_us` accepté. C'est la valeur de `n7rt_now_us()`.
- **Règle des ticks**, avec `P = tick_period_ms × 1000` µs et `P > 0` : quand le temps bus avance à `t`, la carte exécute `student_on_tick(k·P)` pour **chaque** `k ≥ 1` tel que `k·P ≤ t` et pas encore exécuté.
  - Les ticks s'exécutent dans l'ordre croissant, **avant** de livrer la `VCAN_FRAME` horodatée `t`.
  - Chaque tick produit un `TICK_RESULT(t_us = k·P)`.
  - Cette règle rend l'exécution déterministe : c'est elle que reproduit la re-exécution côté serveur.
- **Borne de rattrapage** (amendement S1.2, 2026-10-04) : un message dont le `t_us` ferait franchir **plus de 1 000 frontières de tick** est refusé par `NACK E_TIME`. Le temps bus et les ticks restent alors inchangés.
  - Sans cette borne, un `t_us` démesuré (jusqu'à 2^64 − 1 µs) imposerait à la carte un nombre de ticks pratiquement infini, ce qui contredit l'exigence de robustesse : aucune entrée hôte ne doit bloquer la carte.
  - Un hôte conforme n'atteint jamais cette borne : il envoie un `SIM_TIME` à chaque tick.
  - L'amendement historique n'a modifié aucun fixture de conformité. Les tests d'état du runtime (`test_e_time`), dans le dépôt encadrant, vérifient l'acceptation à la borne et le refus au-delà de cette borne.

### 9.5 Démarrage de la carte
Au boot, la carte DOIT émettre, en moins de 500 ms :
1. un `0x00` ;
2. `HELLO_ACK(reason = 1)` ;
3. puis `FAULT_REPORT` si `fault_pending = 1`.

Elle est alors en `IDLE`.

### 9.6 Réception d'un `HELLO_ACK(reason = 1)` non sollicité
Ce message signifie que la carte a redémarré. Le PC DOIT alors :
- considérer la session comme terminée ;
- abandonner les crédits et les trames en vol ;
- refaire la poignée de main.

Il NE DOIT PAS relancer automatiquement une session si `fault_pending = 1` ou si `student_status = 5` : la relance exige une action de l'utilisateur.

---

## 10. États et délais côté PC

| État | Entrée | Sortie |
|---|---|---|
| `ABSENT` | aucun port compatible | port ouvert → envoi de `0x00` + `HELLO` → `HANDSHAKE` |
| `HANDSHAKE` | `HELLO` envoyé | `HELLO_ACK` compatible → `READY`, `NO_STUDENT` ou `STUDENT_DISABLED` selon `student_status` ; incompatible → `INCOMPATIBLE` ; 10 essais sans réponse → `ABSENT` |
| `READY` | runtime compatible, code étudiant valide | `SESSION_ACK` ok → `SESSION` |
| `INCOMPATIBLE` | version, ABI ou défi refusés | nouveau `HELLO_ACK(boot)` → réévaluation |
| `NO_STUDENT` | `student_status ∈ {0, 2, 3, 4}` | nouveau `HELLO_ACK(boot)` → réévaluation |
| `STUDENT_DISABLED` | `student_status = 5` | action de l'utilisateur → `SESSION_START` |
| `SESSION` | `SESSION_ACK` ok | `SESSION_STOP` → `READY` ; `HELLO_ACK(boot)` → `HANDSHAKE` (§9.6) |
| `STUDENT_BUSY` | `HEARTBEAT.hook_running_ms > 500` | `hook_running_ms` revenu à 0 → état précédent ; reset IWDG → §9.6 |
| `STALLED` | **aucun paquet valide depuis 750 ms** (cœur arrêté, typiquement par un point d'arrêt) | premier paquet valide → état précédent |
| `LOST` | port disparu (signalé par l'OS) | nouvelle tentative d'ouverture toutes les 500 ms → `ABSENT` |

Ce que le PC fait en `STALLED` :
- il DOIT suspendre **tous** ses envois : trames, `SIM_TIME`, `HEARTBEAT` ;
- à la reprise, il ne retransmet rien et recalcule les crédits.

| Délai | Valeur | Règle |
|---|---|---|
| Heartbeat carte | 250 ms ± 25 ms | Émis par interruption timer, y compris pendant un hook |
| Heartbeat PC | 250 ms ± 50 ms | Diagnostic seulement |
| `T_HELLO` | 200 ms | Intervalle entre deux `HELLO`, 10 essais au plus |
| `T_SESSION` | 500 ms | Attente de `SESSION_ACK`, 3 réémissions au plus, même `session_id` |
| `T_STALL` | 750 ms | Sans aucun paquet valide de la carte |
| `T_BUSY` | 500 ms | Seuil de `hook_running_ms` |
| `T_RESET` | 50 ms côté carte ; 1 500 ms côté PC | Délai du reset ; attente du `HELLO_ACK(boot)`, au-delà → `LOST` |
| `T_BOOT` | 500 ms | Du reset au `HELLO_ACK(reason = 1)` |
| Silence du PC | — | La carte **ne prend aucune initiative** sur le silence du PC ; elle se contente de compter |

---

## 11. Codes d'erreur

| Code | Valeur | Origine | Condition | Traitement |
|---|---|---|---|---|
| `E_OVERFLOW` | — | tramage | plus de 267 octets sans délimiteur | paquet ignoré, compté |
| `E_COBS` | — | tramage | encodage COBS invalide | paquet ignoré, compté |
| `E_SHORT` | — | tramage | moins de 10 octets décodés | paquet ignoré, compté |
| `E_CRC` | — | tramage | CRC faux | paquet ignoré, compté |
| `E_UNKNOWN_TYPE` | `0x01` | décodage | type inconnu, réservé ou du mauvais sens | `NACK` |
| `E_VERSION` | `0x02` | décodage | `proto` ≠ version négociée | `NACK` |
| `E_LENGTH` | `0x03` | décodage | `len` > 255 ou ≠ taille réelle | `NACK` |
| `E_PAYLOAD` | `0x04` | décodage | contenu invalide (§7.3, §8.2) | `NACK` |
| `E_STATE` | `0x10` | session | message interdit dans l'état courant (§9.3) | `NACK`, trame non livrée |
| `E_CREDIT` | `0x11` | session | file de trames pleine | `NACK`, trame perdue |
| `E_TIME` | `0x13` | session | `t_us` décroissant, ou franchissant plus de 1 000 frontières de tick (§9.4) | `NACK`, trame non livrée |

Les codes d'erreur de session dépendent de l'état de la carte : ils sont spécifiés ici, mais ne figurent pas dans les vecteurs sans état (§14).

---

## 12. Confidentialité

Le protocole ne transporte que :
- des trames de bus, c'est-à-dire ce que le nœud étudiant reçoit de toute façon ;
- les sorties du code étudiant ;
- l'identité et l'état du runtime ;
- des diagnostics.

**Aucun message ne porte** de mapping de signaux, de DBC, de seed, de valeur attendue ni de paramètre de mission. Les numéros de slot sont publics : ils viennent de l'énoncé.

`nonce` et `challenge` ne sont pas des secrets. Ils prouvent seulement que la carte a lu l'image runtime au moment de la demande.

Les tests publics construisent des données synthétiques dans le vocabulaire du protocole, sans rôle ou valeur de mission. Les fixtures de réponse exacte, y compris les ancres normatives CRC et COBS, ne sont pas livrés dans le dépôt ou le build étudiant.

---

## 13. Débit à 921 600 Bd (charge V1)

Hypothèses, prises dans `docs/can-lab-architecture.md` §6 et toutes au pire cas :
- plafond des missions V1 : 600 trames/s sur le bus ;
- trames CAN classiques de 8 octets ;
- 8 publications par `RESULT` et par `TICK_RESULT` ;
- tick toutes les 10 ms ;
- 20 lignes de log/s de 120 octets ;
- heartbeats à 4 Hz.

Les tailles sur le fil sont les **bornes maximales** COBS + délimiteur : `n + ⌈n / 254⌉ + 1` pour un paquet de `n` octets. Le tableau ci-dessous est calculé par l'outil public `protocol_docs.py` ; `test_921600_baud_covers_v1_load_with_margin` exige une occupation ≤ 70 % dans chaque sens. Ce calcul utilise les formats et débits normatifs, sans jeu de réponses attendues.

<!-- BEGIN GENERATED: budget -->
Capacité de la ligne : 921600 Bd × 8N1 = 92160 octets/s par sens.

**PC → carte**

| Message | Débit (/s) | Octets sur le fil (max) | Octets/s |
|---|---:|---:|---:|
| VCAN_FRAME (CAN classique, 8 o) | 600 | 40 | 24000 |
| SIM_TIME (tick 10 ms) | 100 | 20 | 2000 |
| HEARTBEAT | 4 | 16 | 64 |
| **Total** | | | **26064** |

Occupation : **28.3%** · marge : **71.7%** · trames/s maximales dans ce sens avec la même charge annexe : **2252**.

**carte → PC**

| Message | Débit (/s) | Octets sur le fil (max) | Octets/s |
|---|---:|---:|---:|
| RESULT (8 publications) | 600 | 71 | 42600 |
| TICK_RESULT (8 publications) | 100 | 75 | 7500 |
| STUDENT_LOG (120 o) | 20 | 141 | 2820 |
| HEARTBEAT | 4 | 43 | 172 |
| **Total** | | | **53092** |

Occupation : **57.6%** · marge : **42.4%** · trames/s maximales dans ce sens avec la même charge annexe : **1150**.

Débit de trames maximal tenu dans les deux sens : **1150 trames/s**, soit 1.92 × le plafond V1 de 600 trames/s.
<!-- END GENERATED: budget -->

Ce que ce calcul ne couvre pas :
- la latence et la mise en paquets USB du port série virtuel ;
- les tampons du firmware ST-LINK ;
- l'erreur d'horloge UART.

Le débit réel doit être mesuré sur carte, sans perte sur 10 minutes, dans la tâche S1.2 et au budget du §14 de l'architecture.

---

## 14. Propriétés publiques et conformité exacte encadrant

Les tests publics construisent leurs entrées pendant l'exécution et vérifient :
- aller-retour des messages et des extensions dans chaque sens, avec conservation des champs ;
- encodage COBS sans délimiteur interne, respect des limites et décodage des données d'origine ;
- même réception quelle que soit la fragmentation du flux ;
- rebouclage des séquences et respect des bornes CAN, des longueurs et des textes ;
- rejet des versions, directions, types, longueurs, CRC et charges utiles invalides ;
- reprise sur un paquet valide après corruption, troncature ou débordement.

Commandes publiques : `python -m pytest tools/n7rt_link/tests` et `python tools/n7rt_link/protocol_docs.py check-doc`. Les commandes `layouts`, `budget` et `sync-doc` de cet outil recalculent les tables à partir du contrat, sans charger de fixture privé.

L'aller-retour seul peut laisser passer une erreur commune à l'encodeur et au décodeur. Les encadrants exécutent donc aussi la validation exacte CRC, COBS, paquets et flux sur les implémentations Python, Rust et C, depuis `tools/conformance/n7rt_link/` du dépôt restreint. Ses fixtures et réponses ne sont ni copiés dans le dépôt étudiant ni incorporés à son build. La procédure et les commandes exactes sont documentées dans ce dépôt encadrant. Le parcours étudiant n'a besoin d'aucun accès à celui-ci.

---

## 15. Évolution

- v0 est figée. Ajouter des propriétés ou des fixtures encadrant compatibles avec le comportement existant est permis.
- Les changements suivants imposent `proto = 1` et une nouvelle version des tests de conformité :
  - modifier le comportement normatif d'encodage ou de réception ;
  - modifier une règle DOIT ;
  - ajouter un message que v0 devrait comprendre.
- Les ajouts déjà prévus sont `VCAN_STATUS` (I4), `VCAN_TX` (V2) et un débit de 2 000 000 Bd. Ce sont des **extensions** à négocier par `HELLO` ; ils ne modifient pas v0.

**Amendements de v0**

| Date | Section | Changement | Raison |
|---|---|---|---|
| 2026-10-04 | §9.4, §11 | Une avance de temps franchissant plus de 1 000 frontières de tick donne `E_TIME` | Contradiction démontrée en S1.2 entre la règle des ticks sans borne et l'exigence « aucune entrée hôte ne bloque la carte ». Aucun vecteur modifié ; un hôte conforme n'est pas concerné |
