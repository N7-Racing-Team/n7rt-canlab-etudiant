#import "n7rt-doc.typ": *

// Tutoriel étudiant 0.3, partie 2 : bus CAN, Physical Lab, code STM32, vérification, qualification E4, clôture E5,
// dépannage et FAQ. Document public : toutes les valeurs d'exemple sont fictives (trame 0x3C7, cible R9.p1 – U7.in3).

= Le bus CAN <etu2-bus>

Le trafic du bus et les résultats de votre décodeur s'observent dans la vue *Cockpit*, deuxième bouton de la barre
d'activité. Le tableau central montre les trames que reçoit le nœud étudiant, c'est-à-dire votre carte ; le panneau
de droite montre ce que votre code en a tiré. Les captures de cette section ont été prises sans mission : le trafic
est celui de l'ECU virtuel du SDK (section « Stimuli — ECU virtuel » du panneau latéral), qui émet une trame
d'exemple. La carte exécute le gabarit fourni (#link(<etu2-squelette>)[Le squelette fourni]), qui publie la
longueur de chaque trame dans le slot 0 et son premier octet dans le slot 1.

== Démarrer et arrêter une session

Une session N7RT-Link relie l'application et la carte : l'application transmet chaque trame du bus à votre nœud, la
carte exécute votre code et renvoie ce qu'il publie. Sans session, le tableau reste vide et affiche « Aucune trame
pour l'instant : démarrez une session (Start) pour voir le trafic du bus. »

#fig("etu2-bus-session", caption: [Une session ouverte sur l'ECU virtuel du SDK.],
  etapes: [
    + Vérifiez que l'état de la carte, en haut à droite de la fenêtre, est « Prête », puis cliquez *Start* #b(1). Le
      sélecteur *Tick*, juste à sa gauche, fixe la période des appels de `student_on_tick` (10 ms par défaut).
    + Pour arrêter, cliquez *Stop* #b(2). Arrêtez toujours la session avant de programmer la carte.
  ],
  observer: [
    *Start* #b(1) se grise et *Stop* #b(2) s'active. Le bloc « Session » #b(3) passe à « active » : « Durée » compte le
    temps écoulé, « Temps bus » donne le temps du bus et la période du tick. Le tableau #b(4) se remplit, la trame la
    plus récente en haut ; le champ « Filtre ID (hex) » #b(5) le restreint à un identifiant. « Résultats étudiants »
    #b(6) affiche la dernière valeur de chaque slot écrit par votre code. La barre d'état #b(7) indique « Session
    active ».
  ],
  echec: [
    *Start* reste grisé : survolez-le, la raison s'affiche (« Connectez une carte prête pour démarrer une session. »,
    « Une session est déjà en cours. »). Si l'état de la carte est « Pas de code étudiant », programmez d'abord la
    carte (#link(<etu2-code>)[Le code STM32]).
  ],
)

La ligne de compteurs au-dessus du tableau résume la session : « émises/s » et « résultats/s » (trames envoyées à la
carte et réponses reçues, par seconde), « envoyées » et « RESULT » (totaux de la session), « en vol / W » (trames
en attente de réponse et fenêtre de crédits), « A/R p50/p99 » (aller-retour d'une trame à sa réponse, en ms),
« NACK » (trames refusées par la carte) et « perdues » (trames restées sans réponse). En fonctionnement normal,
« NACK » et « perdues » restent à 0 ; survolez un compteur pour lire sa définition.

#note[*Start* et *Stop* pilotent la session avec la carte. Ils sont indépendants des boutons *RUN*, *STOP* et *RESET*
du Physical Lab, qui pilotent seulement la simulation du circuit (#link(<etu2-lab>)[Le Physical Lab]).]

Pendant une mission, le bus est celui du véhicule simulé : les stimuli manuels sont refusés (« Pendant une mission,
le bus est celui du banc : pas de trame manuelle. ») et le *Tick* est imposé par la mission (sélecteur grisé,
« Période de tick fixée par la mission »). Chaque *Start* remet à zéro la mémoire de votre code et rappelle
`student_init` : vos variables statiques reprennent leur valeur initiale.

== Lire le tableau des trames

À la largeur des captures, le tableau « Trames reçues par le nœud étudiant » affiche six colonnes :

#tableau(columns: (2.6cm, 1fr), header: ([Colonne], [Contenu]),
  [\#], [numéro de la trame dans la session, croissant ;],
  [ID], [identifiant de la trame, en hexadécimal ;],
  [DLC], [code de longueur transmis sur le bus (pour une trame classique, le nombre d'octets de données) ;],
  [Données], [les octets reçus, en hexadécimal, l'octet 0 à gauche ;],
  [Résultat], [« OK » : la carte a livré la trame à votre code et renvoyé sa réponse ; « +débordement » : votre code
    a écrit plus de 8 entrées pendant cet appel (les suivantes sont perdues) ; « +budget » : l'appel a dépassé son
    budget de temps ; « NACK », « perdue » ou « non livrée » : la trame n'a pas été traitée normalement (voyez
    l'onglet « Liaison ») ;],
  [Publications], [ce que `student_on_frame` a publié pour cette trame, sous la forme `s7=…` (une valeur) ou
    `s7=rejet` ; « — » si rien.],
)

Quand le tableau est plus large, cinq colonnes s'ajoutent : « t (ms) » (instant de réception, en millisecondes de
temps bus), « Type » (format de la trame), « A/R ms », « Cycles » (cycles processeur de votre `student_on_frame`) et
« Session ». Pour les faire apparaître, agrandissez la fenêtre ou masquez un panneau avec les boutons en haut à droite
de la fenêtre (« Panneau latéral », « Résultats »).

- *Filtrer un identifiant* : tapez-le dans « Filtre ID (hex) », avec ou sans `0x` ; seules ses trames restent. Un
  texte qui n'est pas hexadécimal est encadré en rouge. Videz le champ pour revoir tout le trafic.
- *Figer la lecture* : dès que vous faites défiler le tableau vers le bas, la vue reste immobile ; les nouvelles
  trames s'ajoutent au-dessus. Remontez tout en haut pour reprendre le défilement.
- *Débit élevé* : le titre peut indiquer « non affichées (débit > affichage) ». L'application saute alors des lignes
  à l'écran ; la carte, elle, reçoit toutes les trames.
- *Mesurer une période* : filtrez l'identifiant, puis faites la différence entre les « t (ms) » de deux trames
  successives, ou entre les « Temps bus » de deux trames cliquées (#figref("etu2-bus-trame")). Moyennez sur
  plusieurs trames : l'émission varie légèrement.

#pagebreak(weak: true)
#fig("etu2-bus-trame", caption: [Le détail d'une trame cliquée.],
  etapes: [
    + Cliquez une ligne du tableau #b(1).
  ],
  observer: [
    La ligne cliquée reste surlignée (ici la trame 36 : les trames arrivées depuis l'ont fait descendre).
    « Trame sélectionnée » #b(2) donne son numéro (« Trame »), son instant de réception (« Temps bus », en ms depuis
    le début de la session), son identifiant suivi du format (« STD » : identifiant standard de 11 bits), « DLC /
    octets », « Résultat », « Cycles », « Aller-retour », puis ses « Données » et ses « Publications ».
  ],
  echec: [
    « Cliquez une trame du tableau. » reste affiché : aucune trame n'a encore été reçue, ou le filtre n'en laisse
    passer aucune.
  ],
)

== Résultats étudiants

Le panneau de droite « Résultats étudiants » garde la dernière valeur écrite dans chaque slot depuis le début de la
session :

#tableau(columns: (2.6cm, 1fr), header: ([Colonne], [Contenu]),
  [Slot], [numéro du slot (`s0`, `s1`…) ;],
  [Valeur], [la dernière valeur publiée, ou « rejet » ;],
  [Source], [« trame … » : écrite par `student_on_frame` en traitant la trame de ce numéro ; « tick … » : écrite par
    `student_on_tick` au tick de cet instant (en ms de temps bus) ;],
  [Màj], [nombre de mises à jour du slot.],
)

Quand une mission est ouverte, un slot de la mission affiche son code et son nom, tels qu'ils figurent dans les
« Sorties à publier » de l'énoncé, et sa valeur avec son unité ; la source s'affiche au survol de la ligne.

#warn[Les vérifications ne lisent que ce que votre code publie *à chaque tick*, dans `student_on_tick` (source
« tick … »). Une valeur publiée seulement dans `student_on_frame` s'affiche ici, mais ne compte pas
(#link(<etu2-publier>)[Quand publier, quand rejeter]).]

== La console et les onglets du bas

#fig("etu2-bus-console", caption: [Les onglets du bas du Cockpit.],
  etapes: [
    + Cliquez *Console étudiant* #b(1) pour lire les lignes que votre code écrit avec `n7rt_log`.
    + Cliquez *Liaison* #b(2) ou *Diagnostic* #b(3) pour contrôler la liaison avec la carte.
    + Tapez un mot dans « Filtrer » #b(4) : seules les lignes de console qui le contiennent restent affichées.
  ],
  observer: [
    #b(1) chaque ligne de console porte son instant (temps bus) et son texte ; le nombre à côté du titre compte les
    lignes reçues (ici 1 : « student_decoder ready », écrite par le gabarit au début de la session). #b(2) les compteurs « CRC (carte) »,
    « tramage (carte) », « trous de seq (carte) », « débordements UART », « logs perdus » et « erreurs reçues (PC) »,
    puis la liste des incidents de liaison : ils concernent la liaison série entre le PC et la carte, pas la
    protection des trames de votre mission, et restent normalement à 0. #b(3) des mesures internes de l'application
    (files d'attente, délais), utiles à l'encadrant si l'affichage ralentit.
  ],
  echec: [
    Aucune ligne n'arrive : la session n'est pas démarrée (les lignes n'arrivent que pendant une session), votre code
    n'appelle pas `n7rt_log`, ou il écrit plus de 20 lignes par seconde (l'excédent est perdu et « logs perdus »
    augmente). Une ligne est limitée à 120 caractères.
  ],
)

Pendant une mission, un quatrième onglet, *Fiche*, s'ajoute à gauche de ces onglets : c'est là que s'ouvre la fiche
de l'étape en cours.

= Le Physical Lab <etu2-lab>

Le Physical Lab, troisième bouton de la barre d'activité, simule un circuit : en mission, le banc de votre véhicule ;
hors mission, un circuit de démonstration ou le vôtre. Les captures de cette section utilisent la démonstration
« Référence : potentiomètre, filtre RC et ADC », chargée hors mission avec la liste « Démonstration » et le bouton
*Charger* de la rubrique « Fichier ». Les gestes sont les mêmes sur le banc d'une mission.

== Organisation de la vue

#fig("etu2-lab-vue", caption: [Les quatre zones du Physical Lab.],
  etapes: [
    + Repérez la bibliothèque de composants #b(1), le schéma #b(2), l'inspecteur #b(3) et le multimètre #b(4).
  ],
  observer: [
    #b(1) « Bibliothèque » : les composants à poser (« Appuyez sur un composant, puis cliquez dans le schéma pour le
    poser. R le tourne, Échap annule. ») ; ne la confondez pas avec la bibliothèque de documents de la mission. Plus
    bas, la rubrique « Fichier » (*Enregistrer*, *Nouveau*, *Ouvrir*, « Démonstration »). #b(2) le schéma, sous la
    barre d'outils (#figref("etu2-lab-barre")) et une ligne de message ; dessous, les onglets « Diagnostics »
    (problèmes du circuit), « Nets » et « Journal » (événements de la simulation). #b(3) l'« Inspecteur » : les
    propriétés de ce qui est sélectionné. #b(4) le « Multimètre ».
  ],
  echec: [
    Un panneau manque : les boutons en haut à droite de la fenêtre (« Bibliothèque », « Console », « Inspecteur »)
    affichent ou masquent chaque panneau.
  ],
)

== Barre d'outils et navigation

#fig("etu2-lab-barre", caption: [La barre d'outils du schéma.],
  etapes: [
    + Choisissez l'outil : *Sélection* #b(1) (touche Échap) pour sélectionner, déplacer et régler ; *Fil* #b(2)
      (touche W) pour tracer un fil.
    + Zoomez avec *−* #b(3) et *+* #b(4) ; le bouton d'ajustement #b(5) (touche F) cadre tout le schéma.
    + Pilotez la simulation : *RUN* #b(6) la lance, *STOP* #b(7) l'arrête, *RESET* #b(8) la remet à zéro ; le temps
      simulé s'affiche à droite #b(9).
  ],
  observer: [
    Entre *Fil* #b(2) et *−* #b(3), deux flèches annulent et rétablissent la dernière modification (Ctrl+Z, Ctrl+Y).
    Entre *−* #b(3) et *+* #b(4), le pourcentage de zoom. Ici, le temps #b(9) vaut 0 : la simulation n'est pas lancée,
    et *RUN* #b(6) est en évidence.
  ],
  echec: [
    Une touche reste sans effet : cliquez d'abord dans le schéma. Un champ de saisie (inspecteur, multimètre) garde
    les touches pour lui.
  ],
)

Au clavier et à la souris :

- molette : zoom autour du pointeur ; Maj + molette : déplacement latéral ;
- bouton du milieu enfoncé, ou barre d'espace maintenue avec le bouton gauche, puis glisser : déplacer la vue ;
- F : ajuster le schéma à la vue ; 1 : zoom à 100 % ;
- R : tourner la sélection ; Ctrl+A : tout sélectionner ; Ctrl+Z et Ctrl+Y : annuler et rétablir.

*RUN*, *STOP* et *RESET* agissent sur la simulation du circuit seulement. *RESET* remet le temps à zéro et décharge
les condensateurs, sans toucher aux réglages que vous avez faits (curseurs, interrupteurs) ; si la simulation tournait,
elle continue.

== Sélectionner un composant et modifier un paramètre

#fig("etu2-lab-inspecteur", caption: [Un composant sélectionné et son inspecteur.],
  etapes: [
    + Outil *Sélection* actif, cliquez le corps du composant #b(1) dans le schéma.
    + Lisez son désignateur #b(2) et ses paramètres #b(3) dans l'inspecteur.
  ],
  observer: [
    Le composant #b(1) est surligné. L'inspecteur titre « R1 · Résistance » ; le champ « Désignateur » #b(2) porte
    son nom dans le schéma ; la rubrique « Paramètres » #b(3) liste ses valeurs avec leur unité. Plus bas :
    « Brochage » (chaque broche, son net et, hors mission, sa tension et son courant), « Mesures », « État de
    fonctionnement » et « Sources » (le modèle du composant).
  ],
  echec: [
    L'inspecteur affiche « Sélectionnez un composant ou un fil dans le schéma. » : le clic est tombé à côté du
    composant. Cliquez au milieu de son corps.
  ],
)

Pour modifier un paramètre :

+ Cliquez sa valeur dans « Paramètres » #b(3) (#figref("etu2-lab-inspecteur")) : le champ affiche la valeur exacte.
+ Tapez la nouvelle valeur, avec un point décimal ; les préfixes comme k ou m sont acceptés (par exemple `4.7k`). Le
  survol du champ indique la plage permise.
+ Appuyez sur Entrée. Échap annule la saisie ; quitter le champ la valide aussi.
+ Une valeur refusée est encadrée en rouge et la raison s'affiche sous le tableau ; corrigez-la ou appuyez sur Échap.

Une valeur notée « valeur fixe (catalogue) » ne se modifie pas. Les composants que l'on manipule (potentiomètre,
interrupteur, bouton poussoir, alimentation, carte, commandes mécaniques) ont en plus une rubrique « Commandes » :
curseur ou champ de position, case à cocher, bouton « Appuyer (maintenir) ».

#warn[Sur le banc d'une mission, chaque vérification, essai nominal compris, compare votre banc au banc de référence.
Curseurs, interrupteurs et commandes sont libres. Les liaisons et les valeurs de la rubrique « Paramètres » doivent
correspondre au banc de référence : ne modifiez une liaison ou un paramètre que si votre diagnostic le justifie, et
notez sa valeur d'origine avant de la changer. Ctrl+Z annule une modification, mais l'historique est perdu quand la
mission est rouverte.]

== Mesurer au multimètre

Le multimètre, sous l'inspecteur, est votre seul instrument de tension en mission : sur le banc d'une mission, les
tensions des broches et des nets ne sont pas affichées.

#fig("etu2-lab-multimetre", caption: [Une tension mesurée au multimètre sur la démonstration.],
  etapes: [
    + Lancez la simulation avec *RUN* #b(1) (ici, elle tourne déjà : *STOP* est actif).
    + Dans « Fonction » #b(2), choisissez « V continu ».
    + Dans « COM » #b(3) (sonde noire), choisissez la broche de référence, ici la broche négative de l'alimentation
      #b(4).
    + Dans « VΩ » #b(5) (sonde rouge), choisissez la broche mesurée, ici le curseur du potentiomètre #b(6).
    + Lisez l'afficheur #b(7).
  ],
  observer: [
    Les listes #b(3) et #b(5) nomment chaque broche « désignateur.broche » (« V1.neg », « RV1.w »). L'afficheur #b(7)
    donne la moyenne de la tension entre la sonde rouge et la sonde noire sur les 100 dernières trames de simulation :
    une ligne numérique active s'y lit à sa valeur moyenne.
  ],
  echec: [
    L'afficheur montre « — » : une sonde n'est pas placée, ou la fonction Ω est choisie sur un banc alimenté.
  ],
)

La fonction « Ω (banc hors tension) » ne mesure que sur un banc éteint, comme un vrai ohmmètre : dans l'inspecteur
(rubrique « Commandes »), décochez la sortie de chaque alimentation et l'alimentation USB de la carte ; sinon
l'afficheur montre « — ». Rallumez ensuite le banc : banc éteint, l'ECU ne transmet plus, et une consignation ou une
vérification porterait sur ce banc éteint. Pendant un incident, un bloc « Mesure clé » s'ajoute sous le multimètre,
avec le bouton « Consigner la mesure » (#link(<etu2-consigner>)[Consigner la mesure]).

== Remplacer un fil

Un fil se remplace en deux temps : le supprimer, puis en tracer un nouveau. Les figures suivantes refont le fil
qui relie l'alimentation `V1` à l'interrupteur `S1` de la démonstration.

#fig("etu2-fil-selection", caption: [Un fil sélectionné.],
  etapes: [
    + Outil *Sélection* actif, cliquez le fil #b(1).
    + Lisez ses extrémités dans l'inspecteur #b(2), puis appuyez sur la touche Suppr.
  ],
  observer: [
    Le fil #b(1) est surligné. L'inspecteur #b(2) affiche la rubrique « Fil » : son numéro, son net, sa tension (hors mission
    seulement), ses extrémités (« De », « Vers ») et son nombre de « Coudes ». Notez les deux extrémités avant de le
    supprimer.
  ],
  echec: [
    La touche Retour arrière ne supprime pas un fil sélectionné : utilisez Suppr. Si le clic sélectionne un
    composant, cliquez exactement sur le trait du fil.
  ],
)

#fig("etu2-fil-supprime", width: 75%, caption: [Le fil supprimé : ses deux broches sont libres.],
  etapes: [
    + Après Suppr, repérez les deux broches que reliait le fil : celle de l'alimentation #b(1) et celle de
      l'interrupteur #b(2).
  ],
  observer: [
    Le fil a disparu. Une broche libre se dessine comme un petit carré au bout de la patte du composant #b(1) #b(2).
  ],
  echec: [
    Supprimé par erreur : Ctrl+Z le rétablit.
  ],
)

#fig("etu2-fil-trace", width: 75%, caption: [Un fil en cours de tracé.],
  etapes: [
    + Choisissez l'outil *Fil* #b(1) (touche W).
    + Cliquez la première broche #b(2) : un segment suit le pointeur.
    + Pour chaque coude, cliquez dans le vide ; puis cliquez la broche d'arrivée #b(3).
  ],
  observer: [
    Le segment part de la broche #b(2) en équerre : la touche / inverse l'équerre du segment en cours ; Retour
    arrière retire le dernier segment ; Échap abandonne le tracé. Un clic sur la broche #b(3) termine le fil.
  ],
  echec: [
    Le fil est refusé et la raison s'affiche au-dessus du schéma, par exemple quand il traverserait le corps d'un
    composant : recommencez en cliquant un point libre à chaque coude pour le contourner.
  ],
)

#fig("etu2-fil-nouveau", width: 75%, caption: [Le nouveau fil.],
  etapes: [
    + Vérifiez le nouveau fil #b(1), puis appuyez sur Échap pour revenir à l'outil *Sélection*.
  ],
  observer: [
    Le fil #b(1) relie de nouveau les deux broches ; cliquez-le pour lire ses extrémités dans l'inspecteur
    (#figref("etu2-fil-selection")).
  ],
  echec: [
    Le fil aboutit à la mauvaise broche : sélectionnez-le, Suppr, et retracez-le.
  ],
)

== Revenir au banc de la mission

En mission, le banc de votre véhicule s'ouvre par le bouton « Ouvrir le banc » du panneau de la mission (Cockpit,
sous les « Sorties à publier »), par le troisième bouton de la barre d'activité, ou par « Banc Physical Lab » dans la
vue *Code STM32*. Au-dessus du schéma, le bandeau « Banc de la mission — mode mission : les grandeurs internes ne sont
pas affichées. » le signale : pas de tension aux broches, pas de journal ; mesurez au multimètre.

- Le banc d'une mission ne s'enregistre pas sous un autre nom (« Enregistrement indisponible en mission. ») : il est
  conservé avec votre progression.
- N'utilisez pas *Nouveau*, *Ouvrir* ni *Charger* pendant une mission : ils remplacent le banc par un autre document,
  et consigner, reproduire l'anomalie ou vérifier sont alors refusés (« Le banc de la mission n'est plus ouvert dans
  le Physical Lab »). Pour retrouver votre banc tel que vous l'aviez laissé, cliquez *Fermer* sous la mission
  (panneau latéral du Cockpit), saisissez le code du groupe, puis *Ouvrir*.
- Pendant une vérification, le banc est en lecture seule (« Banc en lecture seule pendant l'essai. »).

== Consigner la mesure <etu2-consigner>

Pendant un incident (E1 à E3), la fiche impose une grandeur à relever, la *mesure clé*. La consigner, c'est faire
enregistrer par l'application la preuve de votre mesure. Le bouton « Consigner la mesure » existe à deux endroits qui
font la même chose : dans la fiche, section « Mesure clé », et sous le multimètre, bloc « Mesure clé ». Une fiche
d'incident ouverte est enregistrée d'abord.

Conditions : un incident en cours, non validé, hors vérification ; la carte connectée ; une session démarrée
(*Start*, barre du haut) ; le bouton *RUN* du Physical Lab ne suffit pas. L'application n'impose pas de durée
minimale : elle refuse seulement s'il n'y a encore aucune trame de la session en cours. Attendez pourtant au moins
3 s après *Start* : la consignation scelle les trames des dernières secondes du bus, et une mesure calculée sur trop
peu de trames est jugée fausse.

#fig("etu2-consigner", width: 60%, caption: [La mesure clé consignée (contenu de la fiche masqué).],
  etapes: [
    + Pendant que l'anomalie se manifeste, relevez la grandeur imposée, sans toucher aux curseurs.
    + Cliquez « Consigner la mesure » #b(1).
    + Saisissez la valeur relevée dans « Valeur », choisissez l'unité, puis *Enregistrer la fiche*.
  ],
  observer: [
    « Mesure consignée le … » #b(2) donne la date de la consignation (« Mesure non consignée. » tant qu'aucune ne
    l'est). L'application a enregistré la grandeur imposée et les conditions (états des commandes du banc et, pour
    une tension, lecture du multimètre), et scellé les trames du bus des dernières secondes et votre circuit : la
    vérification juge votre valeur sur ces conditions-là.
  ],
  echec: [
    « Aucune trame récente sur le bus : lancez le véhicule, puis consignez la mesure. » : cliquez *Start*, attendez
    au moins 3 s, consignez de nouveau. « La mesure ne peut être consignée que pendant l'incident en cours, hors
    vérification. » : attendez la fin de l'essai ; hors d'un incident, il n'y a rien à consigner.
  ],
)

Consignez *avant* de réparer : la mesure doit montrer l'anomalie. Une nouvelle consignation remplace la précédente.
Exemple fictif : vous soupçonnez le conducteur R9.p1 – U7.in3 et l'état observé impose une tension ; vous relevez
2,48 V au multimètre pendant que l'anomalie se manifeste, vous consignez, puis vous saisissez `2,48` dans
« Valeur » et choisissez `V` dans « Unité ». La valeur s'écrit sans unité, avec une virgule ou un point.

== Reproduire l'anomalie

Vous avez réparé avant d'avoir mesuré, ou votre mesure ne montre plus rien ? Remettez le véhicule dans l'état où il
est revenu d'essai.

#fig("etu2-reproduire", width: 75%, caption: [Le bouton « Reproduire l'anomalie », sous la consigne d'un incident.],
  etapes: [
    + Dans le panneau de la mission (Cockpit), sous la consigne de l'incident, cliquez « Reproduire l'anomalie » #b(1).
  ],
  observer: [
    Le bouton #b(1) n'existe que pendant un incident non validé ; il est grisé pendant une vérification.
  ],
  echec: [
    Un refus s'affiche sous le bouton de vérification (« n'existe que pendant un incident non validé, hors
    vérification. ») : l'étape en cours n'est pas un incident, ou un essai tourne.
  ],
)

#fig("etu2-reproduire-dialogue", width: 80%, caption: [La confirmation de « Reproduire l'anomalie ».],
  etapes: [
    + Lisez la confirmation, puis cliquez « Reproduire » #b(1), ou « Annuler » #b(2) pour ne rien changer.
  ],
  observer: [
    Après « Reproduire » #b(1), le véhicule revient dans l'état où il est revenu d'essai ; vos modifications du
    circuit sont conservées et l'anomalie est de nouveau présente jusqu'à sa réparation. Refaites la mesure,
    consignez-la, réparez de nouveau, puis vérifiez. Chaque reproduction figure dans la fiche finale.
  ],
  echec: [
    La confirmation ne s'ouvre pas : le bouton était grisé (vérification en cours).
  ],
)

= Le code STM32 <etu2-code>

La vue *Code STM32*, premier bouton de la barre d'activité, édite, compile et programme votre décodeur sans
STM32CubeIDE. Les captures de cette section montrent le gabarit fourni, reproduit en texte plus loin
(#link(<etu2-squelette>)[Le squelette fourni]).

== La page de code

#fig("etu2-ide-vue", caption: [La page de code, avec le gabarit fourni.],
  etapes: [
    + Dans l'explorateur, cliquez le fichier à modifier #b(1).
    + Modifiez-le dans l'éditeur #b(2) ; cliquez une fonction dans « Plan du code » #b(3) pour aller à sa ligne.
    + Suivez la ligne d'état #b(4), qui annonce l'étape suivante.
    + Pour revenir au Cockpit ou au banc, cliquez « Mission et résultats CAN » #b(5) ou « Banc Physical Lab » #b(6).
  ],
  observer: [
    #b(1) les deux fichiers du projet, `student_decoder.c` et `student_decoder.h` ; un point signale un fichier
    modifié. #b(3) les trois fonctions du gabarit, `student_init`, `student_on_frame` et `student_on_tick`
    (#link(<etu2-squelette>)[Le squelette fourni]). #b(4) ici « Projet enregistré. Compiler → Flasher → Mission et résultats CAN. » #b(7) la position du
    curseur (« Ln 1, Col 1 »), suivie, à droite, de la révision de la source et de son état (enregistrée ou
    modifiée). Sous l'éditeur, les onglets « Console » et « Problèmes ».
  ],
  echec: [
    La page reste sur « Chargement du projet… » ou affiche une erreur : voyez la #link(<etu2-faq>)[FAQ de démarrage].
  ],
)

« Parcours du TP », dans l'explorateur, rappelle l'enchaînement : modifier le code, compiler et corriger les erreurs,
flasher la carte, ouvrir le Cockpit et lancer l'essai. « Raccourcis » liste les touches de l'éditeur : Ctrl+S
enregistre, Ctrl+F recherche, Ctrl+H remplace, Ctrl+Z et Ctrl+Y annulent et rétablissent, Tab indente, Ctrl+Espace
propose une complétion des fonctions du SDK. Vos deux fichiers sont enregistrés dans le profil local de l'application,
sur ce poste, et non dans le dossier décompressé.

#fig("etu2-ide-entete", caption: [Le fichier `student_decoder.h` du gabarit.],
  etapes: [
    + Cliquez l'onglet `student_decoder.h` #b(1) au-dessus de l'éditeur.
  ],
  observer: [
    L'éditeur #b(2) montre l'en-tête : vos déclarations (constantes, fonctions d'aide) et l'inclusion de `n7rt.h`,
    l'API publique du SDK. Le plan du code est vide pour un en-tête sans fonction.
  ],
  echec: [
    Les deux onglets ont un point de modification après un essai malheureux : *Restaurer le template*
    (#figref("etu2-ide-boutons")) revient aux fichiers de départ.
  ],
)

== Les boutons

#fig("etu2-ide-boutons", caption: [Les boutons de la page de code.],
  etapes: [
    + *Enregistrer* #b(1) (Ctrl+S) enregistre les deux fichiers ; il n'est actif qu'après une modification.
    + *Compiler* #b(2) enregistre, puis compile la source enregistrée.
    + *Flasher* #b(3) programme la carte connectée avec la dernière compilation.
    + *Compiler + flasher* #b(4) enchaîne l'enregistrement, la compilation, la programmation et le contrôle de la
      reconnexion.
    + *Restaurer le template* #b(5) remplace les deux fichiers par leur version de départ, après confirmation.
  ],
  observer: [
    Un bouton grisé donne sa raison au survol. Ici, *Enregistrer* #b(1) et *Flasher* #b(3) sont grisés : rien n'est
    modifié, et rien n'a encore été compilé dans cette session.
  ],
  echec: [
    *Flasher* #b(3) reste grisé : survolez-le (« Compilez les modifications avant de flasher. », « Arrêtez la session
    et attendez la fin de l'essai avant de flasher. », « Connectez une carte avec un runtime compatible et vérifié. »,
    « La carte doit être prête avant le flash. », « Une opération est déjà en cours. »).
  ],
)

*Restaurer le template* ouvre la confirmation « Restaurer le template » : « Remplacer les deux fichiers étudiants par
leur version de départ ? Le code présent sur la carte ne change pas. » Le bouton « Restaurer les deux fichiers »
confirme, « Annuler » renonce. Copiez votre travail ailleurs avant de restaurer, puis recompilez avant de programmer la carte.

== Compiler et corriger une erreur

*Compiler* écrit d'abord « Compilation de la source enregistrée… » dans l'onglet « Console », puis le résultat. Une compilation
réussie se termine par une ligne de la forme `CRC étudiant 0x… · … octets de flash · … octets de RAM`. Une compilation
qui échoue affiche `Compilation refusée.` suivi des diagnostics du compilateur.

#fig("etu2-ide-erreur", width: 85%, caption: [Une compilation échouée et l'onglet « Problèmes ».],
  etapes: [
    + Lisez la ligne d'état #b(1).
    + Ouvrez l'onglet « Problèmes » #b(2) et cliquez le premier diagnostic #b(3) : l'éditeur va au fichier et à la
      ligne concernés.
    + Corrigez, enregistrez, recompilez.
  ],
  observer: [
    #b(1) « Compilation échouée. Corrigez les problèmes indiqués. » #b(2) le nombre de diagnostics, ici 4. #b(3)
    chaque ligne se lit `fichier:ligne:colonne · error · message` (ou `warning`). #b(4) dans l'éditeur, un point
    rouge marque chaque ligne en cause et le mot fautif est souligné : ici un mot `oops` ajouté exprès à la ligne 14
    du gabarit, qui entraîne aussi les trois erreurs suivantes.
  ],
  echec: [
    Corrigez d'abord la première erreur : les suivantes en découlent souvent. Pour découvrir l'onglet sans risque,
    ajoutez `#error N7RT_TEST_DIAGNOSTIC` à la fin de `student_decoder.c`, compilez, lisez le diagnostic, puis
    retirez la ligne.
  ],
)

Une compilation réussie remplit la rubrique « Compilation » du panneau de droite :

#fig("etu2-ide-compile", width: 70%, caption: [La rubrique « Compilation » après une compilation réussie.],
  etapes: [
    + Après *Compiler*, lisez la rubrique « Compilation » : « Source », « Flash étudiant » #b(1), « RAM », « CRC
      compilé » #b(2).
  ],
  observer: [
    « Source » donne la révision compilée (`r3` : numéro de révision de la source). « Flash étudiant » #b(1) et, juste
    dessous, « RAM » donnent la taille de votre programme en mémoire. « CRC compilé » #b(2) est l'empreinte de cette
    compilation : la carte devra afficher la même après le flash. *Flasher* devient actif.
  ],
  echec: [
    Les champs restent à « — » : la compilation a échoué ; lisez « Problèmes ».
  ],
)

== Flasher la carte et contrôler le CRC

Avant de programmer : carte connectée, runtime vérifié et compatible (état « Prête », « Pas de code étudiant » ou
« Code étudiant désactivé »), session arrêtée (*Stop*), aucune vérification en cours. Cliquez *Flasher*, ou *Compiler + flasher*. L'application libère le port série pour le programmateur, écrit
seulement la zone étudiante de la mémoire (le runtime N7RT est contrôlé avant et après, et n'est jamais réécrit),
puis reconnecte et vérifie la carte.

#fig("etu2-ide-flash", caption: [La programmation en cours.],
  etapes: [
    + Cliquez *Flasher* #b(1), puis attendez sans débrancher la carte.
  ],
  observer: [
    La ligne d'état #b(2) affiche « Programmation de la zone étudiante… », puis « Reconnexion et vérification de la
    carte… ». Pendant la programmation, l'« État » de la carte #b(3) passe à `DISCONNECTED` et tous les boutons sont
    grisés, *Flasher* #b(1) compris.
  ],
  echec: [
    « Programmation ou vérification échouée. Consultez la console. » : lisez « Console » ; débranchez et rebranchez
    la carte, connectez-la dans le Cockpit, puis recommencez.
  ],
)

#fig("etu2-ide-flashe", width: 70%, caption: [La carte reconnectée exécute la compilation actuelle.],
  etapes: [
    + Comparez le « CRC compilé » #b(1) et le « CRC étudiant » lu sur la carte #b(2).
    + Lisez la phrase de contrôle #b(3).
  ],
  observer: [
    #b(4) « État » revient à `READY` ; #b(5) « Intégrité » indique « Vérifiée » (runtime contrôlé). Les deux CRC
    #b(1) #b(2) sont égaux et #b(3) affiche « La carte exécute la compilation actuelle. »
  ],
  echec: [
    #b(3) affiche « Le code sur la carte diffère de la source actuelle. » : la carte n'exécute pas votre dernière
    source ; compilez et flashez. « Connectez la carte pour vérifier le code chargé. » : la carte n'est pas
    connectée.
  ],
)

Le CRC de la zone étudiante, relu sur la carte après chaque programmation, est enregistré avec chaque vérification.
Après un redémarrage de l'application, *Compiler* est de nouveau nécessaire pour disposer d'une compilation validée,
même si la carte exécute déjà votre programme. Ce CRC identifie votre programme ; il n'a rien à voir avec le CRC qui
protège certaines trames de votre mission, que votre code contrôle lui-même
(#link(<etu2-crc>)[Protection : le principe du CRC]).

== Le squelette fourni <etu2-squelette>

Vous ne modifiez que `student_decoder.c` et `student_decoder.h`. Le dossier `n7rt/` du projet (en-tête `n7rt.h`,
runtime précompilé, script de liaison) est vérifié à chaque compilation et sur la carte : n'y touchez pas. Voici le
gabarit fourni :

#src("/firmware/n7rt_student/N7RT_CANLab_Student/student/student_decoder.c", 1, 32)

Ce gabarit est un test d'intégration du banc : il ne décode rien. À chaque trame, il publie sa longueur dans le
slot 0 et son premier octet dans le slot 1 ; il ne publie rien aux ticks. Il ne passe donc aucune vérification de
mission.

Le runtime appelle trois fonctions, toujours hors interruption et jamais en même temps :

#tableau(columns: (5.2cm, 1fr), header: ([Fonction], [Quand, et pour quoi faire]),
  [`void student_init(void)`], [au démarrage de la carte et au début de chaque session (*Start*) : remettre votre
    état à zéro, écrire un message de démarrage ;],
  [`void student_on_frame(const n7rt_can_frame_t *frame)`], [pour chaque trame reçue par votre nœud : la filtrer,
    la contrôler, la décoder et mémoriser ce qu'elle apporte ;],
  [`void student_on_tick(uint64_t now_us)`], [à chaque tick de la session, `now_us` étant l'instant du tick en µs
    de temps bus (un multiple de la période du tick) : publier ou rejeter chaque slot de la mission.],
)

La trame reçue et les services du runtime sont déclarés dans `n7rt.h` :

#src("/firmware/n7rt_student/N7RT_CANLab_Student/n7rt/include/n7rt.h", 25, 46)

- `timestamp_us` : instant de réception, en µs de temps bus depuis le début de la session ; `id` : identifiant
  (11 bits, ou 29 bits si le drapeau `N7RT_CAN_EXT` est levé) ; `dlc` : code de longueur transmis ; `len` : nombre
  d'octets valides dans `data` ; `flags` : drapeaux `N7RT_CAN_EXT` (identifiant étendu), `N7RT_CAN_RTR` (trame de
  requête), `N7RT_CAN_FD`, `N7RT_CAN_BRS`, `N7RT_CAN_ESI` ; `data` : les octets, `data[0]` en premier. Les trames
  d'erreur du bus ne sont jamais livrées à votre code.
- `n7rt_publish(slot, valeur)` publie une valeur physique valide pour un slot ; `n7rt_reject(slot)` déclare qu'il n'y
  a pas de valeur exploitable pour ce slot ; `n7rt_now_us()` rend le temps du bus ; `n7rt_log(texte)` écrit une ligne
  dans la console (au plus 120 caractères, 20 lignes par seconde).
- Un *slot* est une sortie numérotée définie par l'énoncé (« Sorties à publier ») : numéro, code, nom et unité.
  Publiez la valeur dans l'unité de l'énoncé. Un appel écrit au plus 8 entrées (publications et rejets confondus) ;
  au-delà, elles sont perdues (« +débordement » dans le tableau des trames).

#note[Votre code tourne sur le même microcontrôleur que le runtime. Un appel qui ne rend pas la main (boucle infinie)
affiche « Code étudiant occupé » ; au-delà de 2 s, le chien de garde réinitialise la carte. Après une faute, la carte
passe à « Code étudiant désactivé » jusqu'au prochain *Start* ou à une nouvelle image. Le budget de temps par trame est
de 200 µs : un dépassement est signalé (« +budget ») sans être sanctionné.]

== Les règles de compilation de l'IDE

L'éditeur compile votre code avec la chaîne GNU Arm, en C (GNU11), sans optimisation (`-O0`), avec les avertissements
`-Wall -Wextra` (ils s'affichent dans « Problèmes »). Il refuse une source qui sort de ces règles :

- `#include` seulement de `student_decoder.h`, `n7rt.h` ou d'un en-tête C standard : `assert.h`, `ctype.h`,
  `errno.h`, `float.h`, `inttypes.h`, `limits.h`, `math.h`, `stdarg.h`, `stdbool.h`, `stddef.h`, `stdint.h`,
  `stdio.h`, `stdlib.h`, `string.h`, `time.h` ;
- `#pragma once` est le seul `#pragma` accepté ; `#include_next`, `#embed` et `#import` sont refusés ;
- pas d'assembleur (`asm`, `__asm__`), ni de `_Pragma`, de `__has_include` ou de collage de symboles `##` ;
- pas de trigraphe (`??`, `%:`) hors des chaînes et des commentaires ;
- chaque fichier est un texte UTF-8 de 256 Kio au plus.

La bibliothèque C fournie n'affiche pas `%f` : écrivez vos valeurs en entiers (par exemple en millièmes). Évitez
`malloc` (la réserve de tas est de 8 Kio). Votre programme dispose de 768 Kio de flash et de 128 Kio de RAM ; une
image trop grande est refusée à la compilation.

== Décoder une trame : méthodes générales

Le SDK ne fournit aucune fonction de décodage : vous les écrivez. Les fragments qui suivent sont génériques et
s'appuient sur une trame *fictive*, qui n'existe dans aucune mission ; vos propres valeurs viennent de votre
documentation et de vos mesures.

#tableau(columns: (5.4cm, 1fr), header: ([Élément fictif], [Description]),
  [Trame], [identifiant `0x3C7`, DLC 6, période 50 ms, protégée ;],
  [CRC de protection], [INTEL, LSB 40, longueur 8 ;],
  [Compteur de protection], [INTEL, LSB 36, longueur 4 ;],
  [Statut], [INTEL, LSB 6, longueur 2 ; code « OK » : 2 ;],
  [Signal _température liquide (exemple)_], [MOTOROLA, LSB 22, longueur 10, non signé, facteur 0,25, offset −40
    (°C), code invalide 1023, slot 7 ;],
  [Supervision], [premier timeout 300 ms, timeout 150 ms, péremption 90 ms, 2 pertes consécutives tolérées.],
)

=== Numéroter les bits

Le bit _i_ de la trame (de 0 à 63) est le bit (_i_ mod 8) de l'octet ⌊_i_/8⌋ ; l'octet 0 est transmis en premier et
le bit 0 est le poids faible de l'octet. Le *LSB* d'un champ est la position de son bit de poids faible. En INTEL,
les bits suivants du champ sont _i_+1, _i_+2… ; en MOTOROLA, ils remontent l'octet, puis continuent au bit 0 de
l'octet précédent. Le tableau ci-dessous place les champs de la trame fictive :

#let etu2-champ(i) = if i == 6 or i == 7 { "S" } else if (i >= 8 and i <= 15) or i == 22 or i == 23 { "T" } else if i >= 36 and i <= 39 { "K" } else if i >= 40 and i <= 47 { "C" } else { none }
#let etu2-teinte = (S: rgb("#dbe7f6"), T: rgb("#fbe7b5"), K: rgb("#d9f0dd"), C: rgb("#f8d9d9"))
#figure(kind: table, supplement: [Tableau], caption: [Position des bits de la trame fictive `0x3C7` (S : statut,
  T : signal _température liquide (exemple)_, K : compteur, C : CRC ; case blanche : bit inutilisé).],
  table(columns: (1.6cm,) + (1fr,) * 8, align: center + horizon, stroke: 0.4pt + luma(200), inset: 4pt,
    table.header([*Octet*], ..range(7, -1, step: -1).map(b => [*bit #b*])),
    ..range(6).map(o => ([*#o*], ..range(7, -1, step: -1).map(b => {
      let i = 8 * o + b
      let t = etu2-champ(i)
      table.cell(fill: if t == none { white } else { etu2-teinte.at(t) },
        [#text(size: 8pt, str(i))#if t != none { h(3pt) + strong(t) }])
    }))).flatten(),
  ))

Lecture du signal fictif : MOTOROLA, LSB 22, longueur 10. Ses deux bits de poids faible sont les bits 22 et 23
(bits 6 et 7 de l'octet 2) ; il continue au bit 0 de l'octet précédent et prend tout l'octet 1 (bits 8 à 15). Sa
valeur brute vaut donc `(data[1] << 2) | (data[2] >> 6)`. Le compteur fictif (INTEL, LSB 36, longueur 4) est le
quartet haut de l'octet 4, `data[4] >> 4` ; le CRC fictif (INTEL, LSB 40, longueur 8) est l'octet 5 entier.

=== Lire un champ INTEL ou MOTOROLA

Deux fonctions générales suffisent pour tous les champs ; vérifiez d'abord que la trame contient assez d'octets
(`frame->len`), pour ne jamais lire au-delà des octets reçus.

#code(```c
#include <stdint.h>

/* Champ INTEL : lsb = position du bit de poids faible (0 à 63), len = longueur (1 à 32). */
static uint32_t lire_intel(const uint8_t *d, unsigned lsb, unsigned len)
{
  uint32_t v = 0u;
  for (unsigned i = 0u; i < len; i++) {
    unsigned bit = lsb + i;                      /* INTEL : lsb, lsb + 1, lsb + 2... */
    v |= (uint32_t)((d[bit / 8u] >> (bit % 8u)) & 1u) << i;
  }
  return v;
}

/* Champ MOTOROLA repéré par son bit de poids faible. */
static uint32_t lire_motorola(const uint8_t *d, unsigned lsb, unsigned len)
{
  uint32_t v = 0u;
  unsigned bit = lsb;
  for (unsigned i = 0u; i < len; i++) {
    v |= (uint32_t)((d[bit / 8u] >> (bit % 8u)) & 1u) << i;
    if (bit % 8u == 7u) {
      bit -= 15u;                                /* fin de l'octet : bit 0 de l'octet précédent */
    } else {
      bit++;                                     /* bit suivant du même octet */
    }
  }
  return v;
}
```.text)

Sur la trame fictive :

- `lire_motorola(frame->data, 22u, 10u)` rend la valeur brute du signal ;
- `lire_intel(frame->data, 36u, 4u)` rend la valeur du compteur.

=== Signe : le complément à deux

Un champ déclaré non signé se lit tel quel. Un champ en complément à deux est négatif quand son bit de poids fort
vaut 1 : on lui retire alors 2#super[_n_], _n_ étant sa longueur. Sur 10 bits, la valeur brute 1023 vaut −1 et la
valeur brute 512 vaut −512.

#code(```c
/* Complément à deux sur len bits (1 à 31). */
static int32_t etendre_signe(uint32_t brut, unsigned len)
{
  if ((brut & (1u << (len - 1u))) != 0u) {
    return (int32_t)((int64_t)brut - ((int64_t)1 << len));
  }
  return (int32_t)brut;
}
```.text)

=== Facteur, offset et code invalide

La valeur physique vaut *valeur brute × facteur + offset*. Avec le signal fictif (facteur 0,25, offset −40 °C), la
valeur brute 300 donne 300 × 0,25 − 40 = 35 °C. Un *code invalide* (ou SNA) est une valeur brute réservée par la
source pour dire _pas de valeur_ : elle ne se convertit pas. Ce que votre décodeur en fait, rejeter ou publier,
est votre décision de conception DD-1, tirée de l'énoncé et du règlement.

#code(```c
#include <stdbool.h>

#define SLOT_TEMP      7u        /* slot fictif du signal d'exemple */
#define TEMP_INVALIDE  1023u     /* code invalide fictif */

static float temp_c;             /* dernière valeur physique décodée */
static bool  temp_code_invalide; /* true : la source a envoyé le code invalide */

/* Dans student_on_frame, une fois la trame acceptée : */
static void decoder_temperature(const uint8_t *d)
{
  uint32_t brut = lire_motorola(d, 22u, 10u);   /* MOTOROLA, LSB 22, longueur 10 */
  temp_code_invalide = (brut == TEMP_INVALIDE);  /* la conduite à tenir : votre décision DD-1 */
  if (!temp_code_invalide) {
    temp_c = (float)brut * 0.25f - 40.0f;       /* valeur brute x facteur + offset */
  }
}
```.text)

=== Statut

Un champ de statut dit si la source considère sa donnée comme bonne. Avec le statut fictif (INTEL, LSB 6,
longueur 2, code « OK » : 2), `lire_intel(d, 6u, 2u) == 2u` signifie que la source n'annonce aucun défaut. La
conduite à tenir pour un autre code vient de l'énoncé.

=== Protection : le principe du CRC <etu2-crc>

L'émetteur d'une trame protégée calcule un CRC sur les octets de la trame et le place dans le champ CRC. Le
récepteur refait le même calcul sur les mêmes octets et compare : si les deux diffèrent, la trame n'est pas
conforme (votre décision DD-2 dit ce que vous faites de ses données). Le calcul exact
(polynôme, valeur de départ, octets couverts, valeur finale et tout ce que la spécification fait entrer dans le
calcul) est défini par la spécification de protection que cite votre mission : cherchez-la dans la bibliothèque.

#code(```c
/* Principe seulement : crc_selon_spec() est à écrire d'après la spécification de protection. */
uint8_t crc_selon_spec(const uint8_t *d, uint8_t len);

static bool protection_conforme(const uint8_t *d, uint8_t len)
{
  uint8_t recu = (uint8_t)lire_intel(d, 40u, 8u);    /* CRC fictif : INTEL, LSB 40, longueur 8 */
  return recu == crc_selon_spec(d, len);
}
```.text)

=== Compteur et continuité

Un compteur de protection augmente d'une trame à la suivante et revient à 0 après sa valeur maximale (15 pour
4 bits). L'écart entre deux trames reçues se calcule modulo 2#super[_n_] : un écart nul signale une trame répétée,
un écart supérieur à 1 des trames manquantes. L'écart toléré vient de la spécification de protection.

#code(```c
/* Écart entre deux valeurs d'un compteur de 4 bits (0 à 15, puis retour à 0). */
static unsigned ecart_compteur(unsigned precedent, unsigned actuel)
{
  return (actuel - precedent) & 0x0Fu;
}
```.text)

=== Fraîcheur et timeout

Le temps se mesure avec le temps du bus : `frame->timestamp_us` dans `student_on_frame`, `now_us` dans
`student_on_tick`. Mémorisez l'instant de la dernière trame valide, puis calculez son âge à chaque tick. Les seuils
de la fiche (premier timeout avant la première trame valide de la session, timeout entre deux trames valides,
péremption, âge au-delà duquel une donnée reçue n'est plus considérée comme fraîche, pertes consécutives
tolérées) et la conduite à
tenir viennent de votre énoncé ; le fragment ci-dessous ne montre que la mesure d'un âge, avec la péremption
fictive.

#code(```c
#define PEREMPTION_US  90000u    /* péremption fictive : 90 ms */

static uint64_t derniere_us;     /* temps bus de la dernière trame valide */
static bool     deja_recue;      /* au moins une trame valide depuis le début de la session */

/* Dans student_on_frame, pour une trame que vos décisions retiennent : */
static void noter_reception(const n7rt_can_frame_t *frame)
{
  derniere_us = frame->timestamp_us;
  deja_recue = true;
}

/* Dans student_on_tick : l'âge de la dernière trame est-il dans la péremption ? */
static bool donnee_fraiche(uint64_t now_us)
{
  return deja_recue && (now_us - derniere_us) <= PEREMPTION_US;
}
```.text)

=== Quand publier, quand rejeter <etu2-publier>

Les vérifications comparent, tick après tick, ce que votre carte publie dans `student_on_tick` à ce qu'attend
l'essai caché. Répartissez donc le travail ainsi :

+ dans `student_on_frame` : ignorez les trames qui ne vous concernent pas (autre identifiant, trame étendue
  `N7RT_CAN_EXT` ou de requête `N7RT_CAN_RTR`, trop courte) ; contrôlez la protection, le compteur et le statut ;
  décodez ; mémorisez la valeur, sa validité et l'instant de réception ;
+ dans `student_on_tick` : écrivez *chaque* slot de la mission, une fois par tick : `n7rt_publish` ou
  `n7rt_reject`, selon l'état de la donnée et vos décisions DD-1 à DD-4 ;
+ ne publiez pas les sorties de la mission dans `student_on_frame` : la vérification ne les lit pas. Pour déboguer,
  écrivez plutôt dans la console avec `n7rt_log`.

#code(```c
void student_on_frame(const n7rt_can_frame_t *frame)
{
  if (frame->id != 0x3C7u || (frame->flags & (N7RT_CAN_EXT | N7RT_CAN_RTR)) != 0u) {
    return;                      /* pas la trame attendue */
  }
  if (frame->len < 6u) {
    return;                      /* jamais lire au-delà des octets reçus */
  }
  bool conforme = protection_conforme(frame->data, frame->len);
  /* compteur et statut ici ; trame non conforme : appliquez votre DD-2 */
  memoriser(frame, conforme);    /* à écrire d'après votre fiche : décodage, validité, instant */
}

void student_on_tick(uint64_t now_us)
{
  if (publiable(now_us)) {       /* à écrire : état mémorisé, DD-1 à DD-4, supervision */
    n7rt_publish(SLOT_TEMP, temp_c);   /* une valeur physique, dans l'unité de l'énoncé */
  } else {
    n7rt_reject(SLOT_TEMP);
  }
  /* ... une entrée, et une seule, pour chacun des autres slots de la mission ... */
}
```.text)

Ce squelette ne tranche aucune question de la mission : quelle trame accepter quand sa longueur est inattendue, que
faire d'une protection non conforme, d'un compteur discontinu ou d'un statut en défaut, combien de temps garder une
valeur. Ces choix figurent dans votre fiche (décisions DD-1 à DD-4, supervision) : les décisions se justifient par l'énoncé,
le règlement et les spécifications de la bibliothèque ; les valeurs de supervision sont celles que votre encadrant
vous a communiquées au lancement de l'E0.

=== Déboguer avec `n7rt_log`

`n7rt_log` écrit au plus 120 caractères par ligne et 20 lignes par seconde ; au-delà, les lignes sont perdues.
Écrivez un décompte ou une moyenne par bloc de trames, jamais une ligne par trame, et des entiers plutôt que des
nombres à virgule.

#code(```c
#include <stdio.h>

static unsigned nb_trames;

/* Dans student_on_frame, une fois la valeur brute lue : */
static void journaliser(uint32_t brut)
{
  nb_trames++;
  if (nb_trames == 100u) {               /* une ligne par bloc de 100 trames */
    char ligne[64];
    snprintf(ligne, sizeof ligne, "temp brute : %lu", (unsigned long)brut);
    n7rt_log(ligne);
    nb_trames = 0u;
  }
}
```.text)

= Vérifier une étape <etu2-verif>

Chaque étape se termine par une vérification que l'application conduit seule : votre fiche contre la vérité scellée
dans votre pack, votre banc contre le banc de référence, votre carte contre un essai caché. Le texte libre des
justifications n'entre jamais dans un verdict.

== Quand le bouton est actif

Le bouton de vérification se trouve sous la consigne, dans le panneau de la mission (Cockpit). Son libellé dépend de
l'étape : « Lancer la vérification » (E0), « Vérifier la réparation » (E1 à E3), « Lancer la qualification » (E4).

#fig("etu2-verif-bouton", width: 55%, caption: [Le bouton de vérification de l'essai nominal.],
  etapes: [
    + Repérez l'étape en cours #b(1) et relisez sa consigne #b(2).
    + Quand votre code est programmé et votre fiche prête, cliquez « Lancer la vérification » #b(3).
  ],
  observer: [
    #b(1) l'étape en cours est « EN COURS » ; la dernière ligne donne l'état de la fiche. #b(2) la consigne de
    l'étape. #b(3) le bouton, actif ici. Une fiche ouverte est enregistrée avant la vérification (« La fiche sera
    enregistrée avant la vérification. »).
  ],
  echec: [
    Le bouton est grisé : le message affiché dessous dit pourquoi (liste ci-dessous).
  ],
)

Quand le bouton est grisé, le message affiché dessous en donne la raison :

#tableau(columns: (1fr, 1fr), header: ([Message], [Que faire]),
  [« Complétez la fiche de cette étape avant la vérification. Champs à renseigner : … »], [Renseignez les champs
    listés, puis enregistrez : la liste porte sur la fiche *enregistrée*.],
  [« Corrigez les champs signalés dans la fiche. »], [Un champ est mal formé (encadré en rouge dans la fiche) :
    corrigez-le.],
  [« Connectez la carte et vérifiez que votre programme est chargé. »], [Branchez et connectez la carte, attendez
    « Prête », programmez-la si besoin.],
  [« Prochaine vérification possible dans … min … s. »], [Patientez : 2 minutes entre deux vérifications pour E0 à
    E3, 5 minutes pour E4. Reprenez vos mesures en attendant.],
  [« Vérification déjà en cours. »], [Attendez le verdict.],
)

== Pendant l'essai

#fig("etu2-verif-encours", caption: [Un essai en cours : bus et résultats masqués.],
  etapes: [
    + Ne touchez à rien : ne débranchez pas la carte, ne fermez pas l'application, attendez le verdict.
  ],
  observer: [
    #b(1) le temps restant (« Essai en cours — 7 s. Ne débranchez pas la carte et ne fermez pas l'application. »),
    sous une barre de progression. #b(2) la vue du bus est masquée (« Essai en cours : la vue du bus est masquée
    jusqu'au verdict. ») ; les résultats, à droite, aussi.
  ],
  echec: [
    Carte débranchée ou application fermée : « Vérification interrompue (…). Elle n'est pas comptée. Relancez quand
    le banc est prêt. » Rebranchez la carte ou rouvrez l'application, puis relancez la vérification.
  ],
)

Pendant l'essai, la carte et la session sont réservées (*Start*, *Stop*, *Connecter* et le choix du port sont
grisés : « Vérification en cours : la carte et la session sont réservées à l'essai. »), la fiche est en lecture seule
(« Vérification en cours : la fiche n'est pas modifiable. ») et le banc aussi. Une interruption n'est pas comptée ;
au-delà de six interruptions d'une même étape dans l'heure, une interruption déclenche aussi le délai entre deux
vérifications.

== Lire le verdict

#fig("etu2-verif-echec", width: 50%, caption: [Le verdict d'un essai nominal en échec.],
  etapes: [
    + Lisez les lignes du verdict dans l'ordre : #b(1), puis #b(2) et #b(3), puis #b(4).
  ],
  observer: [
    #b(1) « Échec à corriger. » #b(2) « Fiche nominale : non conforme ». #b(3) « Essai nominal : réussi ». #b(4) « Le
    détail n'est pas communiqué. Reprenez vos mesures et vos essais. » L'étape passe à « ÉCHEC À CORRIGER » et le
    délai avant la vérification suivante s'affiche sous le bouton.
  ],
  echec: [
    Ici, la carte a réussi l'essai mais la fiche contient au moins une erreur : c'est la fiche qu'il faut reprendre,
    champ par champ, depuis vos sources (#link(<etu2-echec>)[Que faire quand ça échoue ?]).
  ],
)

Chaque ligne dit *où* chercher, jamais *quoi* corriger :

#tableau(columns: (5.6cm, 1fr, 1fr), header: ([Ligne], [Ce qu'elle dit], [Ce qu'elle ne dit pas]),
  [« Vérification réussie. Étape validée le … »], [l'étape est validée, à cette date ; la ligne suivante annonce
    l'étape qui s'ouvre], [—],
  [« Échec à corriger. »], [au moins un contrôle de l'étape a échoué], [lequel],
  [« Fiche nominale : conforme / non conforme » (E0)], [si tous les champs contrôlés de la partie nominale sont
    justes], [quel champ est faux, ni combien],
  [« Essai nominal : réussi / non réussi » (E0)], [si votre carte a passé l'essai caché, sur un banc identique au
    banc de référence], [quel cas a échoué],
  [« Diagnostic (fiche) : conforme / non conforme » (E1 à E3)], [si cible, catégorie, état observé et mesure clé
    sont justes et pertinents], [lequel est faux],
  [« Réparation (essai caché) : vérifiée / non vérifiée » (E1 à E3)], [si votre circuit est identique au banc de
    référence, l'anomalie réellement supprimée, et si votre carte passe un essai caché qui rejoue l'anomalie
    réparée], [ce qui manque],
  [« Le détail n'est pas communiqué. Reprenez vos mesures et vos essais. »], [un rappel : la réponse ne viendra pas
    de l'application], [—],
)

#fig("etu2-verif-reussite", width: 45%, caption: [Le verdict d'un essai nominal réussi.],
  etapes: [
    + Lisez la première ligne #b(1), puis la suivante #b(2).
    + Contrôlez la liste des étapes #b(3).
  ],
  observer: [
    #b(1) « Vérification réussie. Étape validée le … » avec la date ; #b(2) « Incident 1 ouvert. Le véhicule revient
    d'essai avec une anomalie. » #b(3) E0 passe à « VALIDÉ » avec sa date, E1 à « EN COURS ». Sous le bouton, la
    liste des champs à renseigner concerne déjà l'étape suivante.
  ],
  echec: [
    Une étape validée ne se rouvre pas : ses champs contrôlés se figent ; la justification, les pages et les noms des
    signaux restent modifiables.
  ],
)

= La qualification E4 <etu2-e4>

La qualification finale s'ouvre quand l'incident 3 est validé (« Qualification finale ouverte. »). Sa consigne :
« Qualification finale. Une séquence d'essais cachée évalue votre code. Le résultat est donné par compétence. » Le
véhicule n'a plus d'anomalie et la fiche n'a pas de partie propre à E4 : seule votre carte est évaluée.

#fig("etu2-e4-ouverte", width: 50%, caption: [La qualification finale ouverte.],
  etapes: [
    + Vérifiez que E0 à E3 sont validés #b(1) et lisez la consigne #b(2).
    + Programmez la version finale de votre décodeur et contrôlez le CRC (#figref("etu2-ide-flashe")).
    + Cliquez « Lancer la qualification » #b(3).
  ],
  observer: [
    #b(1) E4 « Qualification finale » est « EN COURS ». #b(3) le bouton est actif.
  ],
  echec: [
    Le bouton est grisé : lisez le message dessous (carte, délai), comme pour les autres étapes.
  ],
)

#fig("etu2-e4-encours", caption: [Une qualification en cours.],
  etapes: [
    + Attendez la fin, carte branchée, application ouverte.
  ],
  observer: [
    #b(1) le temps restant (« Qualification en cours — … s. »), jusqu'à la fin de la séquence d'essais ; #b(2) la
    vue du bus et, à droite, les résultats sont masqués jusqu'au verdict. Ici, E4 affiche encore
    « ÉCHEC À CORRIGER », d'une qualification précédente.
  ],
  echec: [
    Interruption (carte débranchée, application fermée) : la qualification n'est pas comptée ; relancez-la.
  ],
)

#fig("etu2-e4-resultat", width: 50%, caption: [Le résultat d'une qualification en échec, une ligne par compétence.],
  etapes: [
    + Lisez les huit lignes de compétence, dont la première #b(1), et repérez celles en FAIL #b(2).
    + Contrôlez l'état de E4 #b(3) et le délai affiché sous le bouton #b(4).
  ],
  observer: [
    #b(1) chaque ligne porte un badge PASS ou FAIL, puis « CP-n — compétence » ; aucune note ni détail des essais.
    #b(2) une ligne en FAIL suffit : #b(3) E4 passe à « ÉCHEC À CORRIGER » et #b(4) la qualification suivante est
    possible après 5 minutes. Les huit en PASS valident E4 et clôturent la mission : « Mission clôturée. », puis
    « La fiche finale est exportée dans le dossier Documents. » (#figref("etu2-e5-cloture")).
  ],
  echec: [
    Une ligne en FAIL : reprenez la famille de situations que nomme la compétence (ci-dessous), corrigez, flashez,
    puis relancez la qualification une fois le délai écoulé.
  ],
)

Les huit compétences, toujours dans cet ordre :

#tableau(columns: (1.6cm, 1fr), header: ([Ligne], [Compétence]),
  [CP-1], [Recevoir et filtrer les trames de l'ECU],
  [CP-2], [Décoder les valeurs nominales],
  [CP-3], [Respecter les limites et les valeurs invalides],
  [CP-4], [Contrôler l'intégrité des messages],
  [CP-5], [Superviser fraîcheur et séquence],
  [CP-6], [Traiter un défaut signalé par la source],
  [CP-7], [Appliquer la logique de sécurité de la mission],
  [CP-8], [Reprendre après un défaut],
)

Après un FAIL, relisez l'intitulé de la compétence : il désigne une famille de situations (trames à ignorer, valeurs
invalides, protection, fraîcheur et séquence, défaut annoncé par la source, logique de sécurité de l'énoncé, retour à
la normale). Reproduisez ces situations sur le banc et sur le bus, observez « Résultats étudiants » et vos lignes de
console, corrigez votre code, compilez, flashez, contrôlez le CRC, puis relancez la qualification une fois le délai
écoulé. Les huit compétences sont réévaluées à chaque qualification.

= Clôture E5 et export <etu2-e5>

Quand la mission est clôturée, la fiche finale est exportée automatiquement et le panneau de la mission passe en
« E5 · Clôture ».

#fig("etu2-e5-cloture", width: 40%, caption: [Une mission clôturée.],
  etapes: [
    + Contrôlez les étapes #b(1), la consigne #b(2) et le verdict #b(3).
    + Cliquez « Ouvrir la fiche finale » #b(4).
  ],
  observer: [
    #b(1) les étapes sont « VALIDÉ » (ici E2 à E4 ; E0 et E1, plus haut dans la liste, aussi) et la ligne *Fiche*
    indique « Exportée le … ». #b(2) « Mission clôturée.
    Votre fiche finale est disponible. » #b(3) les huit compétences, puis « Mission clôturée. » et « La fiche finale
    est exportée dans le dossier Documents. » #b(4) ouvre `fiche.html` dans votre navigateur. La barre d'état, en
    bas de la fenêtre, affiche « E5 · Clôture ».
  ],
  echec: [
    L'export a échoué : le panneau affiche « Exportation de la fiche finale impossible » et le bouton « Réessayer
    l'export ». Causes signalées : « Dossier Documents introuvable. », « La fiche finale ne peut pas être écrite dans
    le dossier Documents. » Libérez de la place ou corrigez les droits, puis réessayez ; si cela persiste,
    prévenez l'encadrant.
  ],
)

L'export crée un dossier par clôture :

#tableau(columns: (5.4cm, 1fr), header: ([Emplacement], [Contenu]),
  [`Documents\N7RT CAN Lab\fiches\`], [un dossier `<mission>-<groupe>-<AAAAMMJJ-HHMM>`, nommé d'après la mission,
    le groupe et l'instant de la clôture ;],
  [`fiche.html`], [la fiche finale lisible, celle qu'ouvre « Ouvrir la fiche finale » ;],
  [`fiche.json`], [les mêmes données, lisibles par un programme ; leur empreinte SHA-256 figure au pied de
    `fiche.html` ;],
  [`scellés\`], [les fichiers scellés : la fiche finale et ceux de votre progression.],
)

La fiche finale reprend vos parties de fiche, vos mesures consignées, les réparations enregistrées par l'application
(CRC de la carte, modifications du circuit), les compétences, toutes vos tentatives, vos reproductions de l'anomalie
et le temps actif.

#fig("etu2-e5-lecture", caption: [La fiche après la clôture, en lecture seule (contenu masqué).],
  etapes: [
    + Pour relire votre fiche dans l'application, cliquez « Ouvrir la fiche » : elle s'ouvre dans l'onglet *Fiche*.
  ],
  observer: [
    #b(1) la note « La mission est clôturée : la fiche n'est plus modifiable. » Tous les champs sont figés.
  ],
  echec: [
    Vous voulez corriger un champ après la clôture : ce n'est plus possible ; parlez-en à l'encadrant.
  ],
)

*À remettre* : si votre encadrant le demande, le dossier d'export entier, sans rien y modifier ; l'empreinte
SHA-256 et les fichiers scellés permettent de contrôler qu'il est intact.

= Que faire quand ça échoue ? <etu2-echec>

L'application ne dit jamais ce qui est faux : un verdict désigne une partie du travail (fiche, code, banc), un
message désigne une condition à remplir. Changer des valeurs au hasard coûte une vérification toutes les 2 minutes
(5 en E4) et n'apprend rien. Reprenez plutôt la démarche : *observer* (bus, résultats, console, banc), *formuler* une
hypothèse, *mesurer ou tester* ce qui la confirme ou l'infirme, *corriger une seule chose*, puis *vérifier*.

#tableau(columns: (4.1cm, 2.3cm, 1fr), header: ([Ce que vous voyez], [Où chercher], [Premières vérifications]),
  [Bouton de vérification grisé], [message sous le bouton], [Lisez-le : champs manquants (enregistrez la fiche),
    carte, délai, champ mal formé.],
  [« Fiche nominale : non conforme »], [fiche], [Reprenez chaque champ depuis sa source : bus (identifiant, DLC,
    période moyennée sur plusieurs trames), documents (positions, ordre, signe, facteur, offset, code invalide,
    protection), énoncé et règlement (décisions DD-1 à DD-4), valeurs données par l'encadrant au lancement de l'E0
    (supervision). Contrôlez la numérotation des bits.],
  [« Essai nominal : non réussi »], [code, carte], [La carte exécute-t-elle la compilation actuelle (CRC) ? Chaque
    slot est-il écrit à chaque tick, valeur ou rejet, dans la bonne unité ? Testez vos cas limites sur le banc et
    lisez vos lignes de console.],
  [« Diagnostic (fiche) : non conforme »], [fiche d'incident], [Cible (les bonnes broches ou le bon composant),
    catégorie (relisez sa définition), état observé (les états se lisent dans l'ordre de la liste : retenez le
    premier dont la définition s'applique), mesure clé consignée pendant l'anomalie, dans une unité acceptée.],
  [« Réparation (essai caché) : non vérifiée »], [banc, code], [L'anomalie a-t-elle réellement disparu (refaites la
    mesure) ? Les « Paramètres » et les liaisons sont-ils ceux d'origine ? Votre code traite-t-il la situation
    réparée ?],
  [Une ligne « CP-n — compétence » en FAIL (E4)], [code], [Reproduisez la famille de situations que nomme la compétence ; observez vos
    publications à chaque tick.],
  [Rien dans « Résultats étudiants »], [session, code], [Session démarrée ? Carte « Prête » avec votre programme ?
    Publiez-vous dans `student_on_tick` ?],
  [Aucune trame au tableau], [session, filtre], [Cliquez *Start* ; videz « Filtre ID (hex) ».],
)

Pièges fréquents :

- *Mesure consignée après la réparation* : la mesure ne montre plus l'anomalie. Cliquez « Reproduire l'anomalie »,
  remesurez, consignez, puis réparez de nouveau.
- *Fiche non enregistrée* : « Modifications non enregistrées » est affiché à côté du bouton d'enregistrement ; rien
  ne s'enregistre tout seul. Cliquez *Enregistrer la fiche* avant de vérifier.
- *Code non flashé* : « Le code sur la carte diffère de la source actuelle. » ; compilez, flashez, contrôlez le CRC.
- *Session arrêtée* : pas de trames, pas de consignation (« Aucune trame récente sur le bus : lancez le véhicule,
  puis consignez la mesure. »). Cliquez *Start* et attendez quelques secondes.
- *Carte dans un mauvais état* : « Pas de code étudiant », « Code étudiant désactivé », « Liaison perdue » ; voyez
  la #link(<etu2-faq>)[FAQ de démarrage].
- *Banc remplacé* : *Nouveau*, *Ouvrir* ou *Charger* ont remplacé le banc de la mission ; fermez puis rouvrez la
  mission avec le code du groupe.
- *Paramètre modifié par erreur* : le banc ne correspond plus au banc de référence ; rétablissez la valeur d'origine
  (Ctrl+Z, ou la valeur que vous aviez notée).
- *Publications dans `student_on_frame` seulement* : elles s'affichent avec la source « trame … » mais ne comptent pas.

Le délai entre deux vérifications est de 2 minutes pour E0 à E3 et de 5 minutes pour E4, compté depuis le dernier
verdict de l'étape. Mettez-le à profit : relisez vos sources, mesurez, testez votre code sur le banc.

= FAQ de démarrage <etu2-faq>

*L'état de la carte reste « Déconnecté ».* Branchez la NUCLEO par son port ST-LINK, choisissez son port dans *CARTE*,
puis cliquez *Connecter*. Le bouton « Rechercher les cartes », à droite de la liste, relit les ports disponibles.

*L'état affiche « Carte absente ».* La carte n'est pas détectée (« Carte … non détectée : vérifiez le câble USB
(connecteur ST-LINK CN1). »). Vérifiez le câble et le connecteur, essayez un autre port USB, puis recherchez les
cartes.

*L'état affiche « Port occupé ».* Un autre programme utilise le port série : console série de STM32CubeIDE, autre
instance du CAN Lab, moniteur série. Fermez-le : la connexion reprend seule.

*L'état reste sur « Poignée de main ».* L'application attend la réponse du runtime de la carte. Patientez quelques
secondes ; si rien ne change, débranchez et rebranchez la carte.

*L'état affiche « Pas de code étudiant ».* La carte n'a pas de programme étudiant valide (« Runtime vérifié, mais … :
compilez et flashez votre projet depuis STM32CubeIDE. ») et *Start* reste grisé. Programmez-la depuis la vue *Code
STM32* (*Compiler + flasher*) : STM32CubeIDE n'est pas nécessaire.

*L'état affiche « Code étudiant désactivé ».* Votre code a provoqué une faute ou bloqué la carte (« Code étudiant
désactivé après une faute : corrigez-le, ou démarrez une session pour le réactiver. »). Cherchez un accès hors
tableau ou une boucle sans fin, corrigez, compilez et flashez ; *Start* le réactive aussi.

*« Carte muette » ou « Code étudiant occupé » s'affiche en haut.* « Carte muette » : la carte ne répond plus (« Carte
muette depuis 750 ms (cœur arrêté par un point d'arrêt ?) : envois suspendus. »). « Code étudiant occupé » : un appel
de votre code ne rend pas la main (« Le code étudiant s'exécute depuis … ms (boucle infinie ?). »). Cherchez une
boucle trop longue dans votre code.

*L'état affiche « Carte incompatible ».* Le runtime de la carte ne correspond pas au SDK de l'application. Seul un
encadrant peut le réinstaller : prévenez-le.

*L'état affiche « Liaison perdue ».* Le câble a été débranché ou la carte a redémarré (« Liaison perdue sur …
(carte débranchée ?) : reconnexion automatique dès son retour… »). Rebranchez : la reconnexion est automatique.

*La compilation est impossible.* Le message _Outil de compilation indisponible : installez les prérequis du projet
étudiant._, ou une console qui signale `arm-none-eabi-gcc not found`, indique un outil manquant :

+ installez Python 3.11 ou plus, la chaîne GNU Arm et STM32CubeProgrammer ; STM32CubeIDE ou STM32CubeCLT,
  installés dans leur dossier par défaut `C:\ST`, conviennent aussi ;
+ ouvrez PowerShell dans le dossier décompressé et lancez : \
  `python -m pip install -r .\requirements-student-ide.txt`
+ relancez l'application.

*Windows bloque le lancement.* L'exécutable n'est pas signé : SmartScreen affiche « Windows a protégé votre
ordinateur ». Cliquez « Informations complémentaires », puis « Exécuter quand même ».

*La fenêtre reste blanche.* Le composant WebView2 manque : installez « Microsoft Edge WebView2 Runtime », puis
relancez.

*Le pack est refusé.* « Ce fichier n'est pas un pack de groupe N7RT. » : vous avez choisi un autre fichier ; prenez
le fichier `.n7rtpack` de votre groupe. « Ce pack est destiné à une autre mission. » : le pack ne correspond pas à la
mission choisie. « Pack du groupe illisible : demandez un nouveau fichier à votre encadrant. » : le fichier est
abîmé.

*« Code de groupe incorrect. »* Ressaisissez le code reçu de l'encadrant, sans espace en trop. Le code n'est écrit
nulle part dans le pack : demandez-le à l'encadrant si vous l'avez perdu.

*« La mission de cette session est déjà choisie : … »* Le choix de la mission est définitif pour la session (« Ce
choix est définitif pour la session : pour en changer, demandez à votre encadrant. »). Demandez à l'encadrant.

*La fermeture demande « Fiche non enregistrée ».* La fiche contient des modifications non enregistrées : « Enregistrer
et fermer » les garde, « Fermer sans enregistrer » les abandonne, « Revenir à la mission » annule la fermeture. Pour
du code non enregistré, la même question propose « Revenir à l'application ».

*Le bouton de vérification reste grisé.* Lisez le message dessous (#link(<etu2-verif>)[Vérifier une étape]) :
champs manquants dans la fiche enregistrée, champ mal formé, carte non connectée ou sans programme, délai entre deux
vérifications.

*Le bouton Flasher reste grisé.* Survolez-le : il faut une compilation de la source actuelle, une carte connectée
dont le runtime est compatible et vérifié (état « Prête », « Pas de code étudiant » ou « Code étudiant désactivé »,
sonde ST-LINK identifiée), et aucune session ni vérification en cours (#figref("etu2-ide-boutons")).

*Le panneau de code affiche « Le code sur la carte diffère de la source actuelle. »* La carte exécute une autre
version que votre source : compilez et flashez.

*« Progression illisible sur ce poste : demandez à l'encadrant de réinitialiser la mission. »* Prévenez
l'encadrant.

*Où sont mes fichiers ?* Le code est enregistré dans le profil local de l'application, sur ce poste ; la fiche et la
progression aussi. La fiche finale est exportée dans `Documents\N7RT CAN Lab\fiches\` à la clôture.
