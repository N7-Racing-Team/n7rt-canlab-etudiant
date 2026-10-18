# SDK étudiant N7RT v0 (S1.3) : runtime précompilé + projet STM32CubeIDE

Le SDK permet à un étudiant de construire, dans STM32CubeIDE 2.0.0, une **image étudiante** liée au runtime N7RT déjà validé en S1.2 ([n7rt-runtime-v0.md](n7rt-runtime-v0.md)). L'étudiant ne voit et ne modifie que `student/student_decoder.c/.h`.

- **Les octets du runtime distribué sont ceux de l'image S1.2 validée.** C'est la région `0x0800 0000…+20 312` de l'image qui a passé le smoke 19/19, la sonde à 1 000/s et le soak 10 min ; ils ne sont jamais recompilés.
- **Épinglage.** Le fichier `firmware/n7rt_student/runtime-release.json` fixe la version, le `build_id`, la taille, le SHA-256 et les preuves de validation. `stm32lab sdk` refuse toute autre image.

## Artefacts distribuables

Le projet modèle est `firmware/n7rt_student/N7RT_CANLab_Student/` : 13 fichiers, sans sortie de build. L'étudiant le reçoit dans l'archive étudiante (dossier `N7RT_CANLab_Student/`, voir [distribution.md](distribution.md)). `stm32lab sdk` le régénère et en fait aussi une archive seule, `firmware/n7rt_student/dist/N7RT_CANLab_Student_sdk0.1.0_rt0.1.0.zip` (procédure encadrant, plus bas).

```text
N7RT_CANLab_Student/
  .project .cproject              projet CubeIDE 2.0.0, configuration unique "Debug"
  N7RT_CANLab_Student.launch      Run/Debug ST-LINK, connect-under-reset, sans numéro de série, arrêt sur student_init
  LISEZMOI_ETUDIANT.txt           procédure étudiant (import, modifier, build, flash, tester)
  student/student_decoder.c       <- modifiable : exemple minimal = test d'intégration du banc
  student/student_decoder.h       <- modifiable
  n7rt/                           SDK généré, vérifié, à ne pas modifier
    include/n7rt.h                API publique (celle du runtime) + contrôle d'ABI à l'édition de liens
    lib/n7rt_runtime.o            runtime S1.2 en binaire (.incbin), section .n7rt_runtime ; seuls symboles : étiquettes
    lib/libn7rt_glue.a            glue S1.2 (en-tête étudiant + appels via la table d'API), sans debug, étiquetée ABI 1
    ld/N7RT_H753_student.ld       script de liaison généré (constantes de la release)
    manifest.json                 versions, ABI, carte mémoire, SHA-256 de chaque fichier
    LISEZMOI.txt
```

**Ce qui n'est pas dans le SDK** :
- aucune source du runtime : ni startup, ni transport, ni N7RT-Link, ni session, ni watchdog ;
- aucun symbole interne : le binaire runtime n'exporte que ses étiquettes, et la glue n'exporte que l'en-tête et les quatre services ;
- aucun contenu de mission : `check_sdk` le vérifie par un balayage de mots entiers (DBC, mapping, seed, secret, réponse…) sur les textes du projet et les chaînes du binaire.

## ABI exposée (v1.0)

| Élément | Valeur |
|---|---|
| Hooks étudiants | `student_init()`, `student_on_frame(const n7rt_can_frame_t *)`, `student_on_tick(uint64_t now_us)` |
| Services | `n7rt_publish(slot, float)`, `n7rt_reject(slot)`, `n7rt_now_us()`, `n7rt_log(const char *)` |
| Trame | `n7rt_can_frame_t`, 80 octets figés (`timestamp_us`, `id`, `dlc`, `len`, `flags`, `bus`, `data[64]`) |
| En-tête étudiant | `0x0804 0000`, 64 octets, magic `N7ST`, ABI 1.0, fourni par la glue |
| Table d'API du runtime | `0x0800 0400` (dans l'image précompilée) |
| Point d'entrée de l'ELF | `n7rt_runtime_reset` = `Reset_Handler` du runtime, `0x0800 0461` |
| Étiquettes de compatibilité | `n7rt_runtime_v0_1_0`, `n7rt_runtime_abi_1` (runtime) ; `n7rt_glue_abi_1` (glue, référencé par `n7rt.h`) |

**Garde-fous du build.** Le build échoue, dans CubeIDE comme en ligne de commande, si les fichiers ne s'accordent pas. Chaque cas est couvert par un test :

| Incohérence | Ce qui casse |
|---|---|
| `n7rt.h` d'une autre ABI | référence non résolue à `n7rt_glue_abi_N` |
| Runtime d'une autre version | `ASSERT` « is not runtime 0.1.0 » |
| Runtime d'une autre taille | région `RT_FLASH` débordée, car elle mesure exactement 20 312 o |
| Linker script d'une autre release | `ASSERT` sur l'étiquette de version |
| En-tête étudiant absent ou déplacé | `ASSERT` |
| Code étudiant trop gros | région `ST_FLASH` débordée |
| Données étudiantes trop grosses | région `ST_RAM` débordée |
| Section étudiante forcée dans la zone runtime | région `RT_FLASH` débordée |

Le linker script ne déclare pas l'AXI SRAM du runtime : rien de l'image étudiante ne peut y être placé.

**Intégrité**, au-delà de la cohérence :
- `stm32lab sdk --check` compare chaque fichier au SHA-256 du manifeste et les octets du runtime à la release ;
- `build --student` vérifie dans l'image CubeIDE que la région runtime est identique octet pour octet ;
- sur la carte, le défi `HELLO` (SHA-256(nonce ‖ runtime)) prouve que le runtime flashé est intact.

Un binaire runtime retouché mais de même taille n'est **pas** détecté par l'éditeur de liens seul. Ce sont ces trois contrôles qui le détectent.

## Carte mémoire observée (build CubeIDE du modèle)

| Région | Origine | Taille | Utilisé | Contenu |
|---|---|---|---|---|
| `RT_FLASH` | `0x0800 0000` | 20 312 o | 20 312 o (100 %) | runtime précompilé, inchangé |
| (trou) | `0x0800 4F58` | 241 832 o | 0 | vide, vérifié |
| `ST_FLASH` | `0x0804 0000` | 768 Ko | 260 o | en-tête étudiant, code de l'exemple, glue |
| `ST_RAM` (DTCM) | `0x2000 0000` | 128 Ko | 8 Ko | tas réservé (l'exemple n'a aucune variable) |

Image CubeIDE : SHA-256 `a5979d31eec76422…`, CRC-32 de la zone étudiante `0x5CBC80B3` (le même CRC est rapporté par la carte dans `HELLO_ACK`). La section `.comment` de l'ELF montre deux compilateurs :
- GCC 13.3.1 (GNU Tools for STM32 13.3.rel1, celui de CubeIDE) pour le code étudiant et la libc ;
- GCC 14.3.1 pour la glue précompilée issue de S1.2.

## Procédure encadrant : régénérer le runtime distribué

Réservée aux encadrants. `build --runtime` et `sdk` construisent depuis `firmware/stm32_h753_bringup`, le projet de bring-up qui contient les sources du runtime : il n'est que dans le dépôt encadrant, qui décrit la mise en place. Dans le dépôt étudiant seul, seule la vérification `sdk --check` s'exécute.

```text
python tools/stm32/stm32lab.py build --runtime   # rebuild reproductible : doit redonner le SHA épinglé
python tools/stm32/stm32lab.py sdk               # extrait le runtime S1.2, régénère n7rt/, vérifie (12 contrôles), zip
python tools/stm32/stm32lab.py sdk --check       # vérification seule
```

- `sdk` refuse une image runtime différente de `runtime-release.json`. Il ne peut donc pas distribuer silencieusement un runtime non validé.
- La génération est déterministe : deux exécutions produisent les mêmes octets.
- **Publier un nouveau runtime** :
  1. le valider en S1.2 (smoke, sonde, soak) ;
  2. mettre à jour `runtime-release.json` (version, `build_id`, taille, SHA-256, preuves) ;
  3. relancer `sdk`.
  Le linker script, les étiquettes et le manifeste changent de version ensemble. Si l'ABI change, la majeure de `n7rt.h` change aussi, et tout projet étudiant ancien échoue à l'édition de liens au lieu de produire une image incohérente.

## Procédure étudiant minimale

Le même SDK est utilisé par la [page de code intégrée au CAN Lab](inapp-student-ide-tutorial.md). Elle sauvegarde les deux fichiers étudiants dans le profil local, compile avec GCC ARM et programme uniquement la zone étudiante avec contrôle du runtime. Les validations de ce parcours sont distinguées des résultats CubeIDE historiques dans [le rapport R1](inapp-student-ide-r1.md).

Le parcours principal est **Code STM32 → Enregistrer → Compiler → Flasher**, détaillé dans [le tutoriel](inapp-student-ide-tutorial.md). La programmation passe alors par les contrôles de zone étudiante et d'intégrité du runtime.

L'import et le débogage sous CubeIDE restent une voie alternative, accompagnée par l'encadrant. La procédure historique ci-dessous décrit cette voie ; le bouton **Run** de CubeIDE utilise son propre programmateur et ne passe pas par les garde-fous du flasher intégré :
1. STM32CubeIDE 2.0.0 > *File > Import > Existing Projects into Workspace > Select archive file* (le zip) ;
2. modifier `student/student_decoder.c` ;
3. **Build** : « 0 errors ». La console affiche `RT_FLASH`, `ST_FLASH` et `ST_RAM` ; un dépassement fait échouer l'édition de liens ;
4. pour programmer depuis le CAN Lab, reprendre les sources dans **Code STM32**, compiler et flasher ; **Run/Debug** dans CubeIDE est réservé au parcours de débogage accompagné, avec vérification de l'image et du runtime ;
5. tester avec le banc : `smoke-test <carte> --student` côté staff, puis l'application du CAN Lab plus tard.

## Validation du 2026-10-04 (carte C : ST-LINK `001E002D3234510433353533`, COM9)

Résultats historiques du checkpoint du 4 octobre, conservés dans leur portée d'origine. Le retrait ultérieur des fixtures publics ne rejoue pas cette campagne et ne change aucun de ces résultats. La conformité exacte est désormais exécutée depuis le dépôt encadrant ; le parcours étudiant courant ne charge ni fixture privé ni dépôt encadrant.

| Étape | Commande | Résultat |
|---|---|---|
| SDK généré et vérifié | `stm32lab sdk` | 12/12 PASS ; régénération identique à l'octet |
| Build réel CubeIDE 2.0.0 headless, clean | `stm32lab build --student` | 0 erreur, 0 avertissement ; contrôles image 6/6 ; deux clean builds → même SHA-256 |
| Archive décompressée hors du dépôt puis construite par CubeIDE | (procédure ci-dessus) | 0 erreur, image identique (`a5979d31…`) : aucune dépendance au build CMake ni au dépôt |
| Flash de l'image CubeIDE | `stm32lab flash C --student` | écrit + vérifié + reset, 2,1 s |
| Carte : handshake → `VCAN_FRAME` → `student_decoder` → `RESULT` | `stm32lab smoke-test C --student` | **PASS 10/10** (détail ci-dessous) |
| Tests de liaison et tests négatifs | `pytest tools/stm32/tests/test_n7rt_sdk.py` | 9/9, dont un build CubeIDE headless réel qui échoue sur dépassement de flash (`N7RT_RUN_CUBEIDE=1`) |
| Non-régression S1.2 | `smoke-test C --runtime` | 19/19 |
| Suite complète | `python -m pytest` | 98 passés, 1 ignoré (le build CubeIDE lourd, lancé à part) |

**Contrôles du smoke-test étudiant** :
- **Runtime et image** : le runtime est intact (défi SHA-256 au boot et avec un nonce aléatoire) ; l'image étudiante est chargée (statut 1, ABI 1.0, CRC égal à celui de l'image CubeIDE).
- **Hooks** : `student_init()` a écrit sa ligne de log, et 21 trames de 7 variantes CAN ont reçu un `RESULT` exact via `student_on_frame()`.
- **Conformité exacte historique rejouée sur la carte** : 48 cas PC → carte, paquets et flux de robustesse compris, chacun dans une session neuve, avec une réaction exacte de la carte (réponses, `NACK`, silence). Les compteurs CRC et tramage de la carte égalent les erreurs injectées (4 et 6). Ces fixtures sont conservés dans le dépôt encadrant ; aucun octet attendu n'est fourni dans ce document.
- **Encodeur de la carte** : 204 paquets émis par la carte, réencodés par le codec de référence, sont identiques à l'octet.
- **Lien** : propre.

## Limites et suite

- **Codec et toolchain.** Le codec N7RT-Link s'exécute dans le runtime précompilé (GCC 14.3). Par conception, CubeIDE ne le recompile pas. Les cas de conformité historique ont donc été repassés contre l'**image produite par CubeIDE sur la carte**, et non contre une recompilation du codec par GCC 13.3. Le retrait des fixtures publics ne change ni cette image épinglée ni son ABI.
- **Flash depuis l'interface CubeIDE.** *Run* n'a pas été exécuté dans l'interface graphique : l'image CubeIDE a été flashée avec STM32_Programmer_CLI. Le `.launch` fourni n'a pas encore été essayé sur la carte.
- **Plateformes.** Seul Windows 11 a été testé. L'import, le build et le Run sous macOS et Linux restent à faire (S1.4).
- **Manifeste.** Il n'est pas signé (Ed25519 prévu par l'architecture) : c'est une empreinte de cohérence, pas une preuve d'origine.
- **Tas étudiant.** 8 Ko sont réservés, mais le `_sbrk` de newlib (nosys) n'est pas borné à cette réserve. `malloc` est à éviter dans le code étudiant.
- **Mémoire de la machine.** Le build headless doit tourner avec un tas JVM réduit (`-Xmx512m`, variable `STM32LAB_CUBEIDE_VMARGS`) : la mémoire engagée libre était de 0,4 Go.
- **Étapes suivantes** : S1.4 (CubeIDE sous macOS et Linux, *Run* depuis l'IDE), signature du manifeste, isolation MPU/SVC du code étudiant (I8).
