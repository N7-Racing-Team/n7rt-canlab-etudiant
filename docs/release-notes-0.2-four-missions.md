# Release 0.2 — candidat des quatre missions

> Historique de la release 0.2 : ce document ne décrit pas la progression 0.3 (étapes E0 à E5, verdicts dans l'application). Pour la 0.3, suivez le [tutoriel étudiant](N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf).

Ce candidat ajoute les bancs WSF, SAS et BPS à APPS. Chaque mission utilise ses composants physiques, sa source ECU et le trafic CAN du véhicule. Les commandes mécaniques agissent sur le banc ; la source lit les tensions, courants, ADC, fronts et intervalles effectivement simulés.

L'IDE intégré conserve le même parcours : ouvrir `student_decoder.c` et `.h`, modifier, compiler avec le SDK validé, puis flasher la carte connectée. Le choix étudiant parmi les quatre missions reste verrouillé pour la session après redémarrage. Le mode encadrant authentifié peut ouvrir, rejouer et réinitialiser chaque banc.

Le bus mission avance la physique milliseconde par milliseconde. L'arrêt acquitte la libération de la source avant de remplacer le banc ou de restituer sa position après un nominal. Un refus de création du thread de vérification conserve l'état précédent et permet de poursuivre la session. Les historiques physiques sont bornés ; les commandes RUN/STOP/RESET de l'exploration sont refusées pendant une séance CAN qui possède l'horloge du banc.

Les règles de jugement, corrigés et vecteurs de réponse des quatre missions restent dans le dépôt encadrant restreint. Les secrets opérationnels résident dans un coffre Windows local hors Git. La distribution étudiante contient les énoncés, la bibliothèque par liens et le template public.

Les fixtures de conformité exacte (CRC, COBS, cryptographie, persistance et circuits à résultats fixes) ne sont plus distribués. Les tests publics construisent leurs entrées et vérifient des lois ou propriétés ; la comparaison octet par octet est exécutée côté encadrant sur les sources publiques courantes. Les formats, constantes et lois du contrat N7RT-Link restent documentés.

Les anciens bancs de bibliothèque version 2 restent lisibles ; ils migrent vers la version 3 lors de l'enregistrement. Le calcul RAM du SDK tient compte des frontières de sections et de l'alignement. Les scripts Windows, exclusions de caches et receipts de compilation sont renforcés pour la distribution.

Les résultats acquis et limites figurent dans [le rapport R1](missions-four-r1.md). Les quatre missions ont été recettées sur NUCLEO : chaîne complète par la liaison série avec le juge privé, puis parcours dans l'application réelle, en modes encadrant et étudiant, avec compilation et flash par l'IDE intégré. Le [protocole de recette](missions-four-manual-acceptance.md) décrit comment les rejouer. Dans le Physical Lab, le bandeau d'un banc de mission nomme désormais la mission ouverte ; il indiquait APPS pour tous les bancs.

Ce candidat n'est ni tagué ni publié en release ; les archives déjà publiées ne sont pas remplacées.
