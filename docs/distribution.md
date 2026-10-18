# Distribution étudiant / encadrant (contrat d'implémentation)

Statut : release 0.3.0 ; archive étudiant dans les Releases du dépôt étudiant public, outils encadrant dans les Releases du dépôt encadrant privé (§1.1). Les résultats et limites des quatre missions sont consignés dans [le rapport R1](missions-four-r1.md). Public : développeurs, encadrants et responsables. Ce document est publié dans le dépôt étudiant : aucun secret de mission ni détail opérationnel encadrant ici.

## 1. Deux dépôts, deux éditions

| Dépôt | Visibilité | Contenu | Jamais dedans |
|---|---|---|---|
| `N7-Racing-Team/n7rt-canlab-etudiant` (étudiant) | Public, sans compte GitHub (§1.1) | Application (code générique, substituts publics de l'oracle), SDK étudiant (runtime en binaire, gabarit), bibliothèque en liens (§1.4), schémas, énoncés publics, documentation publique, outils de banc et de release | Oracle, packs (de référence et de groupe), fichiers d'essai, vecteurs de mission et réponses des quatre missions, fixtures à réponses fixes de CRC, COBS, cryptographie ou circuit, décodeurs de référence, incidents, matrice, clé maîtresse, empreinte du code encadrant, source du runtime, documents PDF |
| Dépôt encadrant | Privé, réservé aux encadrants autorisés (§1.1) | `missions/<mission>/` (pack de référence, banc, vecteurs dorés, décodeur de référence), `oracle/n7rt-vehicle/` (sources de l'oracle), spécification véhicule, recherche, bibliothèque privée (manifestes et liens), source du runtime, outils (build encadrant, audit, tests privés, installation) | Secrets opérationnels (`instructor/`, données de `cohorts/` : hors Git), documents PDF, binaires des outils encadrant (`dist/`, `tools/bin/` : hors git) |

Le code du dépôt étudiant est générique : il simule le véhicule, chiffre et déchiffre les packs, mais ne contient ni valeur de mission ni oracle. Les valeurs vivent dans le dépôt encadrant, et chez l'étudiant seulement sous forme de pack de groupe chiffré (§3.1). L'oracle vit dans le dépôt encadrant et n'est compilé que dans l'édition encadrant (§1.3).

Le retrait demandé couvre aussi les vecteurs normatifs publics de CRC, COBS et cryptographie, ainsi que les fixtures de circuit à résultats fixes. Les formats, dispositions, constantes de protocole et lois physiques restent publics ; leurs tests publics vérifient des propriétés calculées, des aller-retours et des invariants. Les fixtures de conformité exacte et leurs vérificateurs restent dans le dépôt encadrant. Ce contrat ne réécrit pas les anciens commits, archives ou rapports de mesure ; les preuves du candidat après retrait doivent être acquises de nouveau.

### 1.1 Accès

Le dépôt étudiant est public : ses fichiers et ses releases se lisent et se téléchargent sans compte GitHub. Il ne contient que ce que l'audit du §5 autorise ; son historique public commence à la release 0.3.0, l'historique de développement antérieur reste privé.

Le dépôt encadrant est privé et réservé aux encadrants autorisés par le responsable. Ses droits sont contrôlés avant chaque distribution encadrant, en lecture seule, par l'outil de contrôle d'accès du dépôt encadrant. Les secrets opérationnels (empreinte du code encadrant, clé maîtresse, cohortes et codes des groupes) ne sont dans aucun dépôt : ils restent hors Git, dans un stockage réservé aux encadrants. Un étudiant n'a aucun droit sur le dépôt encadrant ni sur ce stockage. Ne jamais apporter ce stockage, ses jonctions ou les outils encadrant sur un poste étudiant.

### 1.2 Source de vérité après publication

L'historique versionné fait foi : celui du dépôt étudiant pour l'application et la distribution publique, celui du dépôt encadrant pour les ressources encadrant. Les tags, commits et SHA-256 des assets publiés identifient les releases livrées. Toute correction se fait dans les clones de ces dépôts et se soumet à leur revue ; un ancien espace de travail combiné non versionné ne doit pas les écraser.

L'espace `perso-ia/can-lab-control-center` et ses templates d'export servent encore de miroir de migration. Les scripts `export_student.py` et `export_instructor.py` sont des outils de migration à liste blanche : utiliser une cible neuve, relire son diff contre les sources versionnées et intégrer les changements explicitement. Ne pas exporter par-dessus un clone de développement ou une modification locale. Les scripts ne sont exécutables que depuis un espace combiné (`app/` et `restricted/` côte à côte), avec une cible hors de cet espace.

`instructor/` et les données de `cohorts/` sont un état opérationnel hors Git, conservé dans le stockage réservé aux encadrants. Le cycle de vie de cet état est distinct de celui du code ; seul `cohorts/README.md` se versionne.

### 1.3 Oracle privé, deux éditions

Décision du 2026-10-07 (critère d'acceptation : aucun oracle dans le dépôt ni dans le build étudiant).

- **Oracle** : supervision des messages côté consommateur (contrôle P22, compteur de vie, machine d'états E2E, RX_TIMEOUT, STALE, qualification), logique consommateur des quatre missions (qualification, plausibilité, persistance, coupure, reprise et valeurs retenues selon chaque mission), consommateur de référence (`RefConsumer`), comparaison d'un essai nominal (`compare`), jugement des fichiers d'essai (`judge_run`). Sources privées : `oracle/n7rt-vehicle/` du dépôt encadrant (`consumer.rs`, `consumer_wsf.rs`, `consumer_sas.rs`, `consumer_bps.rs`, `supervision.rs`, `e2e_check.rs`, `judge.rs`, `tests/`).
- **Compilation** : le crate `n7rt-vehicle` ne les compile que si la variable d'environnement `N7RT_ORACLE_DIR` donne, au moment du build, le chemin absolu de ce dossier (`build.rs`, cfg `n7rt_oracle`). Opt-in seulement : rien n'est cherché sur le disque ; un build du dépôt étudiant ne prend jamais un dépôt encadrant voisin.
- **Build public** (dépôt étudiant seul) : substituts de `app/crates/n7rt-vehicle/src/public/`. Le VCU de fond lit la course du premier canal capteur telle qu'elle est décodée sur le bus, ne coupe jamais et ne supervise rien ; l'onduleur ne passe jamais en timeout ; `RefConsumer::try_new` et `judge_run` répondent une erreur « oracle absent … », `compare` un verdict d'échec qui le dit. Le côté émetteur de l'E2E (CRC8H2F, protection P22) reste public : le véhicule simulé en a besoin.
- **Build encadrant** : le VCU de fond applique le consommateur de référence (spécification véhicule, ch. 01 §3, dépôt encadrant).

| Édition | Construite depuis | Verdicts |
|---|---|---|
| « édition publique » | le dépôt étudiant : release étudiant (§5.3) | aucun, nulle part |
| « édition encadrant » | le dépôt encadrant, oracle compilé : archive des outils encadrant (§5.3) | essai nominal, jugement des fichiers d'essai |

Les binaires de l'édition encadrant portent une marque d'édition (texte de la constante `EDITION` de `n7rt-vehicle` sous cfg `n7rt_oracle`) : le build de la release étudiant refuse un exécutable qui la porte, celui des outils encadrant un binaire qui ne la porte pas, et l'audit la cherche (§5.2). `n7rt-tool --version` affiche l'édition. Le binaire public ne contient ni valeur de mission ni oracle ; le binaire encadrant contient l'oracle et ne sort pas du cercle des encadrants.

### 1.4 Bibliothèque : liens seulement

Décision du 2026-10-07 (V-M03, licence des copies locales, encore ouverte). Aucun document PDF dans les deux dépôts ni dans l'archive étudiant. `library/` garde `README.md`, `links.json` et un `manifest.json` par dossier (`title`, `publisher`, `reference`, `revision`, `date`, `primary_url`, `sha256`, `missions`, `local_file`). L'application ouvre la copie locale si elle est présente dans `library/` à côté de l'exécutable et que son SHA-256 correspond, sinon l'adresse de l'éditeur (accès internet nécessaire). `/library/**/*.pdf` est ignoré par git : les copies locales ne sont jamais committées. Côté encadrant, les documents privés se récupèrent depuis `library-private/private-manifest.json` (`primary_url`, `sha256`) et `library-private/TO-FETCH.md`, ou depuis un partage privé ; `/library-private/**/*.pdf` est ignoré par git. Les copies locales ne reviendront dans la distribution étudiant qu'après clôture de V-M03, avec une preuve par éditeur.

## 2. Modes de l'application

Le candidat des quatre missions permet de choisir APPS, WSF, SAS ou BPS. Le choix étudiant reste verrouillé après redémarrage ; le mode encadrant authentifié permet de les ouvrir et de réinitialiser leur banc. Les preuves acquises et la recette NUCLEO restant à effectuer figurent dans [le rapport R1](missions-four-r1.md) et [le protocole de recette](missions-four-manual-acceptance.md). Les archives publiées le 8 octobre conservent leur périmètre APPS initial.

### 2.1 Mode étudiant (par défaut)

- **Choix de la mission** : confirmé (« Ce choix est définitif pour la session : pour en changer, demandez à votre encadrant. ») puis verrouillé dans l'interface : l'application refuse ensuite toute autre mission.
- **Choix persistant pour le profil étudiant** : le choix est enregistré dans `%APPDATA%\fr.n7rt.canlab\student\state.json`, verrouillé pour la session et conservé au redémarrage. La procédure de reset local ci-dessous ouvre une nouvelle session de choix ; cette persistance ne garantit pas une protection contre la modification des fichiers ou du système par leur propriétaire. L'encadrant remet à chaque groupe uniquement le pack et le code d'une mission ; un pack de groupe n'ouvre que la mission pour laquelle il a été généré.
- **Pack du groupe** : fichier `<GROUPE>.n7rtpack` (par exemple `G01.n7rtpack`) remis par l'encadrant, le code du groupe à part. Importé une fois : l'application en garde une copie, toujours chiffrée, `%APPDATA%\fr.n7rt.canlab\student\packs\<mission>-<GROUPE>.n7rtpack`. À chaque ouverture de la mission, l'étudiant saisit le **code du groupe** ; la clé n'existe qu'en mémoire.
- **Essai nominal** : « Enregistrer l'essai nominal » joue l'essai caché sur la carte et écrit `Documents\N7RT CAN Lab\essais\essai-<mission>-<GROUPE>-<aaaammjj-hhmmss>.n7rtrun` (heure UTC ; `Documents` peut être redirigé vers OneDrive). Pas de verdict dans l'application étudiante : l'étudiant transmet le fichier à son encadrant. L'essai décisif se fait en direct devant l'encadrant (§3.2).

**Réinitialiser le choix d'un étudiant.** Sur le poste de l'étudiant, fermer l'application, puis supprimer le dossier de l'état étudiant (choix et pack importé) :

```powershell
if ([string]::IsNullOrWhiteSpace($env:APPDATA) -or
    -not [System.IO.Path]::IsPathRooted($env:APPDATA)) { throw 'APPDATA absent ou non absolu' }
$canLabProfile = [System.IO.Path]::GetFullPath((Join-Path $env:APPDATA 'fr.n7rt.canlab'))
$studentState = [System.IO.Path]::GetFullPath((Join-Path $canLabProfile 'student'))
if ((Split-Path $studentState -Parent) -ne $canLabProfile -or
    (Split-Path $studentState -Leaf) -ne 'student') { throw 'Cible de reset invalide' }
if (Test-Path -LiteralPath $studentState) {
    $studentStateItem = Get-Item -LiteralPath $studentState -Force -ErrorAction Stop
    if ($studentStateItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
        throw 'Reset refuse : la cible est une jonction ou un lien'
    }
    Remove-Item -LiteralPath $studentState -Recurse -Force
}
```

L'étudiant réimporte ensuite son pack et saisit son code de groupe. Ne jamais apporter sur un poste étudiant le dépôt encadrant, les outils encadrant ni le code encadrant.

### 2.2 Mode encadrant

- **Entrée** : bouton « Mode encadrant ». L'application cherche le **dossier encadrant**, dans cet ordre : (1) la variable `N7RT_INSTRUCTOR_DIR` : définie et non vide, elle seule compte (si elle ne désigne pas un dossier, la recherche s'arrête là) ; (2) le chemin enregistré par l'installation encadrant, `%APPDATA%\fr.n7rt.canlab\instructor.json` (§5.4) ; (3) en remontant depuis le dossier de l'exécutable et depuis le dossier courant, le premier dossier qui contient `instructor\auth.json` ; (4) sinon, le premier sous-dossier `n7rt-canlab-missions` d'un dossier de ces mêmes chemins (dépôts côte à côte). Elle demande ensuite le **code encadrant**, vérifié (Argon2id) contre `instructor/auth.json` de ce dossier. Sans dossier encadrant, le mode est inaccessible. Le mode encadrant dure jusqu'à la déconnexion ou la fermeture de l'application (mémoire seulement).
- **Modèle de confiance** : le code encadrant authentifie l'auteur du `auth.json` trouvé, pas un encadrant. N'importe qui peut créer un dossier encadrant (`n7rt-tool set-code` dans un dossier vide) et entrer en mode encadrant : il n'y gagne rien (ni pack de référence, ni clé maîtresse, ni oracle dans l'édition publique). La vraie frontière est l'accès au dépôt encadrant et à l'archive des outils encadrant.
- **Possibilités du candidat courant** : ouvrir APPS, WSF, SAS ou BPS avec son pack de référence (bus brut, banc, « Vérifier l'essai nominal »), réinitialiser le banc courant, « Réinitialiser » (état étudiant de ce poste : choix et pack importé ; jamais sur un poste étudiant, §2.1), « Packs de groupe… » (packs d'une cohorte, comme `n7rt-tool gen-packs`), « Juger un essai… » (fichier d'essai d'un groupe).
- **Édition publique** : tout cela fonctionne, sauf le verdict de l'essai nominal et le jugement des fichiers d'essai, refusés avec « Verdict réservé à l'édition encadrant : cette version de l'application ne contient pas l'oracle (archive des outils encadrant, ou n7rt-tool judge de cette archive). ». La ligne de mode affiche « Mode encadrant — édition publique, sans verdict ».

## 3. Formats

### 3.1 Pack de groupe `.n7rtpack` (chiffré)

Définition exacte (documentation du module `app/crates/n7rt-seal/src/lib.rs`) :

- **Enveloppe** (JSON UTF-8, champs dans cet ordre) : `format` "n7rt-group-pack", `version` 1, `mission`, `cohort`, `group`, `pack_id` (32 caractères hexadécimaux minuscules), `kdf` {`alg` "argon2id", `m_kib`, `t`, `p`, `salt` (base64, 16 octets)}, `aead` {`alg` "aes-256-gcm", `nonce` (base64, 12 octets)}, `ciphertext` (base64, sortie AES-256-GCM suivie de son tag de 16 octets).
- **Données associées** : le JSON compact (sans espace, échappement de serde_json) de l'enveloppe sans `ciphertext`, clés exactement dans cet ordre : `{"format":…,"version":…,"mission":…,"cohort":…,"group":…,"pack_id":…,"kdf":{"alg":…,"m_kib":…,"t":…,"p":…,"salt":…},"aead":{"alg":…,"nonce":…}}`. Ce n'est pas le JSON canonique du §3.2 : les clés ne sont pas triées. Le lecteur la reconstruit à partir des valeurs lues : tout champ d'en-tête modifié fait échouer la vérification du tag.
- **Clé** = Argon2id v0x13 (code normalisé, salt, m_kib, t, p), 32 octets, avec les coûts écrits dans l'en-tête ; elle n'existe qu'en mémoire.
- **Code normalisé** : le code tapé, en majuscules, sans espaces ni tirets (tout caractère blanc, `-`, U+2010 à U+2015, U+2212). Code de groupe : 16 caractères base32 de Crockford en quatre blocs, `XXXX-XXXX-XXXX-XXXX` (80 bits).

**Contenu déchiffré** : le pack étudiant (`n7rt-student-pack`), projection de la variante du groupe pour la simulation. Il contient la matrice de la variante (identifiants, périodes, dispositions, échelles, listes de Data ID, codes de statut), les étiquettes `source` des signaux, les émetteurs des trames, les libellés des énumérations, les qualificatifs, le conditionnement de l'ECU source, le texte du banc avec ses paramètres cachés (identiques pour tous les groupes), les paramètres du scénario nominal, les ECU de fond (hors VCU), le véhicule et le monde, les paramètres du bus et de l'alimentation, la période de tick, la graine de gigue, et sous `background.vcu` des paramètres consommateur **neutres** (aucun timeout, machine d'états triviale, aucune implausibilité ; seuls les noms des sources sont réels) : aucun paramètre consommateur de l'oracle n'atteint un étudiant. **Absents** : l'oracle et ses paramètres (consommateur, slots attendus et leurs LSB, règles de comparaison), la provenance, les noms et unités des trames et des signaux, les vecteurs dorés, les variantes des autres groupes.

Le format complet du pack (`pack-v0.json`, côté encadrant, lu par `n7rt-vehicle`) accepte en plus deux champs optionnels : `bench` (texte du banc incorporé) et `jitter_seed`.

**Variante** (spécification véhicule, ch. 01 §2.8) : seed de groupe = HMAC-SHA256(clé maîtresse, lp(mission) ‖ lp(cohorte) ‖ lp(groupe)), lp = longueur en u32 gros-boutiste puis octets UTF-8. Varient : phases, positions dans les bits libres, ordre des octets, facteur et offset (chacun conservé avec une probabilité d'au moins ½), permutations des codes de statut, listes de Data ID (16 octets par trame P22, tirés sans remise), graine de gigue. Identifiant, période, DLC, type de protection, longueur et signe des signaux ne varient pas. La variante « V0 » (la référence) n'est générée que pour les essais encadrant (`test-pack`, §4).

**Risque résiduel accepté (architecture D7, réexaminé le 2026-10-07).** Un étudiant qui détient le pack et le code de son groupe peut déchiffrer le pack hors ligne : le format est documenté ci-dessus, la crate `n7rt-seal` est publique, l'application ne contient aucun secret. Aucune rétro-ingénierie de la mémoire n'est nécessaire. Il lit alors la correspondance étiquetée de **sa** variante et les paramètres du banc ; la matrice couvre toutes les trames du véhicule, y compris celles des ECU des missions WSF, SAS et BPS, avec leurs étiquettes `source`. Les champs invariants sont ceux de tous les groupes et de la référence (identifiants, périodes et DLC s'observent de toute façon sur le bus). Ce seul pack et son code ne donnent ni la variante d'un autre groupe ni l'oracle. La valeur pédagogique repose sur l'implémentation de la logique consommateur (supervision et plausibilité) sur la carte, absente des fichiers distribués à l'étudiant. Cette propriété du format ne rétablit pas la confidentialité des références historiquement exposées (§1.1).

**Pack de groupe 0.3 : progression de la mission (en développement).** Le clair scellé devient l'enveloppe `{"format": "n7rt-group-content", "version": 1, "pack": <pack étudiant v1>, "workflow": <n7rt-workflow v1>}`. La section `workflow` contient, résolus pour la variante du groupe :
- la vérité de la fiche nominale : trames, champs et statuts exigés, valeurs de supervision, décisions de conception ;
- pour chaque étape, un lot de vecteurs d'essai caché (trames et instants, format `n7rt-vec`) ;
- pour chaque vecteur, les sorties attendues du consommateur de référence et les règles de comparaison (format `n7rt-exp`).

L'édition encadrant calcule ces données à la génération du pack. L'application étudiante rejoue le vecteur vers la carte et compare les sorties avec un comparateur public, sans code d'oracle. Elle compare aussi le circuit de l'étudiant au banc de référence et la fiche à la vérité scellée.

Les étapes suivent la série stricte E0 essai nominal → E1, E2, E3 incidents → E4 qualification finale → E5 clôture :
- **Incidents (E1–E3).** Chaque étape scelle comment appliquer l'anomalie, la vérité de la partie d'incident de la fiche (cible, catégorie, état observé), la mesure clé et les contrôles de réparation. Le moteur de mission l'applique au banc ou au véhicule par des mécanismes génériques ; rien de sa nature ni de sa cible n'est affiché, journalisé en clair ou annulable. L'anomalie est activée dans la transaction qui valide l'étape précédente et retirée dans celle qui valide la sienne. Pendant l'étape, « Reproduire l'anomalie » (R-11, avec confirmation) la remet en place, par exemple après une réparation faite avant la mesure ; le circuit reste celui de l'étudiant. Une étape ne passe que si le diagnostic de la fiche est conforme et la réparation vérifiée (P-1 à P-4 et essai caché) ; le texte libre n'est jamais lu.
- **Mesure clé.** « Consigner la mesure » enregistre les conditions du banc (états d'interaction, temps simulé, multimètre) et une preuve scellée : les trames brutes des dernières secondes et le circuit. À la vérification, le moteur recalcule la valeur attendue et la même grandeur sans l'anomalie (pertinence).
- **Qualification (E4).** Une séquence cachée (démarrage, nominal, limites, supervision, défaut puis reprise) étiquette chaque point de comparaison d'une compétence CP-1 à CP-8 ; ses durées, ses événements et l'attribution des points sont dans la vérité privée du pack. PASS ⇔ huit compétences réussies ; l'étudiant voit huit lignes PASS ou FAIL, rien d'autre.
- **Clôture (E5).** La fiche finale est écrite dans `Documents/N7RT CAN Lab/fiches/<mission>-<groupe>-<date>/` : `fiche.json` canonique, `fiche.html` lisible (empreinte SHA-256 de `fiche.json` en pied de page) et `scellés/` (preuves des mesures, banc, copie scellée de la fiche). Pour chaque incident, la fiche porte la réparation enregistrée par l'application, jamais par l'étudiant : CRC-32 de la zone étudiante à l'activation et à la validation, modifications du circuit entre les deux (paramètres cachés exclus). Elle porte aussi le temps actif, les reproductions de l'anomalie et toutes les tentatives. Aucune vérité, aucun vecteur ni aucune sortie attendue n'y figure.

Le parcours étudiant de bout en bout est décrit dans le [tutoriel de la progression](N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf).

Un pack 0.3 porte environ 1 à 4 Mio : la borne des packs passe à 8 Mio, celle des fichiers d'essai. La progression d'un pack vit sous `<données de l'application>/student/progress/<mission>-<pack_id>/` : `progress.json` et `fiche.json` (intégrité HMAC-SHA-256 par la clé du pack, écriture atomique), `bench.sealed` (copie du banc de l'étudiant), `preuve-<rang>-<empreinte>.sealed` et `activation-<rang>.sealed` (le banc à l'arrivée de l'anomalie). Les fichiers `.sealed` sont chiffrés en AES-256-GCM avec une clé dérivée du contenu déchiffré du pack, jamais écrite sur disque : le banc contient les paramètres cachés des pièces de mission. Le fil porteur d'un défaut est enregistré sous forme d'étiquette dérivée de cette clé, jamais par son identifiant ; ce champ a la même forme quelle que soit l'anomalie.

Le Physical Lab offre un multimètre : V continu, moyenné sur 100 trames ; Ω sur un banc dont toutes les alimentations sont coupées. Ses lectures restent visibles en mode mission, où les tensions des nets et des broches sont masquées : c'est l'instrument de l'étudiant (amendement de physical-lab-v1 §5.9 : la résistance hors tension n'est exposée que par le multimètre, banc coupé).

Compatibilité : une application 0.2 refuse un pack 0.3 (« Pack du groupe illisible »). L'application 0.3 ouvre un pack 0.2 en mode historique, avec un fichier d'essai et sans fiche.

**Risque résiduel étendu (D7, 0.3).** Sans serveur, la vérité d'un groupe vit dans son pack. Un groupe qui déchiffre son pack hors ligne y lit aussi les réponses attendues de sa fiche et les sorties attendues de ses essais cachés. Les verdicts affichés dans l'application pilotent la progression ; à eux seuls, ils ne prouvent rien pour la notation. La preuve reste :
- le fichier d'essai de l'essai nominal, re-jugé par l'outil encadrant avec le consommateur de référence ;
- l'essai décisif en direct devant l'encadrant (§3.2).

La fenêtre de 48 h et le gel prévus par la spécification ne sont pas appliqués : l'horloge du poste appartient à son propriétaire. Les intervalles entre deux vérifications d'une étape, 120 s et 300 s pour la qualification, reposent sur cette même horloge. De même, le dossier de progression appartient au poste : un fichier de progression supprimé ou altéré est refusé (« Progression illisible », réinitialisation par l'encadrant), jamais remplacé par sa version précédente, et une seule fenêtre de l'application écrit une progression ; mais restaurer une copie complète et antérieure du dossier remet l'état de cette copie, ce qu'aucun contrôle hors ligne ne peut détecter.

### 3.2 Fichier d'essai `.n7rtrun`

JSON UTF-8 canonique, non compressé : sans espace, clés triées par octets, entiers en décimal, aucun flottant JSON. Champs : `format` "n7rt-run", `version` 1, `mission`, `cohort`, `group`, `pack_id`, `app_version`, carte (`board_uid`, `runtime_build`, `student_crc32`), graines (`seed_vehicle`, `seed_scenario`), `duration_us`, `tick_us`, `started_utc`, `frames` (trames livrées : `t_us`, `id`, `flags`, `data` en hexadécimal minuscule), `ticks` (ticks de la carte : `t_us`, `slots` ; chaque slot vaut `null` s'il n'est pas écrit, `"reject"` ou `{"v": "<décimal>"}`, où `<décimal>` est la plus courte chaîne décimale qui relit le même f64, ou "NaN", "inf", "-inf"), et `integrity` = SHA-256 (hexadécimal minuscule) du JSON canonique du document sans ce champ.

**Intégrité, pas authenticité.** L'empreinte détecte une altération accidentelle ; tout le monde peut la recalculer, elle ne prouve pas l'origine du fichier (pas de serveur). Un étudiant qui aurait implémenté la logique ailleurs (sur PC, sans carte) pourrait fabriquer un fichier d'essai qui réussit. Parade : l'essai décisif est enregistré en direct devant l'encadrant, sur la carte du groupe, puis jugé (§5.4) ; le fichier enregistre l'UID de la carte, le build du runtime et le CRC-32 du code étudiant pour les recoupements (`n7rt-tool inspect` les affiche). Un verdict obtenu sur un fichier remis à distance ne prouve pas que le décodeur du groupe fonctionne.

### 3.3 Dossier encadrant

Dossier opérationnel encadrant, présent uniquement sur un poste encadrant ; `instructor/` et données de `cohorts/` restent hors Git :

- `instructor/auth.json` : `{ "format": "n7rt-instructor-auth", "version": 1, "code_phc": "$argon2id$..." }` (empreinte du code encadrant normalisé ; 12 caractères au moins, espaces et tirets non comptés).
- `instructor/master.key` : 32 octets aléatoires (64 caractères hexadécimaux), source des seeds de groupe ; créée par `n7rt-tool set-code` si elle manque, jamais remplacée.
- `missions/<mission>/` : `pack-v0.json`, `bench-v0.json`, `golden/`, `solution/`, `tools/`, `README.md` ; `reports/` (rapports locaux de l'essai nominal encadrant, ignorés par git).
- `oracle/n7rt-vehicle/` : sources de l'oracle (§1.3).
- `cohorts/<cohorte>/<mission>/` : `groups.json` (groupe, pack_id, code), `packs/<groupe>.n7rtpack`, `judged/<essai>.json`. Le chemin de la cohorte est une jonction ignorée vers le coffre protégé, préparée avant génération ; toutes ces données opérationnelles restent hors Git et se sauvegardent dans le stockage encadrant.
- Autres dossiers : `tools/` (dont `tools/bin/`, outils encadrant décompressés, ignoré par git), `vehicle-spec/`, `research/`, `library-private/`, `firmware/` (source du runtime, banc CAN physique), `docs/`.

## 4. Outil console `n7rt-tool`

- `n7rt-tool set-code <dossier encadrant>` : définit ou change le code encadrant (saisie masquée, deux fois ; 12 caractères au moins, espaces et tirets non comptés ; le code actuel est demandé d'abord s'il existe). Crée `instructor/master.key` s'il manque, sans jamais le remplacer.
- `verify-code <dossier>` : vérifie le code encadrant.
- `gen-packs <dossier> --mission apps --cohort <c> --groups G01,G02,… [--replace]` : variante, code et pack chiffré de chaque groupe (`cohorts/<c>/<m>/packs/<groupe>.n7rtpack`, `cohorts/<c>/<m>/groups.json`) ; `--replace` régénère des groupes existants (nouveau code, nouveau pack à redistribuer).
- `test-pack <dossier> --mission apps --group TEST [--cohort essais] [--replace]` : pack de la variante V0 (référence), pour un essai encadrant avec le décodeur de référence.
- `judge <dossier> <fichier .n7rtrun>` : verdict et détail privé, écrit dans `cohorts/<c>/<m>/judged/`.
- `inspect <fichier .n7rtpack | .n7rtrun>` : en-tête d'un pack ou résumé d'un essai (aucun code demandé).
- `--version` : version et édition ; `--help` : aide.

Le code encadrant est demandé (saisie masquée) par les commandes qui lisent le dossier encadrant ; `--code-stdin` le lit sur l'entrée standard pour l'automatisation, jamais sur la ligne de commande. Codes de sortie : 0 succès, 1 code incorrect (`verify-code`) ou essai non réussi (`judge`), 2 erreur.

Éditions (§1.3) : construit depuis le dépôt étudiant seul (`cargo build --release -p n7rt-tool` dans `app/`), `n7rt-tool` est en édition publique : toutes les commandes sauf `judge`, refusé avant toute saisie (code 2 : « cet n7rt-tool ne contient pas l'oracle … »). Pour juger : le `n7rt-tool.exe` de l'archive des outils encadrant (§5.3), ou l'édition encadrant de l'application. La release étudiant ne contient pas `n7rt-tool`. Source : `app/crates/n7rt-tool`.

## 5. Releases reproductibles, audit, installation, tests

Commandes PowerShell, depuis la racine du dépôt indiqué. Les deux dépôts sont clonés côte à côte (`n7rt-canlab` et `n7rt-canlab-missions` dans le même dossier). Leur `.gitattributes` conserve les octets des JSON épinglés et des binaires sans normalisation, impose LF aux scripts `.sh`, `.py` et `.ps1`, et CRLF aux lanceurs `.cmd` et `.bat`, quel que soit `core.autocrlf`. Ces règles explicites protègent les empreintes et les scripts Windows.

### 5.1 Exports des deux dépôts (mainteneur)

Migration depuis un espace combiné (§1.2), vers des cibles neuves de revue ; l'encadrant d'abord (l'export étudiant compare ses fichiers à l'arbre encadrant) :

```powershell
python restricted/tools/export_instructor.py <cible neuve encadrant>
python tools/release/export_student.py <cible neuve étudiant> --instructor <cible neuve encadrant>
```

Copie par **liste blanche** (chemins listés dans les scripts, moins `target/`, `node_modules/`, `dist/`, `build/`, `Debug/`, journaux et fichiers locaux) ; `--dry-run` prépare la sélection sans écrire et `--list` affiche les fichiers. Utiliser des cibles neuves pour la migration. À l'écriture, les deux scripts refusent toute cible qui est un dépôt Git, se trouve dans un arbre Git ou contient un sous-dépôt, même avec `--force` ; ce dernier n'autorise que d'autres cibles non versionnées non vides. Un export précédent non versionné peut être reconstruit. Chaque export vide la cible sauf les caches de build (étudiant : `app/target`, `app/node_modules`, `app/ui/node_modules`, `app/src-tauri/gen` ; encadrant : `build/`). L'export encadrant refuse en plus une cible contenant `instructor/` ou des données de cohorte autres que `cohorts/README.md`, et ne copie aucun secret ou état de cohorte depuis l'ancien espace combiné. Il écrit seulement le README des cohortes depuis son template ; les secrets opérationnels actuels restent dans le coffre local décrit au §1.1.

L'export étudiant conserve aussi son `dist/` existant : ses archives restent des résultats de builds antérieurs et ne sont pas mises à jour par l'export. Vérifier leur manifeste ou reconstruire avant publication. L'export encadrant efface toujours `dist/` et `tools/bin/` ; après cet export, redécompresser l'archive des outils encadrant dans `tools/bin/`. Aucun dossier `dist/` n'est copié depuis la source combinée.

L'export écrit ensuite les fichiers racine depuis `tools/release/templates/` (`README.md`, `.gitignore`, `.gitattributes`, `pyproject.toml`) et `restricted/tools/templates/` (`README.md`, `.gitignore`, `.gitattributes`, `cohorts/README.md`). Le contenu exporté dépend de la source combinée ; cette source n'est plus prioritaire sur les dépôts publiés et un export de cet ancien espace ne produit pas le candidat de clôture courant. Le template README étudiant du miroir historique porte cet avertissement et ne lie pas le rapport de clôture absent de ce miroir. Comparer la cible neuve aux sources versionnées, vérifier les fichiers manquants et les liens, relire (`git status`, `git diff`), auditer (§5.2), puis demander l'autorisation de commit. Aucun export ne vaut publication ni preuve de la campagne mémoire du candidat.

| Famille | Dépôt |
|---|---|
| `app/` (crates, `src-tauri`, `ui`, scripts, `tools/zig-shims`, manifestes et verrous), `firmware/n7rt_student/` (projet CubeIDE, runtime binaire, épinglage), `library/` (`README.md`, `links.json`, manifestes ; aucun PDF), `shared/` (schémas, protocoles, constantes, `missions/*/enonce.json`), documentation publique, `tools/stm32`, `tools/n7rt_link` (codec, documentation de disposition et budget, contrôles carte génériques), `tools/library`, `tools/release`, `pyproject.toml` | étudiant |
| `restricted/missions` → `missions/` (sans `reports/`), `restricted/oracle` → `oracle/`, `restricted/vehicle-spec`, `restricted/research`, `restricted/library` → `library-private/` (sans PDF), `restricted/tools` → `tools/`, `restricted/README.md` → `docs/zone-restreinte.md`, `firmware/stm32_h753_bringup` (source du runtime et du bring-up, sans `build/`, `third_party/`, `Board_*`), `firmware/stm32_node_*` et `arduino_helper_*` (banc CAN physique), tests hôte du runtime (`tools/n7rt_link/c_runtime.py`, `test_c_runtime.py`), `docs/apps-mission-slice.md`, `docs/hardware-wiring.md`, `docs/stm32-toolchain-windows.md`, `cohorts/README.md` | encadrant |
| `data/`, sorties de build, `hs_err*.log`, `*.egg-info`, `.git.disabled-*`, `.vscode/`, ancien échafaudage (`pc/`, `sim/`, `tools/dev`, `tools/setup`, `tools/stm32/bench`, documents de mars 2026), `tools/stm32/boards.local.json`, copies locales des PDF | aucun |

Refus de l'export étudiant (rien n'est écrit) : un fichier sous un dossier `restricted`, un `*.n7rtpack` ou `*.n7rtrun`, un `master.key` ou `auth.json`, un nom `pack-*.json`, `bench-*.json`, `golden*` ou `solution*`, ou un fichier dont le SHA-256 est celui d'un fichier de l'arbre encadrant. Les anciens fichiers publics de vecteurs et de cas dorés ne bénéficient plus d'une exception d'export.

### 5.2 Audit

Dans le dépôt encadrant, avant chaque push du dépôt étudiant et chaque publication de l'archive étudiant :

```powershell
python tools/audit_student.py ..\n7rt-canlab ..\n7rt-canlab\dist\n7rt-canlab-0.3.0-windows-x64.zip
```

Arguments : l'arbre étudiant, puis les archives à auditer, c'est-à-dire l'archive étudiant seulement. L'archive des outils encadrant contient l'oracle par construction : elle n'est pas auditée par cet outil étudiant et n'est publiée que dans les releases du dépôt encadrant privé. Sans archive (avant le build de la release), `python tools/audit_student.py ..\n7rt-canlab` audite l'arbre seul.

Fichiers lus : quand l'arbre étudiant est un dépôt git, exactement les fichiers que git suit ou ajouterait (`git ls-files --cached --others --exclude-standard`), c'est-à-dire ce qu'un push publie une fois committé ; sinon tous les fichiers, hors sorties de build et caches. L'audit lit les fichiers, pas l'historique git : chaque commit poussé doit avoir été audité (export, audit, commit ; puis build de la release, audit avec l'archive, push).

Le jeu de secrets est reconstruit à chaque exécution depuis l'arbre encadrant : identifiants des trames de chaque `missions/*/pack-*.json` (formes `0x7A5`, `0x07A5`, `7A5h`, `"id": 1957` pour un identifiant 0x7A5 fictif, et en binaire des tables d'identifiants consécutifs en u16/u32 little-endian), noms de trames et de signaux distinctifs, listes de Data ID (hex quelle que soit la séparation, compact, décimal, octets bruts), chaînes d'octets de `golden/`, identifiants distinctifs du décodeur de référence (hors API publique du SDK), textes propres aux sources de l'oracle (`oracle/`, hors tests) et marque de l'édition encadrant, SHA-256 de chaque fichier privé (fichier identique, empreinte écrite, empreinte brute du pack, du banc et du décodeur), `instructor/master.key`, l'empreinte de `auth.json`, les codes et `pack_id` des cohortes. Recherche : chaque fichier lu de l'arbre étudiant, chaque entrée des archives (archives imbriquées comprises), les chaînes imprimables ASCII et UTF-16LE (6 caractères ou plus) et les séquences d'octets secrets des exécutables, bibliothèques, objets, documents, images et polices, plus les règles de structure (pas de dossier `restricted`, pas de source du runtime, pas de nom interdit). Les médias sont aussi comparés aux empreintes des fichiers privés ; aucune extension de média ne bénéficie d'une exemption globale. L'audit ne fait pas d'OCR : relire visuellement les captures et documents ajoutés. Faux positifs relus : `tools/audit-allowlist.json`, une justification par entrée. Code de sortie 0 propre, 1 publication bloquée, 2 erreur d'usage. `--redact` masque les valeurs, `--json <fichier>` écrit le rapport complet.

Les chaînes d'octets trop régulières (rampes, octets répétés), présentes dans tout vecteur de test, ne sont pas cherchées comme octets ; l'audit le signale. Les variantes des groupes ne sont lisibles que dans les packs chiffrés.

### 5.3 Release étudiant et outils encadrant

**Candidat étudiant**, depuis le dépôt étudiant, avant toute autorisation de commit/push :

```powershell
python tools/release/build_student.py --build-date 2026-10-08 --jobs 1
```

Prérequis : Rust (Windows sans MSVC : chaîne GNU de `app/scripts/windows-gnu-env.ps1`, que le script reprend), Node 22.12 ou plus (24 pour reproduire la release), Python 3.11 ou plus. Étapes : contrôle de la chaîne d'outils, `npm ci`, `npm run build`, `cargo build --release --locked -p n7rt-can-lab --features tauri/custom-protocol` avec `N7RT_ORACLE_DIR` forcé à vide (édition publique ; un exécutable qui porte la marque de l'édition encadrant est refusé), `--remap-path-prefix=<dépôt>=.` et `--remap-path-prefix=<cargo home>=cargo` (via `CARGO_ENCODED_RUSTFLAGS`), `SOURCE_DATE_EPOCH` = date donnée, symboles retirés par le profil release.

Résultat : `dist/n7rt-canlab-0.3.0-windows-x64.zip` et `dist/n7rt-canlab-0.3.0-windows-x64.zip.sha256`. Contenu : `n7rt-can-lab.exe`, `WebView2Loader.dll`, `N7RT_CANLab_Student/`, `runtime-release.json`, helpers `tools/stm32/` et leurs imports locaux, `requirements-student-ide.txt`, `docs/` (tutoriel, rapport R1 et captures publiques), `library/` (manifestes et liens, aucun PDF), `shared/missions/`, `LISEZMOI.txt`, `manifest.json` (trié : SHA-256 et taille de chaque entrée, provenance de `source`, versions de rustc, cargo, node, npm, Python et zlib, date de build passée en argument). Entrées triées, date 1980-01-01, permissions 0644.

Un build complet crée `app/target/release/n7rt-can-lab.build.json`, reçu local qui lie le SHA-256 des entrées applicatives au SHA-256 de l'exécutable, à l'édition publique ou encadrant, à la date et à la chaîne d'outils. Les entrées couvrent `app/` hors sorties générées, documentation et caches ; elles comprennent aussi huit ressources compilées externes : `runtime-release.json`, `n7rt_runtime.o`, les deux fichiers du gabarit `student_decoder.c` et `.h`, et les énoncés APPS, WSF, SAS et BPS. Un build encadrant inclut en plus ses sources d'oracle. Une variation des entrées pendant la compilation bloque la création du reçu.

`--package-only` vérifie ce reçu avant de réemballer : reçu absent ou invalide, entrées modifiées, exécutable différent, autre édition, date ou outils différents entraînent un refus et imposent un build complet. Une modification uniquement documentaire peut être réemballée si les entrées compilées et l'exécutable restent identiques ; le manifeste décrit alors les nouveaux fichiers réellement distribués. Le reçu reste dans le cache local de build, et le manifeste de l'archive atteste séparément les octets emballés.

Le candidat non committé conserve le commit de base réel dans `source.commit`, `source.dirty: true` et une empreinte `source.working_tree_sha256` de l'arbre candidat suivi ou ajoutable. Aucun commit synthétique ne le fait apparaître propre. Une erreur de lecture de l'état Git bloque l'attestation. `--commit <sha>` est seulement une assertion : il enregistre `dirty: null` et ne prouve pas un checkout propre. Sans dépôt Git lisible, `source` peut être `null` et ne constitue pas une provenance de publication.

La revue du candidat exige deux compilations indépendantes du même instantané d'entrées brutes, mêmes outils et date, dans des arbres isolés avec des caches Cargo distincts. Les modifications non committées de cet instantané sont conservées comme telles ; aucun nouveau commit n'est nécessaire pour cette validation. Comparer les empreintes d'entrées des reçus et les SHA-256 des deux exécutables réellement compilés. Emballer ensuite les deux artefacts avec les mêmes fichiers de distribution et la même provenance candidate, puis comparer les ZIP. Réemballer deux fois un exécutable ne prouve que le packaging. Une modification compilée impose de refaire les deux compilations ; une correction documentaire impose de réemballer et de rejouer l'audit des archives. Consigner les caches, entrées, reçus, empreintes et limites de la campagne dans le rapport R1.

La campagne historique des exécutables déjà publiés 0.2.0 utilisait A au commit `9436a9d` et B au commit `59a2de3`, avec des archives identiques aux checkpoints `59a2de3` puis `734e769`. Elle ne remplace pas la validation du candidat de clôture courant. Le [rapport IDE R1](inapp-student-ide-r1.md) décrit les parcours Board C sur les binaires publiés ; le [rapport de clôture](release-0.2-closure-r1.md) porte l'état des vérifications du candidat. Le SHA-256 d'une archive reste dans son `.sha256` voisin, sans empreinte autoréférente dans un rapport inclus.

Les builds étudiant et encadrant utilisent un seul job Cargo par défaut (`--jobs` permet de choisir un entier positif). Une erreur explicite de manque de mémoire provoque au plus quatre tentatives, espacées de cinq secondes ; les logs des tentatives échouées restent dans `dist/logs/`. Une autre erreur de compilation arrête le build immédiatement.

La publication propre est une étape distincte, après revue résolue et autorisation explicite de commit/push. Depuis les sources finales committées, vérifier un `source.commit` réel et `source.dirty: false`, valider les reçus, réemballer ou reconstruire selon les entrées, puis rejouer l'audit des archives. Un `--commit` déclaré ou une validation candidate réussie n'autorise ni push ni publication.

**Outils encadrant**, depuis le dépôt encadrant, le dépôt étudiant cloné à côté :

```powershell
python tools/build_instructor.py --build-date 2026-10-14 --jobs 1
```

Option `--student ..\n7rt-canlab` si le dépôt étudiant est ailleurs ; `--package-only` comme ci-dessus. Le script reprend les fonctions du build étudiant (`tools/release/build_student.py` du dépôt étudiant), compile l'oracle (`N7RT_ORACLE_DIR` = chemin absolu de `oracle/n7rt-vehicle`), lance `npm ci` et `npm run build` dans `app/` du dépôt étudiant, construit dans `build/target` du dépôt encadrant (ignoré par git) et refuse un binaire sans la marque de l'édition encadrant. Résultat : `dist/n7rt-canlab-0.3.0-instructor-tools-windows-x64.zip` (+ `.sha256`) : `n7rt-can-lab.exe` (édition encadrant), `n7rt-tool.exe` (avec l'oracle), `n7rt-pack.exe` (les mêmes commandes, dont `gen-packs`, avec la progression 0.3 dans les packs de groupe), `WebView2Loader.dll`, les mêmes ressources publiques de SDK, helper, documentation, bibliothèque et catalogue que dans l'archive étudiant, et `manifest.json` (commits des deux dépôts). La page Code STM32 fonctionne aussi depuis cette archive extraite ; Python, GCC ARM et STM32CubeProgrammer restent des prérequis externes. **Encadrants seulement** : jamais publiée avec la release étudiant, jamais remise à un étudiant ; elle circule entre encadrants.

### 5.4 Installation

**Étudiant**

1. Télécharger `n7rt-canlab-<version>-windows-x64.zip` (page Releases du dépôt étudiant, ou remise par l'encadrant) et comparer son empreinte au `.sha256` publié : `Get-FileHash .\n7rt-canlab-<version>-windows-x64.zip`.
2. Décompresser dans un dossier accessible en écriture (pas `Program Files`) ; `library/` reste à côté de l'exécutable.
3. Lancer `n7rt-can-lab.exe`. Il n'est pas signé : SmartScreen demande « Informations complémentaires » puis « Exécuter quand même ».
4. Pour programmer depuis **Code STM32**, installer Python 3.11+, la chaîne GCC ARM et STM32CubeProgrammer, puis exécuter `python -m pip install -r .\requirements-student-ide.txt` depuis la release extraite. Suivre le [tutoriel du mini-IDE](inapp-student-ide-tutorial.md). L'import de `N7RT_CANLab_Student` dans STM32CubeIDE reste une alternative de débogage accompagnée ; sa commande Run ne passe pas par les garde-fous du flash intégré.
5. Choisir la mission (définitif pour la session), importer le pack du groupe (`<GROUPE>.n7rtpack` reçu de l'encadrant), saisir le code du groupe, ouvrir la mission.
6. Avec un pack 0.3, la progression E0 → E5 s'affiche et chaque vérification donne son verdict ; la fiche finale est exportée dans `Documents\N7RT CAN Lab\fiches\` à la clôture ([tutoriel](N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf), [démarrage en 5 minutes](demarrage-5-minutes.md)). Avec un pack 0.2, l'essai nominal écrit `Documents\N7RT CAN Lab\essais\essai-<mission>-<GROUPE>-<aaaammjj-hhmmss>.n7rtrun` (`Documents` peut être redirigé vers OneDrive), à remettre à l'encadrant, sans verdict affiché.

**Premier encadrant (une fois par équipe)**

1. Cloner les deux dépôts côte à côte.
2. Télécharger l'archive des outils encadrant de la release du dépôt encadrant (ou la construire, §5.3) ; la décompresser dans `tools\bin\` du dépôt encadrant (ignoré par git), par exemple après un build local : `Expand-Archive dist\n7rt-canlab-0.3.0-instructor-tools-windows-x64.zip -DestinationPath tools\bin`. `n7rt-tool` n'est pas dans le PATH : l'appeler par son chemin, `.\tools\bin\n7rt-tool.exe`.
3. Préparer sur le poste encadrant un coffre hors dépôt protégé par une ACL sans héritage (propriétaire Windows et SYSTEM), puis créer `instructor/` comme jonction ignorée vers son sous-dossier `instructor`. Pour une nouvelle équipe seulement, définir le code encadrant à la racine du dépôt encadrant :

   ```powershell
   .\tools\bin\n7rt-tool.exe set-code .
   ```

   La commande crée `instructor\auth.json` et `instructor\master.key` : les garder hors Git et les sauvegarder dans le stockage chiffré réservé aux encadrants. La clé maîtresse fixe la variante des groupes ; une rotation nécessite une nouvelle génération de cohorte et de packs.
4. Enregistrer le dépôt pour l'application :

   ```powershell
   python tools\install_instructor.py
   ```

   Le script demande le code sans écho, le vérifie avec `n7rt-tool verify-code` (cherché dans le PATH, puis dans `tools\bin\` ; sinon `--tool <chemin>`) et écrit `%APPDATA%\fr.n7rt.canlab\instructor.json`. `python tools\install_instructor.py --uninstall` retire l'enregistrement.

**Encadrant qui rejoint l'équipe** : après vérification des droits (§1.1), cloner les deux dépôts côte à côte et décompresser l'archive des outils encadrant dans `tools\bin\`. Après validation du canal de partage et de récupération, restaurer `instructor/` et les cohortes autorisées dans un coffre local protégé hors dépôt et créer leurs jonctions ignorées. Obtenir le code encadrant auprès de l'équipe (jamais par courriel en clair avec le lien du dépôt), puis lancer `python tools\install_instructor.py`. Ne pas lancer `set-code` pour s'installer : `auth.json` existe déjà, et la commande changerait le code de toute l'équipe. Changer le code : `.\tools\bin\n7rt-tool.exe set-code .` (demande d'abord le code actuel ; la clé maîtresse n'est jamais remplacée), puis transmettre le nouvel `auth.json` par le stockage chiffré encadrant, hors Git.

**Chemin de `n7rt-tool` sous Windows.** Le script d'installation ne modifie pas le `PATH`. Pour sélectionner sans ambiguïté l'édition encadrant, utiliser `python tools\install_instructor.py --tool .\tools\bin\n7rt-tool.exe` et vérifier `.\tools\bin\n7rt-tool.exe --version`. Si le nom court est souhaité dans la session PowerShell encadrant :

```powershell
$instructorBin = (Resolve-Path -LiteralPath .\tools\bin).Path
$env:PATH = "$instructorBin;$env:PATH"
Get-Command n7rt-tool.exe | Select-Object -ExpandProperty Source
n7rt-tool.exe --version
```

Le `PATH` contient le dossier, pas le fichier `.exe`. Cette modification dure seulement cette session ; elle n'est jamais installée sur un poste étudiant. `--tool` explicite a priorité sur le `PATH`, puis sur les emplacements locaux que le script recherche.

**Application des encadrants** : l'édition encadrant, `tools\bin\n7rt-can-lab.exe` (archive des outils encadrant), donne les verdicts ; l'application de la release étudiant n'en donne pas.

**Cohorte** : préparer `current/cohorts/<cohorte>/` dans le coffre et sa jonction ignorée `cohorts/<cohorte>/` avant la génération. Vérifier que sa cible hérite exclusivement des droits du propriétaire Windows et de SYSTEM ; les commandes ne préparent pas ce confinement. Pour une nouvelle génération, créer d'abord sa propre jonction et choisir son nom explicitement. À la racine du dépôt encadrant :

```powershell
.\tools\bin\n7rt-pack.exe gen-packs . --mission apps --cohort <cohorte> --groups G01,G02
.\tools\bin\n7rt-tool.exe judge . <fichier.n7rtrun>
.\tools\bin\n7rt-pack.exe test-pack . --mission apps --cohort <cohorte d'essai> --group TEST
```

- `gen-packs` : `n7rt-pack` scelle la progression 0.3 dans les packs (même commande dans l'application : mode encadrant, « Packs de groupe… ») ; `n7rt-tool gen-packs` produit des packs 0.2, sans progression. Les codes sont affichés une fois et gardés dans `cohorts\<cohorte>\apps\groups.json` ; remettre à chaque groupe son pack (`packs\<GROUPE>.n7rtpack`) et son code séparément.
- `judge` : code de sortie 0 réussi, 1 non réussi, 2 erreur. Autre voie : l'édition encadrant de l'application (mode encadrant, « Juger un essai… »). L'essai décisif est jugé en direct (§3.2).
- `test-pack` : pack de la variante de référence, jamais distribué à un étudiant. Un groupe existant est refusé sans `--replace` ; ne pas omettre `--cohort`.

### 5.5 Tests

**Tests publics**, dans le dépôt étudiant :

```powershell
cd app
. .\scripts\windows-gnu-env.ps1        # Windows sans MSVC seulement (même session)
npm ci
npm run check
npx vitest run
python scripts/test-workspace.py --public --jobs 1 --build-date 2026-10-08
cd ..
python -m pip install ".[dev]"
python -m pytest
```

Dans le dépôt étudiant seul, le runner lance les binaires de tests publics (substituts de l'oracle). Sous Windows, il ajoute le manifeste Common Controls v6 standard de Tauri aux seuls binaires de tests générés qui en ont besoin ; sans ce manifeste, le chargeur peut s'arrêter avant les assertions avec `0xC0000139`. Aucun réglage Windows ni exécutable distribué n'est modifié. Rapport : `app/target/test-workspace-public.json`.

**Tests privés**, dans le dépôt encadrant, le dépôt étudiant à côté :

```powershell
python tools/test_oracle.py
```

Option `--student ..\n7rt-canlab` ; les arguments placés après `--` sélectionnent ou configurent les binaires de tests. Le script appelle `app/scripts/test-workspace.py` du dépôt étudiant avec `--oracle` et le chemin absolu de `oracle/n7rt-vehicle`. Le runner compile le workspace puis exécute ses vrais binaires de tests, avec le manifeste Tauri requis sous Windows, un job de build et le cache isolé `build/target-test` du dépôt encadrant. Le rapport est `build/test-workspace-oracle.json` dans le dépôt encadrant. Avec les dépôts côte à côte, les tests sur données privées trouvent `n7rt-canlab-missions/missions/apps` (ou `N7RT_INSTRUCTOR_DIR`).

Pour la campagne complète comprenant les assertions de la bibliothèque Tauri sous Windows, depuis la racine du dépôt étudiant :

```powershell
python app/scripts/test-workspace.py --oracle --oracle-dir ../n7rt-canlab-missions/oracle/n7rt-vehicle --jobs 1 --build-date 2026-10-08
```

Le rapport est `app/target/test-workspace-oracle.json`. Le workspace combiné trouve automatiquement son oracle privé avec `--oracle`, sans `--oracle-dir`.

**Banc réel (encadrant)**, depuis le dépôt étudiant, avec `N7RT_INSTRUCTOR_DIR`, `N7RT_E2E_INSTRUCTOR_CODE` et, pour `student-apps`, `N7RT_E2E_PACK` et `N7RT_E2E_GROUP_CODE` dans l'environnement (`<carte>` : carte déclarée dans `tools/stm32/boards.json` ou `boards.local.json`) :

```powershell
python tools/stm32/stm32lab.py app-test <carte> --scenario student-apps --release --tool <n7rt-tool.exe des outils encadrant>
python tools/stm32/stm32lab.py app-test <carte> --scenario mission-apps --exe <n7rt-can-lab.exe des outils encadrant>
```

**Mémoire prolongée de l'application publique**, depuis le dépôt étudiant, sur l'exécutable extrait de l'archive candidate :

```powershell
python tools/stm32/stm32lab.py app-test --phys --scenario phys-perf --exe "CHEMIN_RELEASE_EXTRAITE\n7rt-can-lab.exe" --seconds 7200
```

`--phys` lance le Physical Lab sur PC sans carte, ST-LINK, SWD ni port série ; ne pas ajouter `C` ou une autre carte. Le scénario impose deux heures de RUN sur le circuit public de 150 composants et 300 fils, avec gestes sur potentiomètre/interrupteur et journal de production visible. Le harness relève l'application et ses processus WebView2, l'identité PID/rôle et le SHA-256 de l'exécutable ; il écrit `data/logs/app_phys-perf_<date>.json`. L'enregistrement détaillé des frames du harness est désactivé pendant le soak pour éviter une croissance de l'instrumentation.

`app-test` produit les vérifications générales du Physical Lab ; la conclusion mémoire prolongée se calcule avec l'API Python `n7rt_memory.analyze`, qui n'a pas de CLI dédiée. Depuis une session Python ouverte à la racine du dépôt, remplacer le nom du rapport :

```python
import json
import sys
from pathlib import Path

sys.path.insert(0, "tools/stm32")
from n7rt_memory import analyze

result = json.loads(Path("data/logs/app_phys-perf_DATE.json").read_text(encoding="utf-8-sig"))
verdict = analyze(result, requested_seconds=7200,
                  budget_mib=300_000_000 / 2**20,
                  drift_mib_per_hour=20_000_000 / 2**20)
print(json.dumps(verdict, indent=2, ensure_ascii=False))
```

Les seuils du contrat sont décimaux : 300 MB = 286,102 MiB pour le pic de mémoire privée, 20 MB/h = 19,073 MiB/h pour la dérive. L'analyse exclut dix minutes d'échauffement de la dérive, compare une tendance par médianes d'une minute et des médianes aux extrémités, et garde le pic brut pour le budget absolu. Elle exige la durée complète, des échantillons continus avec l'application et le renderer présents, des données PID/rôle cohérentes et les vérifications RUN réussies. Un essai interrompu ou une preuve manquante reste `incomplete`. `stable` juge la dérive ; `absolute_budget_ok` juge séparément l'empreinte, et seule `all_memory_criteria_ok` valide les deux. Reporter la durée, les unités, le pic, les deux pentes, les rôles et le SHA-256 avant de conclure ; le job Cargo unique concerne la mémoire de compilation et ne valide pas ce test.
