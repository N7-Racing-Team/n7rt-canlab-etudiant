# CAN Lab 0.3 — problèmes connus

Ces points ne bloquent aucune mission et ne faussent aucun verdict ; chacun a un contournement simple. Le [tutoriel étudiant](N7RT_CANLab_0.3_Tutoriel_Etudiant.pdf) décrit le parcours complet.

| Sujet | Ce qui se passe | Que faire |
|---|---|---|
| Bibliothèque sans PDF | L'archive ne contient aucun PDF : chaque document s'ouvre sur sa copie locale vérifiée si elle existe, sinon sur la page de l'éditeur (connexion internet). Les documents ne sont pas référencés page par page : les trouver et les lire fait partie de la mission. | Suivez la méthode du tutoriel (§4, **Bibliothèque**) ; sans connexion, demandez les copies locales à l'encadrant. |
| Fil refusé | Un fil dessiné de broche à broche part en équerre ; s'il croise un composant, une broche ou un autre fil, le Physical Lab le refuse et dit pourquoi au-dessus du schéma (« Ce fil traverserait le corps de … », « Deux fils se superposeraient… »). | Outil **Fil** : cliquez la première broche, puis un point libre à chaque coude (Retour arrière retire le dernier, / inverse le coude), puis la seconde broche. |
| Verdict long et barre d'actions | Après la qualification, la fin du verdict (« Mission clôturée. », la ligne d'export) peut se trouver sous la barre d'actions collée en bas du panneau. | Faites défiler le panneau latéral jusqu'en bas. |
| Captures du tutoriel | La figure 09 montre **Reproduire l'anomalie** assombri par la confirmation ouverte ; la figure 01 porte le libellé d'un pack de test (« E2E-… — cohorte essais ») ; les captures de la page de code datent de la version 0.2 (mêmes commandes, apparence antérieure). | Aucune action : les libellés et les commandes décrits par le texte sont ceux de l'application. |
| Documents de la version 0.2 | `missions-four-r1.md`, `missions-four-manual-acceptance.md` et `release-notes-0.2-four-missions.md` décrivent le parcours 0.2 (essai enregistré sans verdict). | Ils sont marqués « Historique » ; suivez le tutoriel 0.3. |
| Mémoire du poste | L'application et son composant WebView2 occupent environ 0,4 Go (davantage avec le Physical Lab ouvert), stable dans le temps. Sur un poste dont la mémoire est saturée par d'autres applications, la fenêtre peut rester noire ou figée. | Fermez les applications inutiles, puis relancez CAN Lab : la progression enregistrée est reprise. |
