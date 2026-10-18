#import "n7rt-doc.typ": *

#let chap(l) = ref(l, supplement: [chapitre])
#let idt(s) = raw(brk(s))

// Tutoriel étudiant 0.3, partie 1 : prise en main, mission, interface, progression, fiche.
// Public : aucune valeur d'une mission réelle ; les exemples de valeurs sont les exemples fictifs du brief.

= À lire avant de commencer <etu1-ch-lire>

Ce tutoriel vous rend autonome sur N7RT CAN Lab 0.3. Il ne résout pas votre mission : il vous apprend à installer l'application, à brancher et connecter la carte, à ouvrir la mission de votre groupe, à lire la progression et à tenir la fiche, puis, dans sa seconde partie, à observer le bus, à mesurer au banc, à programmer la carte et à faire vérifier votre travail. Il est commun aux quatre missions (APPS, WSF, SAS et BPS) : l'interface et les procédures sont les mêmes dans la vôtre ; seuls le contenu de la mission (masqué ici) et les valeurs (fictives ici) diffèrent.

L'application juge seule chaque étape : votre fiche contre la vérité scellée dans votre pack, votre circuit contre le banc de référence, votre carte contre des essais cachés. Le texte libre (justifications, pages citées, noms des signaux) n'entre jamais dans un verdict ; il sert à votre encadrant. Un verdict dit si l'étape est validée, sans détailler ce qui est faux : à vous d'observer, de mesurer, de corriger, puis de vérifier de nouveau.

== Ce que ce document ne contient pas

- Aucune réponse : aucun identifiant de trame, aucune position de bit, aucun facteur ni offset, aucune valeur de supervision, aucune cible, catégorie ou valeur de mesure d'une mission réelle. Les établir fait partie de votre travail, à partir du bus, du banc et de la bibliothèque.
- Aucune méthode propre à une mission : les explications portent sur l'outil, sur les formats de saisie et sur la démarche générale.
- Sur les figures prises pendant une mission, tout ce qui appartient à la mission est recouvert d'un rectangle gris uni : titre et groupe, énoncé, bibliothèque, trafic du bus, résultats, valeurs choisies dans la fiche.
- Les valeurs lisibles dans les fiches des figures sont *fictives*, comme la trame `0x3C7` ou le signal _Température liquide (exemple)_. Elles ne correspondent à aucune mission : ne les recopiez pas.

== Lire une figure

Chaque figure est une capture réelle de l'application 0.3. Un tracé rouge entoure chaque élément à regarder ou à utiliser : un cercle ou un cadre arrondi pour un bouton ou un champ, un rectangle pour une zone, un trait rouge épais posé sur le fil pour un fil du banc ; une flèche rouge désigne un petit élément. Une pastille rouge numérotée accompagne chaque tracé, et le texte cite les mêmes numéros (#b(1), #b(2)…) dans l'ordre où vous agissez.

Sous chaque figure, un tableau reprend :
- *Étapes* : ce que vous faites, dans l'ordre des pastilles ;
- *Ce qu'on doit observer* : ce que l'écran montre quand l'action a réussi ;
- *Si cela échoue* : les causes fréquentes et ce qu'il faut faire.

Les libellés de l'interface sont cités entre guillemets, tels qu'ils sont écrits dans l'application. Certains titres s'affichent en capitales : « Carte » apparaît CARTE dans la barre d'outils, « Sorties à publier » apparaît SORTIES À PUBLIER dans le panneau de la mission.

== Vocabulaire

#tableau(columns: (3.3cm, 1fr), header: ([Terme], [Sens dans ce tutoriel]),
  [Cockpit], [La vue principale : mission, carte, session, trafic du bus, résultats et fiche.],
  [Physical Lab], [La vue du banc : le circuit simulé du véhicule, son inspecteur et son multimètre.],
  [Code STM32], [L'éditeur de `student_decoder.c` et `student_decoder.h`, la compilation et le flashage.],
  [Carte], [La NUCLEO-H753ZI de votre poste, qui exécute votre décodeur.],
  [Session], [Un échange en cours entre l'application et la carte : les trames du bus simulé partent vers la carte, ses résultats reviennent. « Start » la démarre, « Stop » l'arrête.],
  [Pack, code du groupe], [Le fichier `.n7rtpack` remis par l'encadrant (la mission de votre groupe, scellée) et le code remis à part qui l'ouvre.],
  [Fiche], [Le document que vous remplissez dans l'application : votre décodage (partie nominale), puis un diagnostic par incident.],
  [Slot, tick], [Un slot est une sortie numérotée de votre décodeur. Le tick est la période des appels de `student_on_tick` pendant une session (10 ms par défaut) : à chaque tick, la carte renvoie pour chaque slot la valeur publiée ou le rejet.],
)

= Installer et lancer <etu1-ch-installer>

== Récupérer et vérifier l'archive

L'application est livrée en deux fichiers, l'archive et son empreinte SHA-256 :
- `n7rt-canlab-0.3.0-windows-x64.zip` ;
- `n7rt-canlab-0.3.0-windows-x64.zip.sha256`.

Elle fonctionne sous Windows 10 ou 11, 64 bits.

+ Placez les deux fichiers dans le même dossier.
+ Ouvrez PowerShell dans ce dossier et tapez `Get-FileHash .\n7rt-canlab-0.3.0-windows-x64.zip`.
+ Comparez la valeur de la colonne « Hash » avec celle du fichier `.sha256` : elles doivent être identiques, aux majuscules près. Sinon, le téléchargement est abîmé : téléchargez de nouveau l'archive.
+ Décompressez toute l'archive (clic droit, « Extraire tout… ») dans un dossier où vous pouvez écrire, par exemple dans vos Documents, mais pas dans `Program Files`.

#tableau(columns: (5.4cm, 1fr), header: ([Élément de l'archive], [Rôle]),
  [`n7rt-can-lab.exe`], [L'application, édition étudiante.],
  [`WebView2Loader.dll`], [Le composant d'affichage de la fenêtre ; il reste à côté de l'exécutable.],
  [`N7RT_CANLab_Student\`], [Le SDK public et le projet de secours : le dossier `n7rt\` du SDK, fourni et vérifié à chaque compilation (n'y modifiez rien) ; `student\student_decoder.c` et `student\student_decoder.h`, le gabarit public de départ ; `LISEZMOI_ETUDIANT.txt` et le projet STM32CubeIDE de secours, réservé à l'encadrant (ne programmez pas la carte depuis STM32CubeIDE). Les fichiers que vous modifiez sont les copies de la vue « Code STM32 », enregistrées dans le profil local de l'application (seconde partie du tutoriel), et non ceux de ce dossier.],
  [`tools\stm32\`], [Les scripts Python qu'utilise la vue « Code STM32 » pour piloter le compilateur et STM32CubeProgrammer, à installer à part (section suivante).],
  [`library\`], [La liste des documents techniques de la bibliothèque : manifestes et liens vers les éditeurs. L'archive ne contient aucun PDF de documentation technique.],
  [`shared\missions\`], [Les énoncés publics des quatre missions (`enonce.json`), déjà intégrés à l'application ; n'y modifiez rien.],
  [`docs\`], [La documentation de la version : démarrage en 5 minutes, tutoriel de la progression 0.3 (Markdown), tutoriel de la vue « Code STM32 », problèmes connus, documents de conception, rapports de validation et captures.],
  [`requirements-student-ide.txt`], [La dépendance Python des outils (`pyserial`).],
  [`LISEZMOI.txt`], [Le résumé de l'installation.],
  [`manifest.json`, `runtime-release.json`], [L'empreinte SHA-256 de chaque fichier de l'archive ; la version et l'empreinte du runtime validé de la carte.],
  [`CODEMIRROR-LICENSES.txt`], [Les licences de l'éditeur de code intégré.],
)

#warn[Gardez tout le dossier décompressé intact : l'application cherche à côté d'elle `WebView2Loader.dll`, `library\`, `tools\` et `N7RT_CANLab_Student\`, et les outils de compilation lisent `runtime-release.json`. Ne déplacez jamais l'exécutable seul.]

== Installer les outils de compilation

La fiche, le Cockpit et le Physical Lab fonctionnent sans autre installation. Pour compiler et programmer la carte depuis la vue « Code STM32 », installez séparément, car aucun Python ni compilateur n'est embarqué :

+ Python 3.11 ou plus récent, accessible par la commande `python` ;
+ la chaîne GNU ARM, qui fournit `arm-none-eabi-gcc` ;
+ STM32CubeProgrammer ;
+ puis, dans le dossier décompressé, la dépendance des outils : \
  `python -m pip install -r .\requirements-student-ide.txt`.

#tip[Une installation de STM32CubeIDE ou de STM32CubeCLT dans `C:\ST` fournit à la fois le compilateur et STM32CubeProgrammer : les outils de l'archive les y trouvent sans autre réglage. Ils les cherchent aussi dans le `PATH`.]

Si Python ou sa dépendance manque, la vue « Code STM32 » répond _Outil de compilation indisponible : installez les prérequis du projet étudiant._ Si le compilateur manque, « Compiler » s'arrête sur un message en anglais du type `arm-none-eabi-gcc not found` ; si STM32CubeProgrammer manque, c'est « Flasher » qui s'arrête sur `STM32_Programmer_CLI not found`. Le document cité par ces messages n'est pas fourni : installez l'outil nommé ci-dessus, puis relancez l'opération.

== Brancher la carte

+ Prenez la carte NUCLEO-H753ZI de votre poste : son identifiant (« UID MCU ») est enregistré avec chaque vérification, une autre carte n'est donc pas équivalente.
+ Branchez-la au PC par le connecteur USB de son ST-LINK, CN1, au milieu du petit côté de la carte, et non par l'autre connecteur USB.
+ Attendez que Windows ait reconnu le périphérique : un port série COM apparaît.

== Premier lancement

+ Double-cliquez sur `n7rt-can-lab.exe`. L'exécutable n'est pas signé : si Windows affiche « Windows a protégé votre ordinateur », cliquez sur « Informations complémentaires », puis « Exécuter quand même ».
+ La fenêtre N7RT CAN Lab s'ouvre en mode étudiant (« Mode étudiant » dans la section « Mission » du Cockpit). Au premier lancement, elle s'ouvre sur la vue « Code STM32 » ; ensuite, elle rouvre la dernière vue utilisée.
+ Passez au Cockpit par le deuxième bouton de la barre d'activité, tout à gauche de la fenêtre (#figref("etu1-activite")).

Si la fenêtre reste blanche, le composant d'affichage de Windows manque : installez « Microsoft Edge WebView2 Runtime », puis relancez. Si elle reste noire ou figée, la mémoire du poste est peut-être saturée : fermez les applications inutiles et relancez ; la progression enregistrée est reprise.

#note[Si une seule carte ST-LINK est branchée, l'application s'y connecte d'elle-même au démarrage. Avec plusieurs cartes, elle attend votre choix : _Plusieurs cartes détectées : choisissez-en une puis « Connecter »._]

== Connecter la carte <etu1-connecter>

La connexion se règle dans la barre d'outils, en haut de la fenêtre ; cette barre est la même dans les trois vues.

#fig("etu1-carte-deconnectee",
  caption: [Carte déconnectée : choisir la carte, puis « Connecter ».],
  etapes: [
    #b(1) Dans la liste « Carte », choisissez votre carte : chaque ligne donne le port COM, la sonde ST-LINK et son numéro de série (`COM9 · STLINK-V3E · …`). Le bouton aux deux flèches circulaires, dans le même cadre à droite de la liste, est « Rechercher les cartes » : il relit la liste des ports si vous venez de brancher la carte. \
    #b(2) Cliquez sur « Connecter ». \
    #b(3) Lisez le message d'état, sous la barre d'outils : il dit à chaque instant ce que fait la liaison. \
    #b(4) Tant que la carte n'est pas connectée, la section « Carte » du panneau latéral n'affiche que des tirets.
  ],
  observer: [Avant la connexion, #b(3) affiche _Déconnecté. Choisissez une carte puis « Connecter »._ Après #b(2), l'état passe par « Poignée de main », puis « Prête » (#figref("etu1-carte-prete")).],
  echec: [La liste #b(1) affiche « Aucun port série détecté » : vérifiez le câble sur CN1, puis cliquez sur « Rechercher les cartes » (les deux flèches de #b(1)). Le message #b(3) dit que le port est déjà ouvert par une autre application (console série de STM32CubeIDE, seconde fenêtre du CAN Lab, moniteur série) : fermez-la, la connexion reprend seule.],
)

#fig("etu1-carte-prete",
  caption: [Carte prête : le message d'état, la section « Carte » et « Déconnecter ».],
  etapes: [
    #b(1) Le message d'état confirme la connexion : « Carte prête : runtime … vérifié, code étudiant chargé (CRC …). » \
    #b(2) La section « Carte » détaille la carte connectée (tableau ci-dessous). \
    #b(3) « Déconnecter » ferme la liaison. Pour passer à une autre carte, choisissez-la dans la liste « Carte », puis cliquez sur « Connecter ».
  ],
  observer: [Le voyant d'état de la barre d'outils passe de « Déconnecté » à « Prête » (#figref("etu1-barre-outils")). La barre d'état, en bas à gauche de la fenêtre, répète l'état, le port, le runtime vérifié et le CRC du code chargé.],
  echec: [L'état reste « Pas de code étudiant » : la carte n'a pas de programme étudiant valide ; programmez-la depuis la vue « Code STM32 » (« Compiler + flasher »). « Carte incompatible » ou « NON vérifié » à la ligne « Intégrité » de #b(2) : prévenez l'encadrant.],
)

#tableau(columns: (3.1cm, 1fr), header: ([Ligne de #b(2)], [Ce qu'elle indique]),
  [« Port »], [Le port série de la carte (COM…).],
  [« Sonde »], [Le modèle et le numéro de série du ST-LINK.],
  [« Runtime »], [La version du runtime N7RT installé sur la carte, son identifiant de construction et son ABI.],
  [« Intégrité »], [« défi SHA-256 vérifié », en vert, quand l'application a vérifié l'empreinte du runtime ; « NON vérifié » sinon.],
  [« Code étudiant »], [L'état du programme étudiant : « code étudiant chargé » quand un décodeur valide est présent.],
  [« CRC étudiant »], [L'empreinte CRC-32 du code étudiant présent sur la carte, suivie de son ABI. Elle change dès que le code programmé change (le même code redonne le même CRC) : comparez-la avec le « CRC compilé » de la vue « Code STM32 » pour savoir si la carte exécute bien votre dernière compilation.],
  [« UID MCU »], [L'identifiant unique du microcontrôleur, enregistré avec chaque vérification.],
  [« Dernier reset »], [La cause du dernier redémarrage de la carte.],
)

=== États de la carte

Le voyant d'état de la barre d'outils (#figref("etu1-barre-outils")) prend l'une des valeurs suivantes.

#tableau(columns: (3.5cm, 1fr, 1fr), header: ([État affiché], [Ce qui se passe], [Que faire]),
  [« Déconnecté »], [Aucune liaison n'est ouverte.], [Choisissez la carte, puis « Connecter ».],
  [« Carte absente »], [La carte choisie n'est pas détectée : câble débranché, autre connecteur, port changé.], [Vérifiez le câble sur CN1 ; l'application réessaie seule.],
  [« Port occupé »], [Une autre application a ouvert le port série de la carte.], [Fermez cette application ; la connexion reprend seule.],
  [« Poignée de main »], [L'application et la carte s'identifient.], [Attendez quelques secondes.],
  [« Prête »], [Runtime vérifié, code étudiant chargé.], [Vous pouvez démarrer une session (« Start »).],
  [« Pas de code étudiant »], [Le runtime est vérifié, mais la carte n'a pas de programme étudiant valide : « Start » reste grisé.], [Programmez la carte (vue « Code STM32 », « Compiler + flasher »), au besoin avec le modèle fourni.],
  [« Code étudiant désactivé »], [Votre code a provoqué une faute ; la carte l'a désactivé.], [Corrigez et reprogrammez ; démarrer une session le réactive aussi.],
  [« Carte incompatible »], [Le runtime de la carte ne correspond pas à cette version de l'application.], [Prévenez l'encadrant : vous ne pouvez pas le réinstaller vous-même.],
  [« Session active »], [Une session tourne : trames vers la carte, résultats en retour.], [« Stop » l'arrête.],
  [« Liaison perdue »], [La carte a été débranchée ou son port a disparu.], [Rebranchez la carte : l'application se reconnecte dès son retour.],
)

== Reconnexion

- Si le câble est débranché, l'état passe à « Liaison perdue » et le message annonce une reconnexion automatique dès le retour de la carte. Rebranchez-la : la liaison et, le cas échéant, la session en cours reprennent d'elles-mêmes. Pendant une vérification, en revanche, débrancher la carte interrompt l'essai, qui n'est pas compté (#chap(<etu1-ch-progression>)).
- Après chaque programmation, la carte redémarre et l'application refait seule la poignée de main : attendez « Prête », puis vérifiez que le « CRC étudiant » est égal au « CRC compilé » de votre dernière compilation.
- « Déconnecter » coupe la liaison volontairement : rien ne se reconnecte ensuite tant que vous n'avez pas cliqué sur « Connecter ».

= Ouvrir sa mission <etu1-ch-mission>

Tout se passe dans le Cockpit, panneau latéral, section « Mission », sous les sections « Carte » et « Session ». Une mission s'ouvre en trois temps : choisir la mission de la séance (une fois pour toutes), importer le pack du groupe (une fois), puis saisir le code du groupe (à chaque ouverture).

#fig("etu1-mission-choix", width: 78%,
  caption: [Choisir la mission de la séance (le nom de la mission choisie est masqué).],
  etapes: [
    #b(1) Dans la liste, sélectionnez la mission de votre groupe : « APPS — pédale d'accélérateur », « WSF — vitesse des roues », « SAS — angle du volant » ou « BPS — pression de freinage ». \
    #b(2) Cliquez sur « Choisir cette mission ».
  ],
  observer: [Une demande de confirmation s'affiche sous #b(2) (#figref("etu1-mission-confirmer")). Rien n'est encore verrouillé.],
  echec: [Une mission grisée dans #b(1) porte sa raison en info-bulle (« Indisponible : … ») : prévenez l'encadrant.],
)

#fig("etu1-mission-confirmer", width: 78%,
  caption: [Confirmer le choix de la mission : il est définitif.],
  etapes: [
    #b(1) Lisez l'avertissement : « Ce choix est définitif pour la session : pour en changer, demandez à votre encadrant. » \
    #b(2) Si la mission sélectionnée est bien celle de votre groupe, cliquez sur « Confirmer ». \
    #b(3) Sinon, « Annuler » referme la confirmation : sélectionnez la bonne mission.
  ],
  observer: [Après #b(2), la liste disparaît : la ligne « Mission » affiche la mission choisie et la ligne « Pack » indique « Aucun pack importé. » (#figref("etu1-mission-import")). Le choix reste verrouillé, même après un redémarrage de l'application.],
  echec: [Mauvaise mission confirmée : seul l'encadrant peut effacer ce choix sur le poste. Toute tentative d'ouvrir une autre mission répond « La mission de cette session est déjà choisie : … ».],
)

#fig("etu1-mission-import", width: 78%,
  caption: [Importer le pack du groupe (le nom de la mission est masqué).],
  etapes: [
    #b(1) Cliquez sur « Importer le pack du groupe… », puis, dans la fenêtre de Windows, sélectionnez le fichier `.n7rtpack` de votre groupe, remis par l'encadrant. \
    #b(2) Le champ « Code du groupe » reste grisé tant qu'aucun pack n'est importé.
  ],
  observer: [La ligne « Pack », juste au-dessus de #b(1), passe de « Aucun pack importé. » au nom de votre groupe suivi de sa cohorte ; le champ #b(2) devient actif. L'import se fait une seule fois sur ce poste.],
  echec: [« Ce fichier n'est pas un pack de groupe N7RT. » : vous avez sélectionné un autre fichier. « Ce pack est destiné à une autre mission. » : vérifiez avec l'encadrant le fichier et la mission choisie. « Pack du groupe illisible : demandez un nouveau fichier à votre encadrant. » et « Fichier trop volumineux pour un pack de groupe. » : demandez un nouveau pack.],
)

#fig("etu1-mission-code", width: 78%,
  caption: [Saisir le code du groupe, puis « Ouvrir » (mission et groupe masqués).],
  etapes: [
    #b(1) Vérifiez la ligne « Pack » : le nom de votre groupe et sa cohorte (masqués sur la figure). \
    #b(2) Tapez le code du groupe, reçu à part et jamais écrit dans le pack, dans « Code du groupe ». Les caractères sont masqués ; l'icône en forme d'œil les affiche. \
    #b(3) Cliquez sur « Ouvrir », ou appuyez sur Entrée.
  ],
  observer: [Le champ #b(2) se vide et la mission s'ouvre (#figref("etu1-mission-ouverte")). Le bouton « Fermer », à droite de #b(3), refermera la mission.],
  echec: [« Code de groupe incorrect. » : retapez le code tel qu'il vous a été remis. Le bouton #b(3) grisé donne sa raison en info-bulle, par exemple « Saisissez le code du groupe. » ou « Importez d'abord le pack du groupe. » Ne communiquez ni le pack ni le code à un autre groupe.],
)

#fig("etu1-mission-ouverte", width: 80%,
  caption: [Mission ouverte : le panneau de la mission en tête du panneau latéral (mission et groupe masqués).],
  etapes: [
    #b(1) Le titre de la section donne la mission et votre groupe (« Mission … — groupe … »). \
    #b(2) La progression : E0 « Essai nominal » est « EN COURS », les autres étapes sont « À FAIRE » ; la dernière ligne, « Fiche », est « En cours de rédaction ».
  ],
  observer: [Sous #b(2) viennent la consigne, « Ouvrir la fiche », « Lancer la vérification », la liste des champs à remplir (#chap(<etu1-ch-progression>)), puis l'énoncé (#chap(<etu1-ch-vue-mission>)). Les sections « Carte » et « Session » se replient et la section « Stimuli — ECU virtuel » disparaît : pendant une mission, le bus est celui du banc. La liste « Tick » est fixée par la mission. Le Physical Lab charge le banc de la mission.],
  echec: [La mission ne s'ouvre pas : lisez le message affiché sous les boutons de la section « Mission ». « Progression illisible sur ce poste : demandez à l'encadrant de réinitialiser la mission. » : prévenez l'encadrant.],
)

== Fermer et rouvrir la mission

- « Fermer », dans la section « Mission », referme la mission. Si la fiche contient des modifications non enregistrées, une fenêtre propose d'abord de les enregistrer (#figref("etu1-fiche-fermeture")).
- Pour rouvrir la mission, tapez de nouveau le code du groupe, puis « Ouvrir » : le pack reste importé et le choix de la mission reste verrouillé, même après un redémarrage de l'application.
- À la réouverture, tout ce qui a été enregistré est là : progression, fiche, banc et l'anomalie de l'incident en cours.
- Fermer l'application revient au même : rouvrez-la, puis tapez le code du groupe.

#note[Le bouton « Mode encadrant », à droite de « Mode étudiant », est réservé à l'encadrant : il demande le code encadrant.]

= L'interface en un coup d'œil <etu1-ch-interface>

La fenêtre est organisée comme un environnement de développement : une barre d'activité à gauche pour changer de vue, une barre d'outils en haut, un panneau latéral, une zone centrale, un panneau de droite, un dock en bas et une barre d'état. La figure ci-dessous montre le Cockpit sans mission, avec une session de démonstration.

#fig("etu1-vue-ensemble",
  caption: [Le Cockpit sans mission, session démarrée avec l'ECU virtuel (liste des missions masquée).],
  etapes: [
    #b(1) La barre d'outils, ici son bouton « Start » : elle porte la carte, la session (« Start », « Stop »), le voyant d'état et les boutons d'affichage des panneaux (#figref("etu1-barre-outils")). \
    #b(2) Le panneau latéral, ici sa section « Session » : identifiant, état, durée, temps du bus et tick ; toutes ses sections sont décrites dans le tableau ci-dessous. \
    #b(3) La zone centrale, « Trames reçues par le nœud étudiant » : le trafic du bus tel que la carte le reçoit, avec ses compteurs au-dessus. \
    #b(4) Le panneau de droite, « Résultats étudiants » : la dernière valeur publiée par chaque slot de votre décodeur ; dessous, « Trame sélectionnée » détaille la trame cliquée dans #b(3). \
    #b(5) La barre d'état, ici l'état de la carte (« Session active ») : elle donne aussi le port, le runtime et le CRC du code chargé, la session, le débit, le temps d'aller-retour et le nombre d'anomalies.
  ],
  observer: [Sans mission, l'ECU virtuel du SDK envoie des trames de démonstration ; pendant une mission, c'est le banc du véhicule qui émet. Le dock du bas, sous #b(3), est décrit plus loin (#figref("etu1-dock")). Pendant une mission, la barre d'état #b(5) affiche aussi l'étape en cours ; un clic dessus ramène à la progression.],
  echec: [Le panneau de droite #b(4) n'apparaît pas : la fenêtre fait moins de 1 280 pixels de large ; agrandissez-la, ou affichez le panneau avec le bouton « Résultats » de la barre d'outils.],
)

#tableau(columns: (4.3cm, 1fr), header: ([Section du panneau latéral], [Contenu]),
  [Panneau de la mission (en tête, mission ouverte)], [La progression, la consigne, les boutons de la fiche et de la vérification, puis l'énoncé et la bibliothèque (#chap(<etu1-ch-vue-mission>) et suivants).],
  [« Carte »], [Les informations de la carte connectée (#figref("etu1-carte-prete")).],
  [« Session »], [L'identifiant de la session, son état, sa durée, le temps du bus et le tick.],
  [« Mission »], [« Ouvrir le code STM32 », le mode (« Mode étudiant »), le choix de la mission, le pack, le code du groupe, « Ouvrir » et « Fermer » (#chap(<etu1-ch-mission>)).],
  [« Stimuli — ECU virtuel »], [Sans mission seulement : les commandes de l'ECU virtuel du SDK (potentiomètre, bouton, format, identifiant, DLC, cadence, « Envoyer une trame ») pour essayer un décodeur sur des trames de démonstration. Elle disparaît pendant une mission.],
)

Un clic sur le titre d'une section la replie ou la déplie. Les séparateurs entre les panneaux se déplacent à la souris ; l'application mémorise la disposition de chaque vue.

#fig("etu1-activite", width: 78%,
  caption: [La barre d'activité : les trois vues de l'application.],
  etapes: [
    #b(1) « Code STM32 » : l'éditeur de vos deux fichiers, la compilation et la programmation de la carte. \
    #b(2) « Cockpit » : la mission, la carte, la session, le trafic du bus, les résultats et la fiche. \
    #b(3) « Physical Lab » : le banc du véhicule, son inspecteur et son multimètre.
  ],
  observer: [Le bouton de la vue affichée est surligné : #b(3) sur la figure, prise dans le Physical Lab. La barre d'outils et la barre d'état restent les mêmes dans les trois vues ; le panneau latéral, la zone centrale et le panneau de droite changent. D'autres boutons mènent aux mêmes vues : « Ouvrir le code STM32 » dans la section « Mission », « Ouvrir le banc » dans le panneau de la mission.],
  echec: [Au premier lancement, la fenêtre s'ouvre sur « Code STM32 » : c'est normal ; cliquez sur #b(2) pour passer au Cockpit.],
)

#fig("etu1-barre-outils",
  caption: [La barre d'outils : session, voyant d'état de la carte et affichage des panneaux.],
  etapes: [
    #b(1) « Tick » : la période des appels de `student_on_tick` pendant une session (10 ms par défaut) ; pendant une mission, elle est fixée par la mission et la liste est grisée. \
    #b(2) « Start » démarre une session avec la carte connectée. \
    #b(3) « Stop » l'arrête ; arrêtez la session avant de programmer la carte. \
    #b(4) Le voyant d'état de la carte : « Prête », « Session active », « Déconnecté »… (tableau des états, #ref(<etu1-connecter>, supplement: [section])). \
    #b(5), #b(6) et #b(7) affichent ou masquent le panneau latéral, le dock du bas (« Console ») et le panneau de droite (« Résultats »).
  ],
  observer: [Un bouton grisé donne sa raison en info-bulle, par exemple « Connectez une carte prête pour démarrer une session. » sur « Start ». Les boutons #b(5) à #b(7) suivent la vue : dans le Physical Lab, leurs info-bulles deviennent « Bibliothèque », « Console » et « Inspecteur » ; dans « Code STM32 », « Explorateur de projet », « Console de compilation » et « Plan du code et carte ».],
  echec: [#b(2) reste grisé avec la carte connectée : l'état est « Pas de code étudiant » (programmez la carte) ou une session tourne déjà. Pendant une compilation ou un flashage, « Connecter », « Déconnecter » et « Start » sont grisés : « Compilation ou flashage en cours : la carte est occupée. »],
)

#fig("etu1-dock",
  caption: [Le dock du bas, sous la zone centrale.],
  etapes: [
    #b(1) « Console étudiant » : les lignes que votre décodeur écrit avec `n7rt_log`, et leur nombre. \
    #b(2) « Liaison » : les compteurs de la liaison avec la carte (CRC, tramage, trous de séquence, débordements, journaux perdus) et les erreurs reçues. \
    #b(3) « Diagnostic » : des mesures internes de la liaison et de l'affichage (files d'attente, contre-pression, temps de traitement), utiles pour décrire un problème à l'encadrant. \
    #b(4) « Filtrer » ne garde que les lignes qui contiennent le texte tapé.
  ],
  observer: [Pendant une mission, un onglet « Fiche » apparaît à gauche de #b(1) quand vous ouvrez la fiche (#figref("etu1-fiche-ouvrir")). Le séparateur entre la zone centrale et le dock se déplace à la souris.],
  echec: [Le dock a disparu : réaffichez-le avec le bouton « Console » de la barre d'outils (#figref("etu1-barre-outils")).],
)

= L'énoncé de la mission <etu1-ch-vue-mission>

L'énoncé de votre mission n'a pas de vue à lui : il se lit dans le panneau de la mission, en tête du panneau latéral du Cockpit, sous la progression (faites défiler le panneau). Il réunit le résumé, les exigences fonctionnelles, les sorties à publier, l'accès au banc et la bibliothèque. Lisez-le en entier avant de coder et de remplir la fiche.

#fig("etu1-enonce", width: 42%,
  caption: [Le résumé et les exigences fonctionnelles de la mission (texte masqué).],
  etapes: [
    #b(1) Le résumé : le rôle de votre carte dans le véhicule, ce que votre programme reçoit et ce qu'il publie. \
    #b(2) « Exigences fonctionnelles » : les règles que votre décodeur doit respecter (seuils, délais, conventions, conduite à tenir en cas de défaut).
  ],
  observer: [Ces textes sont propres à votre mission. Avec le règlement de la bibliothèque, #b(2) fixe les seuils et les délais qu'il énonce et la conduite que vous reportez dans les « Décisions de conception » de la fiche. Les valeurs de supervision de la fiche n'y figurent pas : votre encadrant les communique à tous les groupes au lancement de l'E0, avec les éventuelles règles que l'énoncé ne détaille pas.],
  echec: [Le panneau de la mission est absent : aucune mission n'est ouverte (#chap(<etu1-ch-mission>)).],
)

#fig("etu1-sorties-banc", width: 42%,
  caption: [« Sorties à publier » et accès au banc (liste masquée).],
  etapes: [
    #b(1) « Sorties à publier » : une ligne par sortie de votre décodeur, de la forme `Slot n — code · libellé (unité)`. Le numéro de slot est celui où votre décodeur publie la grandeur à chaque tick et que la fiche demande dans « Slots » ; le code et le libellé nomment la grandeur ; l'unité est celle de la valeur publiée. \
    #b(2) « Ouvrir le banc » affiche le banc de la mission dans le Physical Lab.
  ],
  observer: [Pour le signal fictif de ce tutoriel, une ligne de #b(1) aurait la forme `Slot 7 — XX · Température liquide (exemple) (°C)`, où `XX` serait le code de la sortie. Dès que votre décodeur publie sur un slot de la mission, « Résultats étudiants » affiche ce slot avec son code et son libellé.],
  echec: [Un slot de #b(1) n'apparaît jamais dans « Résultats étudiants » : votre décodeur ne publie pas sur ce numéro ; vérifiez le numéro dans votre code et dans la fiche.],
)

#note[La partie nominale de la fiche n'a pas de champ d'unité : l'unité d'une sortie est donnée dans les « Sorties à publier » (#figref("etu1-sorties-banc")) et intervient dans le facteur et l'offset de votre décodage. Une unité se choisit seulement pour la mesure clé d'un incident.]

#fig("etu1-bibliotheque", width: 45%,
  caption: [La bibliothèque de la mission, un document par ligne (les documents propres à la mission sont masqués).],
  etapes: [
    #b(1) Chaque document est un bouton : son titre, puis son éditeur, sa référence, sa révision et sa date. Un clic l'ouvre hors de l'application : sa copie locale si elle est présente et vérifiée, sinon la page de l'éditeur dans votre navigateur (connexion internet nécessaire).
  ],
  observer: [La liste, triée par titre, réunit les documents de votre mission et des documents communs aux quatre missions, comme la spécification CAN de #b(1). Les documents ne disent jamais quoi saisir : vous y retrouvez l'information, puis vous citez les documents et les pages lus dans « Documents utilisés » de la fiche.],
  echec: [« Bibliothèque introuvable sur ce poste. » : prévenez l'encadrant. Un document ne s'ouvre pas sans internet : demandez sa copie locale à l'encadrant (procédure dans `library\README.md`).],
)

Méthode : relevez le marquage écrit sur le symbole de chaque capteur dans le Physical Lab et cherchez-le (Ctrl+F) dans les fiches techniques, car certaines décrivent un autre capteur que celui du banc ; dans la bonne, cherchez la caractéristique de sortie et les diagnostics du capteur. Pour la protection des trames, servez-vous des spécifications de la bibliothèque ; pour les seuils et les délais, des « Exigences fonctionnelles » de l'énoncé, puis du règlement ; les valeurs de supervision n'y figurent pas, votre encadrant les communique au lancement de l'E0. Ne confondez pas cette bibliothèque de documents avec la « Bibliothèque » de composants du Physical Lab.

= La progression E0 → E5 <etu1-ch-progression>

Une mission se déroule en six étapes, toujours dans le même ordre. Les étapes vérifiées (E0 à E4) ont chacune leur bouton ; E0 et chaque incident (E1 à E3) ont aussi leur partie de fiche.

== Les étapes et leurs états

#tableau(columns: (1.4cm, 3.2cm, 1fr, 3.5cm), header: ([Étape], [Nom affiché], [Ce qui est demandé], [Bouton]),
  [E0], [« Essai nominal »], [Identifier les trames de votre ECU sur le bus, les décoder, programmer la carte et documenter le décodage dans la partie nominale de la fiche.], [« Lancer la vérification »],
  [E1 à E3], [« Incident 1 », « Incident 2 », « Incident 3 »], [Le véhicule revient d'essai avec une anomalie : l'observer, la diagnostiquer dans la fiche d'incident, consigner la mesure clé, puis réparer le véhicule, votre code, ou les deux.], [« Vérifier la réparation »],
  [E4], [« Qualification finale »], [Une séquence d'essais cachée évalue votre code ; le résultat est donné par compétence. La fiche n'a pas de partie propre à E4.], [« Lancer la qualification »],
  [E5], [« Clôture »], [La mission est close et la fiche finale exportée.], [« Ouvrir la fiche finale » (aucune vérification)],
)

#tableau(columns: (4cm, 1fr), header: ([État d'une étape], [Sens]),
  [« À FAIRE »], [L'étape n'est pas encore ouverte : elle s'ouvrira quand la précédente sera validée.],
  [« EN COURS »], [L'étape sur laquelle vous travaillez ; aucune de ses vérifications n'a encore échoué.],
  [« VALIDÉ »], [L'étape est réussie ; sa date s'affiche dessous (`le JJ/MM/AAAA à HH:MM`). Elle ne se rouvre jamais.],
  [« ÉCHEC À CORRIGER »], [La dernière vérification de l'étape a échoué : corrigez, puis vérifiez de nouveau.],
)

La ligne « Fiche », sous les étapes, donne l'état de votre fiche : « En cours de rédaction » pendant toute la mission, puis « Exportée le … » à la clôture (ou « Clôturée, exportation à reprendre » si l'export a échoué).

- *Une étape à la fois.* Les étapes s'enchaînent dans l'ordre E0, E1, E2, E3, E4, puis E5. Une étape ne s'ouvre qu'une fois la précédente validée ; une étape validée ne se rouvre pas ; aucune ne se saute.
- *Pas de note.* Il n'y a ni note, ni compteur, ni pourcentage : une étape est validée ou à reprendre. Le nombre de vérifications n'est pas limité.

== La consigne et le bouton de vérification

#fig("etu1-progression-etapes", width: 40%,
  caption: [Le haut du panneau de la mission : les étapes et la consigne.],
  etapes: [
    #b(1) Les cinq étapes vérifiées, E0 à E4, avec leur état, puis la ligne « Fiche » et l'état de la fiche. \
    #b(2) « Consigne », suivi de l'étape en cours (ici « E0 · Essai nominal »). \
    #b(3) La consigne elle-même, c'est-à-dire l'ordre de travail de l'étape (ici celui de E0).
  ],
  observer: [Au début de la mission, seule E0 est « EN COURS » dans #b(1). La consigne #b(3) change à chaque étape.],
  echec: [Le panneau de la mission est replié : cliquez sur son titre pour le déplier. Il est absent : la mission n'est pas ouverte (#chap(<etu1-ch-mission>)).],
)

#fig("etu1-progression-boutons", width: 44%,
  caption: [Sous la consigne : la fiche, la vérification et ce qui la bloque.],
  etapes: [
    #b(1) « Ouvrir la fiche » affiche la partie de fiche de l'étape en cours dans le dock du bas (#chap(<etu1-ch-fiche>)) ; le bouton devient « Fermer la fiche ». \
    #b(2) Le bouton de vérification de l'étape, ici « Lancer la vérification ». \
    #b(3) La raison pour laquelle #b(2) est grisé ; ici « Complétez la fiche de cette étape avant la vérification. », suivi de « Champs à renseigner : » et d'une ligne par section ou par bloc incomplet.
  ],
  observer: [La liste #b(3) porte sur la fiche enregistrée et se met à jour à chaque enregistrement. Les autres raisons possibles sont « Connectez la carte et vérifiez que votre programme est chargé. », « Prochaine vérification possible dans … » (#figref("etu1-delai")), « Corrigez les champs signalés dans la fiche. » (#figref("etu1-fiche-erreur")) et « Vérification déjà en cours. »],
  echec: [#b(2) reste grisé alors que la fiche vous semble complète : enregistrez-la (« Enregistrer la fiche »), car #b(3) ne tient compte que de la fiche enregistrée.],
)

== Pendant une vérification

Une vérification exige la carte connectée et prête, avec votre programme chargé, une partie de fiche complète et enregistrée, et la fin du délai depuis la vérification précédente. Le bouton de vérification enregistre d'abord la partie de fiche ouverte. Pendant l'essai :

- la zone centrale et les résultats sont masqués : « Essai en cours : la vue du bus est masquée jusqu'au verdict. » et « Essai en cours : les résultats sont masqués jusqu'au verdict. » ;
- sous le bouton, une barre de progression et le compte à rebours : « Essai en cours — … s. Ne débranchez pas la carte et ne fermez pas l'application. » ;
- la fiche est en lecture seule.

Le verdict s'affiche ensuite sous la consigne ; sa lecture est détaillée dans la seconde partie du tutoriel. Un essai interrompu (carte débranchée, application fermée) n'est pas compté.

== Le délai entre deux vérifications

Deux vérifications d'une même étape sont séparées d'au moins 2 minutes pour E0 à E3, et de 5 minutes pour E4, comptées depuis la fin du dernier verdict de l'étape. Un essai interrompu ne déclenche pas ce délai, sauf au-delà de six interruptions de la même étape dans l'heure.

#fig("etu1-delai", width: 50%,
  caption: [Après un échec : le délai avant la vérification suivante.],
  etapes: [
    #b(1) Le bouton de vérification reste grisé pendant le délai. \
    #b(2) « Prochaine vérification possible dans … » donne le temps restant ; il se met à jour seul.
  ],
  observer: [Au-dessus de #b(1), le verdict de la vérification précédente reste affiché : « Échec à corriger. » et ses lignes de contrôle. À la fin du délai, #b(1) redevient actif et #b(2) disparaît.],
  echec: [Le délai ne se raccourcit pas : profitez-en pour reprendre vos observations, vos mesures, votre code et votre fiche.],
)

== Des tentatives liées à la session, à la carte et au code

Chaque vérification est enregistrée comme une tentative, avec la version de la fiche, l'empreinte de votre circuit, le CRC du code étudiant présent sur la carte, l'identifiant de la session, l'identifiant (UID) de la carte et une empreinte du trafic du bus. Toutes les tentatives, réussies ou non, figurent dans la fiche finale. Vérifiez donc avec la carte de votre poste, programmée avec votre dernière compilation : un CRC qui ne correspond pas à votre code signale que la carte n'exécute pas ce que vous croyez.

== Les étapes d'incident

Pendant les incidents, le panneau garde la même organisation : la consigne et le bouton de vérification changent, et un bouton s'ajoute sous la consigne.

#fig("etu1-etape-incident", width: 48%,
  caption: [Une étape d'incident : états, consigne et bouton « Vérifier la réparation ».],
  etapes: [
    #b(1) E0 est « VALIDÉ », avec sa date ; E1 est l'étape en cours, ici « EN COURS » : elle vient de s'ouvrir, comme l'annonce le verdict vert de E0, encore affiché sous la consigne ; les étapes suivantes restent « À FAIRE ». Après une vérification manquée, E1 passerait à « ÉCHEC À CORRIGER ». \
    #b(2) L'étape de la consigne, ici « E1 · Incident 1 », puis la consigne d'un incident : observer l'anomalie, consigner le diagnostic, réparer. \
    #b(3) « Reproduire l'anomalie » remet le véhicule dans l'état où il est revenu d'essai ; son usage est décrit avec les incidents, dans la seconde partie. \
    #b(4) Pendant les incidents, « Vérifier la réparation » remplace « Lancer la vérification ».
  ],
  observer: [Pendant un incident, « Ouvrir la fiche » ouvre la fiche de l'incident en cours (#figref("etu1-incident")) ; la partie nominale reste consultable après E0. Sous #b(4), le délai avant la vérification suivante s'affiche comme en E0.],
  echec: [#b(4) reste grisé : la liste sous le bouton nomme ce qui manque, par exemple « Mesure clé : consigner la mesure » tant que la mesure n'est pas consignée.],
)

= La fiche <etu1-ch-fiche>

La fiche est le document de votre groupe : la partie nominale, en E0, décrit votre décodage ; chaque incident, de E1 à E3, a sa propre partie de diagnostic. Elle s'ouvre dans le dock du bas du Cockpit. Le formulaire contrôle seulement la forme de chaque champ (un identifiant hexadécimal, un entier dans sa plage, un nombre décimal…), jamais la justesse : la justesse n'est jugée qu'à la vérification.

== Ouvrir et parcourir la fiche

#fig("etu1-fiche-ouvrir", width: 78%,
  caption: [Ouvrir la fiche : elle s'affiche dans l'onglet « Fiche » du dock.],
  etapes: [
    #b(1) Cliquez sur « Ouvrir la fiche », sous la consigne ; une fois la fiche ouverte, ce bouton affiche « Fermer la fiche », comme sur la capture. \
    #b(2) La fiche s'affiche dans l'onglet « Fiche » du dock, le premier à gauche ; le dock s'agrandit pour elle.
  ],
  observer: [L'onglet #b(2) n'existe que pendant une mission. « Fermer la fiche », ou un clic sur un autre onglet, masque la fiche sans perdre ce que vous y avez tapé ; #b(1) ou #b(2) la réaffiche.],
  echec: [Le dock du bas était masqué : #b(1) le réaffiche de lui-même. Un message s'affiche sous le bouton de vérification au lieu de la fiche : lisez-le, puis fermez et rouvrez la mission avec le code du groupe.],
)

#fig("etu1-fiche-sections", width: 90%,
  caption: [La partie nominale vide : l'index des sept sections et « Enregistrer la fiche ».],
  etapes: [
    #b(1) L'index des sections, numérotées 01 à 07 : « Documents utilisés », « Trames », « Signaux », « Statuts », « Supervision de la trame de données », « Décisions de conception », « Justification ». Un clic sur une entrée amène sa section en haut de la fiche ; l'entrée de la section affichée est surlignée. \
    #b(2) « Enregistrer la fiche » reste visible en bas de la fiche.
  ],
  observer: [Une fiche vide n'a aucun bloc : sous chaque titre, « Ajouter un document », « Ajouter une trame », « Ajouter un signal » ou « Ajouter un statut » crée un bloc (« Document 1 », « Trame 1 »…). Chaque bloc a son bouton « Supprimer » : un premier clic le change en « Confirmer la suppression », un second clic dans les 5 secondes supprime le bloc. Une section qui contient un champ mal formé apparaît en rouge dans l'index.],
  echec: [« Ajouter … » ne crée rien : un champ de la fiche est mal formé ; corrigez-le d'abord (#figref("etu1-fiche-erreur")).],
)

== Enregistrer la fiche

- *Rien ne s'enregistre tout seul.* Dès qu'un champ change, « Modifications non enregistrées » s'affiche à droite de « Enregistrer la fiche », qui passe en couleur (#figref("etu1-fiche-trame")).
- « Enregistrer la fiche » enregistre tout le formulaire et affiche « Enregistré » à la place de « Modifications non enregistrées ». Une fiche incomplète s'enregistre : vous la complétez quand vous voulez.
- Le bouton de vérification, et « Consigner la mesure » pendant un incident, enregistrent aussi la partie ouverte ; sous le bouton de vérification, « La fiche sera enregistrée avant la vérification. » le rappelle.
- Un champ mal formé bloque tout l'enregistrement : la fiche répond « Certains champs ne sont pas au bon format : corrigez-les avant d'enregistrer. » et rien n'est enregistré (#figref("etu1-fiche-erreur")).
- La fiche passe en lecture seule pendant une vérification (« Vérification en cours : la fiche n'est pas modifiable. »), quand sa partie est validée (« Partie validée : seuls la justification, les pages et les noms des signaux restent modifiables. » pour la partie nominale, « Partie validée : la fiche n'est pas modifiable. » pour un incident) et après la clôture (« La mission est clôturée : la fiche n'est plus modifiable. »).

#fig("etu1-fiche-erreur", width: 85%,
  caption: [Un champ mal formé (le reste de la fiche est masqué).],
  etapes: [
    #b(1) Le champ refusé est encadré en rouge ; ici, un identifiant de quatre chiffres hexadécimaux. \
    #b(2) Son message de format s'affiche dessous : « 0x suivi de 1 à 3 chiffres hexadécimaux, au plus 0x7FF. » \
    #b(3) L'index marque en rouge la section qui contient le champ. \
    #b(4) Sous le bouton de vérification, grisé : « Corrigez les champs signalés dans la fiche. »
  ],
  observer: [Le contrôle a lieu quand vous quittez le champ et à l'enregistrement ; dès que vous retapez dans le champ, sa marque #b(1) disparaît. Une valeur bien formée mais fausse n'est jamais signalée ici : seul le verdict en tient compte.],
  echec: [Vous ne trouvez pas le champ fautif : cliquez sur « Enregistrer la fiche » ; la fiche amène le premier champ mal formé à l'écran et y place le curseur.],
)

#fig("etu1-fiche-fermeture", width: 70%,
  caption: [Fermer la mission avec une fiche non enregistrée (l'arrière-plan est masqué).],
  etapes: [
    #b(1) « Enregistrer et fermer » enregistre la fiche, puis ferme. Dans cette version, la fenêtre est trop étroite pour ses trois boutons : le début du libellé est coupé par son bord gauche et seul « et fermer » reste lisible. \
    #b(2) « Fermer sans enregistrer » ferme en abandonnant les modifications ; ce bouton rouge reste inactif une fraction de seconde après l'ouverture de la fenêtre. \
    #b(3) « Revenir à la mission » annule la fermeture : vous retrouvez la fiche telle quelle.
  ],
  observer: [La fenêtre « Fiche non enregistrée » s'ouvre quand vous fermez la mission (« Fermer ») ou l'application alors que la fiche contient des modifications non enregistrées : « La fiche contient des modifications non enregistrées. Que souhaitez-vous faire ? » À la fermeture de l'application, #b(3) s'appelle « Revenir à l'application » ; ce bouton a le focus à l'ouverture, et la touche Échap a le même effet.],
  echec: [L'enregistrement demandé par #b(1) échoue (un champ mal formé, par exemple) : la fenêtre devient « Fermeture suspendue » et la raison est écrite au bas de la fiche ; cliquez sur « Revenir à la mission », corrigez, puis fermez de nouveau. Pour abandonner les modifications, fermez de nouveau et choisissez #b(2).],
)

== Les informations de la partie nominale

=== Positions des bits

Plusieurs champs décrivent la place d'une information dans les 8 octets de données d'une trame, par trois valeurs : « Ordre », « LSB » et « Longueur ».

- Le bit $i$ de la trame (de 0 à 63) est le bit ($i$ mod 8) de l'octet $floor(i\/8)$ ; l'octet 0 est transmis le premier et le bit 0 est le poids faible de chaque octet.
- « LSB » est la position du bit de poids faible du champ ; « Longueur » est son nombre de bits.
- En `INTEL`, les bits suivants du champ sont $i+1$, $i+2$… En `MOTOROLA`, ils remontent l'octet, puis continuent au bit 0 de l'octet précédent.

#tableau(columns: (4.4cm, 3.4cm, 1fr), header: ([Champ fictif], [Ordre, LSB, longueur], [Bits occupés]),
  [CRC de la trame fictive `0x3C7`], [`INTEL`, 40, 8], [40 à 47 : tout l'octet 5.],
  [Compteur de la trame `0x3C7`], [`INTEL`, 36, 4], [36 à 39 : les quatre bits de poids fort de l'octet 4.],
  [_Température liquide (exemple)_], [`MOTOROLA`, 22, 10], [22 et 23 (bits 6 et 7 de l'octet 2, poids faible du champ), puis 8 à 15 (tout l'octet 1, poids fort).],
  [Statut fictif], [`INTEL`, 6, 2], [6 et 7 : les deux bits de poids fort de l'octet 0.],
)

La valeur physique se calcule à partir de la valeur brute lue dans ces bits : valeur physique = valeur brute × facteur + offset. Pour le signal fictif, le facteur vaut 0,25 et l'offset −40, en °C ; sa valeur brute maximale, 1023 (dix bits à 1), est ici son code invalide et ne se convertit pas.

=== Documents utilisés

#tableau(columns: (2.7cm, 1fr, 1fr), header: ([Champ], [Ce qu'il faut saisir], [Erreurs fréquentes]),
  [« Document »], [Un document de la liste, qui reprend la bibliothèque de votre mission. Ajoutez un bloc par document lu.], [Laisser le choix vide : le bloc reste incomplet.],
  [« Pages »], [Texte libre, non évalué : les pages ou les sections lues (`p. …`).], [Aucune : le champ n'est pas contrôlé.],
)

Où chercher : la bibliothèque (#figref("etu1-bibliotheque")). Exemple fictif : dans « Document », la fiche technique d'un capteur ; dans « Pages », _section Caractéristiques de sortie_. La fiche exige au moins un document.

=== Trames

#fig("etu1-fiche-trame",
  caption: [Un bloc « Trame » rempli avec les valeurs fictives du tutoriel.],
  etapes: [
    #b(1) « Identifiant ». #b(2) « Format ». #b(3) « DLC ». #b(4) « Période (ms) ». #b(5) « Protection ». \
    #b(6) Le « LSB » du « CRC de protection », entre son « Ordre » et sa « Longueur ». \
    #b(7) Le « LSB » du « Compteur de protection », avec les mêmes voisins. \
    #b(8) Le « LSB » du « Compteur de vie », laissé vide comme ses voisins : la trame fictive n'en a pas. \
    #b(9) « Enregistrer la fiche », en couleur tant que la fiche a changé. #b(10) « Modifications non enregistrées ».
  ],
  observer: [Le titre du bloc reprend l'identifiant (« Trame 1 » suivi de `0x3C7`). Après #b(9), #b(10) laisse place à « Enregistré ».],
  echec: [Un champ s'encadre de rouge : sa forme est refusée ; lisez le message sous le champ et le tableau ci-dessous.],
)

#tableau(columns: (2.7cm, 1fr, 0.8fr, 3cm), header: ([Champ], [Sens et format exact], [Erreurs fréquentes], [Exemple fictif]),
  [« Identifiant »], [L'identifiant CAN de la trame, sur 11 bits : `0x` suivi de 1 à 3 chiffres hexadécimaux, au plus `0x7FF`. La fiche l'affiche ensuite en majuscules, sur trois chiffres (`0x3c7` devient `0x3C7`).], [Oublier `0x` ; écrire l'identifiant en décimal ; écrire quatre chiffres (`0x03C7`) ; dépasser `0x7FF`.], [`0x3C7`],
  [« Format »], [Le format de la trame ; une seule valeur proposée, #idt("STANDARD_11_BITS").], [Laisser le choix vide.], [#idt("STANDARD_11_BITS")],
  [« DLC »], [Le nombre d'octets de données : entier de 0 à 8.], [Donner une longueur en bits.], [`6`],
  [« Période (ms) »], [L'intervalle nominal entre deux trames de cet identifiant, en millisecondes : entier de 0 à 100 000, sans unité.], [Écrire `50 ms` ou `50,0` ; recopier un écart isolé au lieu d'une moyenne sur plusieurs trames.], [`50`],
  [« Protection »], [`oui` si la trame porte une protection de bout en bout (CRC et compteur), `non` sinon.], [Répondre sans avoir observé la trame sur plusieurs envois.], [`oui`],
  [« CRC de protection »], [La place du CRC de protection : « Ordre » (`INTEL` ou `MOTOROLA`), « LSB » (entier de 0 à 63), « Longueur » (entier de 1 à 64). À remplir si la protection est `oui`.], [Donner le numéro de l'octet au lieu de celui du bit ; un champ qui déborde au-delà du bit 63.], [`INTEL`, `40`, `8`],
  [« Compteur de protection »], [La place du compteur de la protection : mêmes trois champs.], [Le confondre avec un compteur de vie.], [`INTEL`, `36`, `4`],
  [« Compteur de vie »], [Un compteur distinct du compteur de protection, s'il existe ; sinon, laissez les trois champs vides. Ils sont tous vides ou tous remplis.], [Remplir un seul des trois champs ; y recopier le compteur de protection.], [vides],
)

Où chercher : le tableau « Trames reçues par le nœud étudiant » (colonnes « ID », « DLC », « Données », et « t (ms) » pour la période, en moyennant plusieurs écarts), le détail « Trame sélectionnée », et, pour la protection, les spécifications de la bibliothèque. La fiche exige au moins une trame.

=== Signaux

#fig("etu1-fiche-signal",
  caption: [Un bloc « Signal » rempli avec le signal fictif du tutoriel.],
  etapes: [
    #b(1) « Nom », texte libre non évalué. #b(2) « Identifiant » de la trame qui porte le signal. #b(3) « Slots ». \
    #b(4) « Position » : « Ordre », « LSB » et « Longueur ». #b(5) « Signe ». \
    #b(6) « Facteur ». #b(7) « Offset ». #b(8) « Code invalide ».
  ],
  observer: [Le titre du bloc reprend l'identifiant de #b(2). Le champ #b(3) montre la saisie de deux slots séparés par un point-virgule ; la virgule et l'espace sont aussi acceptés. Le signal fictif du tutoriel n'a qu'un slot, `7`.],
  echec: [#b(6) ou #b(7) refusé : écrivez un seul nombre, sans unité ni fraction. #b(8) refusé : un entier, `0x…` ou `AUCUN`.],
)

#tableau(columns: (2.6cm, 1fr, 0.85fr, 2.6cm), header: ([Champ], [Sens et format exact], [Erreurs fréquentes], [Exemple fictif]),
  [« Nom »], [Le nom que vous donnez au signal ; texte libre, non évalué.], [Aucune.], [_Température liquide (exemple)_],
  [« Identifiant »], [La trame qui porte le signal ; même format que l'identifiant d'une trame.], [Un identifiant absent de la section « Trames ».], [`0x3C7`],
  [« Slots »], [Le ou les numéros de slot où ce signal est publié, pris dans les « Sorties à publier » : entiers de 0 à 31, séparés par une virgule, un espace ou un point-virgule.], [Écrire le code de la sortie au lieu de son numéro ; un intervalle écrit avec un tiret ; un numéro au-delà de 31.], [`7`],
  [« Position »], [« Ordre », « LSB », « Longueur » du signal (voir les positions des bits).], [Inverser `INTEL` et `MOTOROLA` ; donner le bit de poids fort au lieu du LSB.], [`MOTOROLA`, `22`, `10`],
  [« Signe »], [`NON_SIGNÉ` ou #idt("COMPLÉMENT_À_DEUX") : la façon de lire la valeur brute.], [Confondre le signe de la valeur brute avec celui de la grandeur physique : un offset négatif peut rendre la grandeur négative avec une valeur brute non signée.], [`NON_SIGNÉ`],
  [« Facteur »], [Le multiplicateur de la valeur brute : nombre décimal, virgule ou point, exposant accepté (`2.5e-1`).], [Une fraction (`1/4`) ; une unité (`0,25 °C`) ; un espace entre les milliers.], [`0,25`],
  [« Offset »], [La constante ajoutée après le facteur, dans l'unité de la grandeur publiée ; nombre décimal.], [Inverser l'ordre des opérations (l'offset s'ajoute après le facteur).], [`-40`],
  [« Code invalide »], [La valeur brute qui signale une valeur indisponible : entier décimal, `0x…` en hexadécimal, ou `AUCUN` s'il n'y en a pas.], [Laisser le champ vide au lieu d'écrire `AUCUN` ; donner une valeur physique au lieu de la valeur brute.], [`1023`],
)

Où chercher : les fiches techniques des capteurs (caractéristique de sortie, plage, diagnostics), les « Sorties à publier » (slots et unités), et le bus : faites varier la grandeur au banc et observez quels bits changent. La fiche exige au moins un signal.

=== Statuts

Un statut est un champ de la trame par lequel la source dit si sa valeur est valable. Il se décrit comme un signal, avec moins de champs : un bloc « Statut » (« Ajouter un statut ») a les mêmes champs « Identifiant », « Slots » et « Position » que le bloc « Signal » (#figref("etu1-fiche-signal")), puis « Code OK ».

#tableau(columns: (2.6cm, 1fr, 0.85fr, 2.6cm), header: ([Champ], [Sens et format exact], [Erreurs fréquentes], [Exemple fictif]),
  [« Identifiant »], [La trame qui porte le statut.], [—], [`0x3C7`],
  [« Slots »], [Les slots concernés par ce statut ; même format que pour un signal.], [Oublier un slot concerné.], [`7`],
  [« Position »], [« Ordre », « LSB », « Longueur » du statut.], [Voir les signaux.], [`INTEL`, `6`, `2`],
  [« Code OK »], [La valeur brute qui signifie que tout va bien : nombre entier, en décimal.], [Donner le code d'un état de défaut ; écrire le code en binaire (`10` est lu dix) ou en hexadécimal.], [`2`],
)

Où chercher : les diagnostics décrits dans les fiches techniques et la spécification de la source, et le bus. La fiche exige au moins un statut.

=== Supervision de la trame de données

#fig("etu1-fiche-supervision",
  caption: [La supervision fictive du tutoriel, au-dessus des décisions de conception encore vides.],
  etapes: [
    #b(1) « Premier timeout (ms) ». #b(2) « Timeout (ms) ». #b(3) « Péremption (ms) ». #b(4) « Pertes consécutives tolérées ». \
    #b(5) « Décisions de conception » : quatre listes, de DD-1 à DD-4.
  ],
  observer: [Les quatre champs de supervision sont des entiers de 0 à 100 000, sans unité. Les listes de #b(5) commencent sur « — » : aucune décision n'est choisie.],
  echec: [« Entier de 0 à 100 000. » sous un champ : vous avez tapé une unité, une virgule ou un nombre négatif.],
)

#tableau(columns: (3.4cm, 1fr, 0.8fr, 1.8cm), header: ([Champ], [Sens], [Erreurs fréquentes], [Exemple fictif]),
  [« Premier timeout (ms) »], [Le délai maximal accepté pour recevoir la première trame valide, au démarrage.], [Le confondre avec le timeout, qui ne s'applique qu'après la première trame valide.], [`300`],
  [« Timeout (ms) »], [Le délai maximal accepté entre deux trames valides, ensuite.], [Écrire la durée en secondes ou avec son unité.], [`150`],
  [« Péremption (ms) »], [L'âge au-delà duquel une donnée reçue n'est plus considérée comme fraîche (voir DD-4).], [La confondre avec le timeout : la péremption porte sur l'âge de la donnée, le timeout sur l'écart entre deux trames valides.], [`90`],
  [« Pertes consécutives tolérées »], [Le nombre de trames consécutives qui peuvent manquer sans que la supervision déclare un défaut.], [Donner une durée au lieu d'un nombre de trames.], [`2`],
)

Où chercher : ces quatre valeurs ne figurent ni dans l'énoncé ni dans la bibliothèque. Votre encadrant les communique à tous les groupes au lancement de l'E0 ; notez-les, car la fiche et le code de votre carte doivent appliquer exactement ces valeurs-là. Si vous ne les avez pas reçues, demandez-les-lui.

=== Décisions de conception

Chaque décision fixe la conduite de votre décodeur dans une situation précise ; elle est contrôlée comme les autres champs et doit correspondre à ce que fait votre code.

#tableau(columns: (4.6cm, 1fr), header: ([Décision], [Choix proposés et effet de chacun dans votre code]),
  [« DD-1 Valeur égale au code invalide »], [`REJETER` : le slot reçoit un rejet (`n7rt_reject`) ; `PUBLIER` : la valeur est publiée comme les autres.],
  [« DD-2 Trame à protection non conforme »], [#idt("IGNORER_SES_DONNÉES") : les données de cette trame ne sont pas utilisées ; #idt("UTILISER_SES_DONNÉES") : elles sont décodées malgré tout.],
  [« DD-3 Valeur hors plage sous statut OK »], [`REJETER` : rejet ; `BORNER` : la borne la plus proche de la plage est publiée ; `PUBLIER` : la valeur est publiée telle quelle.],
  [« DD-4 Donnée plus vieille que la péremption »], [`REJETER` : rejet ; #idt("PUBLIER_LA_DERNIÈRE") : la dernière valeur reste publiée.],
)

Où chercher : les « Exigences fonctionnelles » de l'énoncé, puis le règlement. Erreur fréquente : choisir une décision que le code n'applique pas, ou l'inverse. Aucun exemple n'est donné : tout choix pourrait être la réponse d'une mission.

=== Justification

« Justification » (non évaluée automatiquement) : un texte libre où vous expliquez votre raisonnement, les mesures faites et les documents utilisés. La vérification ne le lit jamais ; votre encadrant, si. Il reste modifiable après la validation de E0, comme les pages et les noms des signaux.

== La fiche d'incident (E1 à E3)

Pendant un incident, « Ouvrir la fiche » ouvre la fiche de l'incident en cours. Elle a cinq blocs. Les listes de catégories et d'états observés sont les mêmes pour les quatre missions.

#fig("etu1-incident",
  caption: [La fiche d'un incident : les cinq blocs (toutes les valeurs choisies sont masquées).],
  etapes: [
    #b(1) « Cible » : choisissez d'abord le type de cible. \
    #b(2) Puis désignez la cible : deux broches, un composant ou un identifiant, selon le type. \
    #b(3) « Catégorie » : la famille de la cause ; sa définition s'affiche sous la liste. \
    #b(4) « État observé » : ce que vous constatez ; sa définition s'affiche sous la liste. \
    #b(5) La grandeur imposée par l'état observé : « Grandeur imposée : … ». \
    #b(6) « Consigner la mesure » scelle les conditions de votre mesure. \
    #b(7) L'état de la consignation : ici « Mesure non consignée. » ; après un clic sur #b(6), « Mesure consignée le … », suivi de la date et de l'heure. \
    #b(8) « Valeur » et « Unité » de la mesure clé. \
    #b(9) La justification, en texte libre.
  ],
  observer: [Tant qu'aucun état n'est choisi, #b(5) affiche « Choisissez l'état observé : il impose la grandeur à relever. » Sous #b(5), la consigne de mesure : « Relevez cette grandeur dans des conditions où l'anomalie se manifeste, sans bouger les curseurs avant d'appuyer sur Consigner. » La liste « Unité » de #b(8) ne propose que les unités de la grandeur imposée.],
  echec: [« La mesure ne peut être consignée que pendant l'incident en cours, hors vérification. » : attendez la fin de l'essai. « Aucune trame récente sur le bus : lancez le véhicule, puis consignez la mesure. » : démarrez une session (« Start »), attendez au moins 3 s, puis consignez de nouveau.],
)

#tableau(columns: (4.3cm, 1fr, 2.6cm), header: ([Type de cible], [Désignation et format], [Exemple fictif]),
  [« Conducteur (ses deux broches) »], [Un fil ou une liaison du banc, désigné par ses deux extrémités, « Broche A » et « Broche B », choisies dans les listes des broches du banc sous la forme `désignateur.broche`.], [`R9.p1` – `U7.in3`],
  [« Composant (son désignateur) »], [Un composant du banc, choisi par son désignateur dans la liste « Composant ».], [`U7`],
  [« Identifiant (trame du bus) »], [Une trame du bus, par son identifiant : même format que dans la partie nominale.], [`0x3C7`],
)

#tableau(columns: (4.6cm, 1fr), header: ([Catégorie], [Définition affichée sous la liste]),
  [#idt("SENSOR/PHYSICAL")], [Défaut du capteur, de son faisceau, de son montage ou de la grandeur physique. Les trames de l'ECU source restent conformes à leur protection et à leur rythme.],
  [#idt("SOURCE_ECU/APPLICATION")], [Défaut de l'ECU source lui-même (logique, configuration, étalonnage, alimentation de l'ECU). Ses trames restent conformes à leur protection ; leur contenu ou leur présence est faux.],
  [#idt("MESSAGE/E2E")], [Le message ne passe pas le contrôle de protection de bout en bout, ou un autre émetteur utilise le même identifiant.],
  [#idt("CAN_BUS/PROTOCOL")], [Défaut de la couche CAN : trames d'erreur, acquittement, bus-off, débit.],
)

Les états observés sont rangés dans un ordre : chaque définition suppose que les précédentes ne s'appliquent pas, et la dernière se lit « Rien de ce qui précède ». L'état choisi impose la grandeur de la mesure clé et ses unités (tableau ci-dessous).

#tableau(columns: (4.7cm, 1fr, 1fr, 2.6cm), header: ([État observé], [Définition], [Grandeur imposée], [Unités]),
  [#idt("TRAMES_ABSENTES")], [Les trames n'apparaissent plus sur le bus.], [Nombre de trames de l'identifiant cible (pour une cible composant : de la trame DATA de l'ECU cible) reçues en 1,000 s.], [trames/s],
  [#idt("TRAMES_FIGÉES")], [Présentes à leur rythme, contenu et compteur identiques d'une trame à l'autre.], [Incrément du compteur entre deux trames consécutives de la trame concernée.], [sans unité],
  [#idt("PROTECTION_NON_CONFORME")], [Au moins une trame sur 100 a une protection non conforme.], [Proportion des trames concernées dont la protection est conforme, sur 100 trames consécutives.], [%, fraction],
  [#idt("SÉQUENCE_NON_CONFORME")], [Protection conforme, mais compteur discontinu ou trames en surnombre.], [Nombre de trames concernées reçues en 1,000 s.], [trames/s],
  [#idt("DÉFAUT_SIGNALÉ_PAR_LA_SOURCE")], [Trames conformes ; la source signale un défaut (statut différent de « OK » ou code d'invalidité).], [Tension continue, par rapport à la broche de masse de l'ECU source, à la broche de l'ECU source où aboutit la cible (pour un composant : reliée à sa sortie).], [V, mV],
  [#idt("VALEUR_HORS_PLAGE")], [Trames conformes, statut OK, valeur hors de la plage codable ou code d'invalidité sous statut OK.], [Valeur brute (entier) du signal concerné.], [sans unité],
  [#idt("VALEUR_INCOHÉRENTE")], [Trames conformes, statut OK, valeur dans la plage mais contraire à la redondance, à la grandeur imposée au banc ou à une autre grandeur transmise.], [Valeur physique transmise par l'ECU source pour le signal qui dépend de la cible, moyenne sur 1 s, aux conditions capturées.], [%, fraction, #box[tr/min], Hz, °, rad, bar, kPa, 0/1],
  [#idt("AUCUNE_ANOMALIE_SUR_LE_BUS")], [Rien de ce qui précède.], [Tension à la broche LV_IN de l'ECU source.], [V, mV],
)

Où chercher, et erreurs fréquentes, pour les trois premiers blocs :

- *Cible* : les désignateurs et les broches du schéma du banc, dans le Physical Lab (inspecteur et multimètre), ou le tableau « Trames reçues par le nœud étudiant » pour un identifiant. Erreurs fréquentes : désigner un composant quand le défaut porte sur une liaison ; prendre une broche voisine, ou une broche d'un autre net.
- *Catégorie* : la définition affichée sous la liste, confrontée à vos observations du bus (les trames restent-elles conformes à leur protection et à leur rythme ?) et du banc. Erreur fréquente : classer l'incident d'après le symptôme visible dans vos résultats plutôt que d'après sa cause.
- *État observé* : le tableau des trames et le détail « Trame sélectionnée » (présence et rythme des trames, compteur, protection, statut, valeur), en parcourant les états dans l'ordre de la liste. Erreur fréquente : retenir un état alors qu'un état précédent de la liste s'applique.
- *Mesure clé lue sur le bus* : comptez les trames de l'identifiant sur 1,000 s avec la colonne « t (ms) » (le champ « Filtre ID (hex) » ne garde que cet identifiant), comparez les « Données » de deux trames consécutives pour un compteur, lisez la valeur brute dans les bits du signal. Une tension se relève au multimètre du Physical Lab.

#tableau(columns: (3cm, 1fr, 0.85fr), header: ([Champ], [Sens et format exact], [Erreurs fréquentes]),
  [« Valeur »], [La valeur relevée de la grandeur imposée : nombre décimal, virgule ou point, sans unité.], [Écrire l'unité dans la valeur (`2,48 V`) ; arrondir trop tôt.],
  [« Unité »], [L'unité de la valeur, parmi celles de la grandeur imposée. L'application convertit les notations d'une même grandeur (V et mV, % et fraction…).], [Laisser le choix vide ; choisir l'unité avant l'état observé (la liste change avec lui).],
  [« Consigner la mesure »], [Pendant l'incident en cours, carte connectée et session démarrée depuis au moins 3 s (« Start ») : l'application enregistre la grandeur, les états des commandes du banc et la lecture du multimètre, et scelle les trames du bus des dernières secondes et votre circuit. Une nouvelle consignation remplace la précédente. Le même bouton existe sous le multimètre du Physical Lab.], [Consigner après avoir réparé, ou après avoir bougé un curseur : la mesure ne correspond plus à l'anomalie.],
  [« Justification »], [Votre raisonnement, en texte libre ; jamais lu par la vérification.], [—],
)

Exemple fictif complet : cible `R9.p1` – `U7.in3` (type conducteur), mesure clé `2,48` en `V`. Aucun exemple de catégorie ni d'état observé n'est donné : toute valeur pourrait être la réponse d'un incident réel. La mesure au multimètre et la consignation pas à pas sont détaillées dans la seconde partie du tutoriel.
