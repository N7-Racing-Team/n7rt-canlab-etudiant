# Design system N7RT CAN Lab

Un **outil technique N7RT**, pas un tableau de bord web violet : thème sombre, géométrie carrée, densité d'IDE, couleur au service de l'information.

**Sources.**
- **Code** : `styles/tokens.css` (valeurs), `styles/base.css` (polices, garde globale), `styles/components.css` (composants). Aucune valeur brute ailleurs.
- **Garde** : `npm run check` (`scripts/check-ui.mjs`), également lancé par la suite du projet.
- **Aperçus** : [docs/img/can-lab-ui/](../../docs/img/can-lab-ui/).

## 1. Règles non négociables

**Géométrie.**
- Aucun coin arrondi : `border-radius` vaut 0 partout. La garde globale de `base.css` l'impose aussi aux contrôles natifs, aux barres de défilement et aux pseudo-éléments.
- La vérification automatique échoue sur :
  - tout rayon non nul (CSS, `rx`/`ry` SVG, `borderRadius` en TS) ;
  - toute ombre ;
  - toute `@keyframes` ;
  - toute transition de plus de 150 ms ;
  - l'absence de la garde globale.
- Panneaux adjacents séparés par des lignes de 1 px (`--line`), pas de cartes flottantes. Pas de pastilles, chips, boutons capsule ou avatars ronds.

**Couleur et effets.**
- Pas d'ombre, de lueur, de flou, de glassmorphism ni de 3D. La hiérarchie vient des fonds (`--bg-*`), des lignes et du contraste.
- Le dégradé N7RT (`--n7-gradient`, celui du logo) n'apparaît que sur le **bloc de marque** de la barre d'outils.
- Le violet sert la hiérarchie et l'action : bouton principal, sélection, onglet ou vue active, focus, session en cours. Jamais en aplat décoratif.

**Mouvement.** Aucune animation continue. Seules les transitions d'état de contrôle sont permises, en 80 ms (`--t-fast`).

L'éditeur CodeMirror utilise des styles contrôlés par le nonce CSP de Tauri. Les règles `animation: none` désactivent ses animations ; la garde les accepte, mais refuse les animations actives. Les sources C restent du texte, jamais du HTML.

**Contenu.** Pas d'emoji, pas d'illustration. Les icônes sont filaires : le sprite SVG de `index.html` pour les éléments écrits dans `index.html`, les masques CSS `.ico` (§5) pour les contrôles créés en TypeScript. Mêmes règles pour les deux : boîte de 16 px, trait de 1,5, extrémités carrées, angles vifs.

## 2. Couleurs

Elles sont prélevées sur le logo N7RT (`drive/…/N7RTrond.png`) :
- violet `#592783` ;
- indigo `#2C2E84` → `#383E90` ;
- bleu tricolore `#0F70B7` ;
- rouge tricolore `#E72F2A`.

Le guide d'édition N7RT (LaTeX) ne fixe ni palette d'interface ni police : celles-ci sont dérivées ici.

| Usage | Jeton | Valeur |
|---|---|---|
| Fond de fenêtre, barre d'activité | `--bg-app` | `#0D0E12` |
| Panneaux, espace de travail | `--bg-panel` | `#14151A` |
| En-têtes de panneau, barre d'outils, en-tête de tableau, barre d'état | `--bg-head` | `#191A20` |
| Champs de saisie | `--bg-field` | `#0F1015` |
| Survol / pression | `--bg-hover` / `--bg-active` | `#23252E` / `#2B2E39` |
| Barre de défilement survolée | `--bg-scroll-hover` | `#3A3E4B` |
| Séparateurs / bordures de boutons et contour désactivé / lignes de tableau | `--line` / `--line-strong` / `--line-row` | `#24262F` / `#343846` / `#1B1D24` |
| Bordure de bouton survolé | `--line-hover` | `#454A5A` |
| Bordures de champs, nœuds d'étape vides, barre d'action de la progression / survol d'un champ | `--line-field` / `--line-field-hover` | `#6B7180` / `#7D8391` |
| Texte fort / secondaire / faible | `--text` / `--text-2` / `--text-3` | `#E4E6EB` / `#A3A8B4` / `#8A909C` |
| Accent : bordure, marqueur actif | `--accent` | `#7A4BC0` |
| Bouton principal (violet N7RT tel quel) | `--accent-fill` | `#592783` |
| Texte violet sur fond sombre, focus | `--accent-text`, `--focus` | `#B794E8` |
| Sélection | `--accent-sel` | `#261D3A` |
| Succès / alerte / erreur / information | `--ok` / `--warn` / `--err` / `--info` | `#3DBE7A` / `#E0A93B` / `#F25A56` / `#4A9BE0` |

Chaque état a aussi un fond (`--*-bg`) et une ligne (`--*-line`) assortis, réservés aux badges et aux encarts.

**Contrastes sur `--bg-panel`** (texte : 4,5:1 au moins ; limites de contrôle : 3:1 au moins, WCAG 1.4.11) :
- `--text` : 14:1 ;
- `--text-2` : 7,5:1 ;
- `--accent-text` : 7:1 ;
- `--text-3` : 5,7:1 (5,4:1 sur `--bg-head`, 4,8:1 sur `--bg-hover`) : unités, horodatages, codes, libellés désactivés ; jamais une donnée essentielle seule, et jamais sur `--bg-active` (4,2:1) ;
- `--err` : 5,5:1 (4,9:1 sur `--bg-raised`) ; `--sch-invalid` lui reste égal ;
- `--line-field` : 3,7:1, et au moins 3,1:1 sur toutes les surfaces (`--bg-hover` compris) : c'est la limite visible d'un champ vide ;
- `--line-strong` : 1,6:1, donc décoratif seulement (séparations, boutons identifiés par leur libellé, contour d'un contrôle désactivé).

**Pourquoi ces jetons.**
- `--text-3` passe de `#6C7280` (3,8:1) à `#8A909C` : les étiquettes de clé/valeur, les en-têtes de tableau et les onglets inactifs restaient sous 4,5:1.
- `--err` passe de `#EF4B47` à `#F25A56` : l'ancien rouge tombait à 4,5:1 sur les boutons neutres (Stop).
- `--line-field` et `--line-field-hover` donnent aux champs une bordure visible (l'ancienne `--line-strong` faisait 1,6:1).
- `--line-hover` et `--bg-scroll-hover` remplacent les deux valeurs brutes qui restaient dans `components.css` et `base.css`.
- `--h-step` (28 px) et `--node` (12 px) sont la ligne et le nœud du parcours de l'étudiant.

## 3. Typographie

- **IBM Plex Sans** pour l'interface, **IBM Plex Mono** pour les identifiants CAN, charges utiles, horodatages, valeurs, logs et compteurs.
  - Une famille d'ingénierie, sous licence OFL, embarquée (`fonts/`, sous-ensemble latin, environ 110 Ko).
  - Rien n'est chargé depuis le réseau.
  - Les sous-ensembles latins n'ont pas les symboles d'unité et de calcul (Ω ∞ ≤ ≥ → ∆ ≈ ≠ µ ± °). Une police complémentaire, **N7RT Symbols Sans/Mono** (environ 2,4 Ko par graisse), les fournit. Elle vient juste après Plex dans `--font-ui` et `--font-mono`, si bien que le navigateur ne lui prend que ces glyphes.
    - C'est un sous-ensemble de l'IBM Plex officiel, renommé parce que la licence OFL réserve le nom « Plex » aux versions non modifiées (note dans `fonts/OFL-IBM-Plex.txt`).
    - Ω s'écrit U+03A9 (forme canonique) ou U+2126 : les deux points de code sont couverts.
- **Trois tailles seulement** :
  - `--fs-s` 11 px : titres de panneau en capitales, en-têtes de tableau, unités, étiquettes d'état ;
  - `--fs-m` 12 px : corps, contrôles, tableaux, console ;
  - `--fs-l` 13 px : marque, valeurs de métriques.
- Pas de titre géant. Exception réservée aux instruments du Physical Lab : `.readout`, 22 px.
- **Chiffres tabulaires** partout (`font-variant-numeric: tabular-nums`) : les nombres s'alignent verticalement. Les colonnes numériques sont alignées à droite (`.num`).

## 4. Espacement, grille, dimensions

| Élément | Valeur |
|---|---|
| Espacements | 2 / 4 / 6 / 8 / 12 / 16 px (`--sp-1…6`) |
| Bordures | toujours `--bw` = 1 px |
| Barre d'outils | 36 px |
| En-tête de panneau, onglets | 26 px |
| Contrôles | 24 px |
| Lignes de tableau et de console | 20 px |
| Barre d'état | 22 px |
| Barre d'activité | 40 px |

**Coque (IDE).**
- Barre d'outils, puis : barre d'activité | panneau latéral | espace de travail (métriques, trames, dock à onglets) | panneau de résultats.
- Barre d'état en bas. Avec une progression ouverte, un segment (`.sb-progress`) donne l'étape en cours (« E1 · Incident 1 — EN COURS ») et ramène au panneau de progression.
- Les panneaux latéraux et la console se masquent (boutons de la barre d'outils) et se redimensionnent (séparateurs). La disposition est mémorisée.
- **Panneau latéral du cockpit en mission** : la progression en haut (parcours, consigne, verdict, barre d'action collante), puis la description de la mission et la bibliothèque.
- **Dock du cockpit** : onglets Fiche, Console étudiant, Liaison, Diagnostic. L'onglet **Fiche** (`#tab-fiche`) n'apparaît qu'avec une progression ouverte, et un onglet masqué n'est jamais choisi. Il est plus haut que les autres : 60 % de l'espace de travail par défaut, dans des limites qui suivent la fenêtre ; sa hauteur redimensionnée est mémorisée à part (`Layout.heights`), les autres onglets gardent la hauteur commune (`Layout.bottom`).
- Le tableau des trames se replie seul quand l'espace manque. Par requêtes de conteneur, les colonnes s'effacent dans cet ordre : session, cycles, A/R, type, t.
- Aucun défilement horizontal global.

## 5. Composants

| Composant | Classe | Règle |
|---|---|---|
| Bouton | `.btn` | Neutre par défaut. `.btn--primary` (violet) : **une** action principale par groupe (Connecter, Start). `.btn--danger` : arrêt ou action qui interrompt (Stop). `.btn--icon` : icône seule, avec `aria-label`. `.btn--ghost` : bascules de panneau. `.btn--add` : « Ajouter … » sous le dernier bloc, fantôme (sans fond ni contour), « + » et texte `--accent-text` ; les tirets restent réservés au désactivé |
| Champ / liste / texte long | `.input` / `.select` / `.textarea` | Fond `--bg-field`, bordure `--line-field` (3,7:1), survol `--line-field-hover`. Focus : bordure et contour de 1 px `--focus`, 2 px en tout. `.invalid` : bordure et fond d'erreur. `.textarea` : 72 px au moins, police de l'interface, redimensionnable en hauteur |
| Contrôle créé en TypeScript | `el("button", "btn …")` | Tout contrôle porte sa classe du système de design à côté de sa classe sémantique ; `npm run check` refuse un contrôle sans elle. `color-scheme: dark` (`base.css`) reste le filet de sécurité des contrôles natifs |
| Icône de contrôle | `.ico` + `.ico--play` / `--doc` / `--plus` / `--undo` / `--meter` | Masque CSS dans la couleur du texte, mêmes règles que le sprite (16 px, trait 1,5, extrémités carrées, angles vifs) |
| Case / curseur | `.check` / `.range` | Carrés, sans contrôle natif arrondi ; case bordée de `--line-field` |
| Badge d'état | `.badge` + `--ok/warn/err/info/accent` | Rectangulaire, capitales 11 px, carré de couleur de 6 px |
| Section repliable | `details.section` | En-tête 26 px en capitales, triangle carré |
| Tableau | `.grid-table` | Mono 12 px ; lignes de 20 px séparées de 1 px ; survol `--bg-hover` ; sélection `--accent-sel` + filet violet à gauche ; en-tête collant ; largeurs fixes (`table-layout: fixed`) ; ellipses, valeur complète dans le détail |
| Console | `ol.console` | Grille horodatage / type / message, mono, sans bulles ; filtre dans la barre d'onglets |
| Onglets | `.tabs` / `.tab` | Onglet actif : fond panneau + filet violet de 2 px en haut. Le compteur passe en rouge s'il reste des erreurs non vues |
| Barre d'état | `.statusbar` | Neutre. Seuls le carré d'état et le texte prennent la couleur de l'état |
| Métriques | `.metrics` / `.metric` | Valeur mono 13 px, étiquette 11 px ; une valeur anormale non nulle passe en rouge (`.nonzero`) |
| Barre de l'espace de travail (Physical Lab) | `.lab-bar` | 36 px, groupes `.tool-group` : outils Sélection / Fil, annuler, rétablir, zoom (−, %, +, ajuster en icône `.btn--icon`), « Simulation : RUN / STOP / RESET » en texte (distincts du Start de la session STM32 ; RUN primaire, STOP danger), temps simulé mono à largeur fixe. Le badge `.badge--warn` « ralenti » est au bout droit de la ligne de message, juste sous la barre : il apparaît sans changer les rangées de la barre ni la hauteur de la ligne. Aucun contrôle ni libellé n'est masqué : sur un espace étroit, les marges se resserrent, puis des groupes entiers passent sur une seconde rangée de 36 px |
| Élément de bibliothèque | `.lab-item` | Ligne de 24 px : nom, préfixe mono en `--text-3`. Armé : `--accent-sel` + filet violet de 2 px. Un appui arme le placement, sans glisser-déposer HTML5 |
| Grille de paramètres | `.grid-table.lab-params` | Nom \| valeur \| unité. Éditeur à la hauteur de la ligne, bordure au survol et au focus ; plage en infobulle ; le texte tapé part tel quel au cœur (Entrée ou sortie du champ, Échap annule). Refus : `.input.invalid` et message sous la grille |
| Lecture en mots | `.lab-word` / `.lab-none` | « flottant », « sans référence », « indéfini », « circuit ouvert », « saturé (4095) » en `--warn` ; « — » en `--text-3`, motif en infobulle. Jamais un nombre pour un état invalide |
| Bouton maintenu | `.btn.lab-hold` | `aria-pressed="true"` tant qu'il est tenu (fond `--bg-active`, bordure `--accent`) ; relâché au `pointerup`, `pointercancel`, perte de focus ou changement de vue |
| Console virtualisée | `.lab-vlist` / `.lab-row` | Lignes de 20 px positionnées dans un espaceur, recyclées, écrites par `textContent` au rAF ; colonnes fixes ; sélection `--accent-sel` + filet |
| Confirmation | `.btn--danger` | Abandonner des modifications, écraser un fichier ou supprimer un bloc de la fiche (« Confirmer la suppression ») : le bouton devient danger et pose la question ; un second clic confirme, 5 s au plus |

### Progression et fiche (mission 0.3)

`src/views/progress.ts`, `src/views/incident.ts`, `src/views/form.ts`. Ces vues tournent aussi dans le faux DOM de `progress.test.ts` : chaînes `className`, `textContent`, propriétés ARIA reflétées et `dataset` seulement ; le focus et le défilement passent par la fonction `reveal` injectée.

| Composant | Classe | Règle |
|---|---|---|
| Parcours | `.progress-steps` / `.step` | Liste verticale : une ligne de `--h-step` par étape (nœud de `--node` \| nom \| état, la date sous le nom), puis la ligne « Fiche ». `role="listitem"`, `aria-current="step"` sur l'étape en cours. En cours : `--accent-sel` et filet violet ; validée : coche carrée verte ; à corriger : ambre (`.step-failed`), jamais rouge. Aucun compteur |
| Consigne | `.progress-brief` | En-tête « Consigne » et code d'étape (« E1 · Incident 1 »), la consigne telle quelle, sélectionnable ; « Reproduire l'anomalie » dans son pied |
| Verdict | `.progress-feedback` + `.is-ok/warn/info/err` | Un bloc, filet de 2 px et fond selon le ton ; un `p.feedback-line` par ligne du cœur, texte inchangé, `data-kind` (head, part, hint, cp, closure, export…) et `data-tone`. Un texte inconnu reste neutre, jamais une couleur fausse |
| Liste des compétences (E4) | `.feedback-line[data-kind="cp"]` | Une ligne par compétence : badge PASS / FAIL (`.badge--ok` / `.badge--warn`) d'abord, libellé ensuite ; filet ambre sur un FAIL ; aucun total |
| Barre d'action | `.progress-actions` | Collée en bas du panneau, opaque, filet `--line-field` en haut : la bascule de la fiche, **une** action principale (la vérification), sa barre d'avancement de 2 px (`.progress-run`), le message sous le bouton (`.progress-blocked`, violet `.is-run` pendant un essai ; les champs manquants en liste `.pb-list`, texte du cœur inchangé), puis `.progress-hint` (champ mal formé : bouton désactivé, ambre ; modifications non enregistrées : « La fiche sera enregistrée avant la vérification. »). La bascule ne prend le violet que si la vérification n'est pas possible et que la fiche est à remplir. Pendant un essai, le verdict précédent est masqué et le décompte n'est dit que sous le bouton (les caches du bus et des slots disent seulement ce qui est masqué) |
| Alerte d'exportation (E5) | `.progress-alert.is-err` (`#progress-export`) | Fiche finale non exportée : bloc rouge, ce qui est conservé, et « Réessayer l'export » en action principale (la commande d'ouverture de la fiche finale l'exporte de nouveau) ; « Ouvrir la fiche finale » reste, sans le violet |
| Fiche | `.fiche` (`#progress-fiche`) | Conteneur défilant de l'onglet Fiche. En-tête collant (`.fiche-head` : titre, note `.fiche-note`, index `.fiche-nav` de boutons `.fiche-nav-item` numérotés 01–07, rouges si leur section contient un champ mal formé, la section visible sur `--accent-sel` ; sur une fenêtre basse, une seule ligne qui défile de côté) ; sections numérotées (`.fiche-block`, titre `.fiche-section`) ; pied collant (`.fiche-foot` : `.fiche-save`, marque `.fiche-dirty` « Modifications non enregistrées », état `.fiche-status` ; masqué une fois la mission clôturée). Aucun enregistrement automatique |
| Bloc de la fiche | `.fiche-record` | En-tête (« Trame 2 », l'identifiant tel que saisi, « Supprimer » en deux clics : fantôme, icône corbeille, rouge au survol), puis des lignes `.fiche-line` en grille qui se replie, jamais de défilement horizontal |
| Champ étiqueté | `label.ff` | Étiquette `.ff-label`, le contrôle en pleine largeur, le message de format `.ff-error` dessous (masqué tant qu'il est vide). Le `label` donne son nom accessible au contrôle. `.ff--wide` : deux colonnes ; `.ff--full` : toute la ligne |
| Champ de bits | `fieldset.ff-bits` | Légende `.ff-legend` (le nom du cœur : « CRC de protection »), puis Ordre, LSB, Longueur sur une ligne |
| Fiche d'incident | `.incident-grid` | Cinq sections ; deux colonnes à partir de 640 px de fiche (Cible, Catégorie, État observé \| Mesure clé, Justification). Définitions publiques sous chaque liste ; « Consigner la mesure » et sa date sur une ligne (`.incident-consign-row`), puis valeur et unité |
| Mesure clé du multimètre | `.meter-mission` | Sous le multimètre du Physical Lab, pendant un incident seulement : la grandeur imposée, « Consigner la mesure » (`.meter-consign`, même commande et mêmes textes que la fiche), la date de la consignation. `.meter-note` dit pourquoi les sondes sont verrouillées pendant une vérification |

### États

| État | Rendu |
|---|---|
| Connecté (`READY`) | badge et carré vert « Prête » |
| En cours (`SESSION`) | badge et carré **violet** « Session active » ; Start désactivé, Stop actif |
| Arrêté / déconnecté | badge neutre ; actions sans objet désactivées (voir Désactivé) |
| Alerte (carte absente, pas de code étudiant, carte muette) | ambre ; ligne de message en ambre |
| Erreur (port occupé, liaison perdue, incompatible, NACK/pertes non nulles) | rouge ; compteurs concernés en rouge |
| Poignée de main | bleu (information) |
| Sélection | `--accent-sel` + filet `--accent` |
| Focus clavier | contour de 1 px `--focus`, jamais d'anneau flou ; dans la progression et la fiche, 2 px décalés de 2 px, distincts de la bordure rouge d'un champ mal formé |
| Survol / pression | `--bg-hover` / `--bg-active` |
| Désactivé | Ni fond ni violet (le violet veut dire « à faire maintenant »), contour de 1 px **en tirets** `--line-strong`, libellé `--text-3` (5,7:1), curseur interdit ; un champ garde sa valeur en `--text-2` (7,6:1). Jamais estompé. Sa raison reste visible : message sous le bouton, infobulle ou note de la fiche. Les bascules `.btn--ghost` et la barre d'activité (icônes seules) gardent l'opacité 0,4 |

## 6. Performance (zones vivantes)

- **Rythme.** L'UI tire les changements à 20 Hz et ne touche le DOM qu'au `requestAnimationFrame`, uniquement pour ce qui est marqué modifié dans le `Store`.
- **Tableau des trames.**
  - Il garde 200 `<tr>` recyclées : la plus ancienne remonte en tête et se remplit, sans création d'élément en régime établi.
  - Ses largeurs sont fixes : une nouvelle trame ne décale jamais la mise en page.
  - Quand l'utilisateur a fait défiler, la vue est maintenue (ancrage).
- **Console** : 300 lignes au plus ; elle suit la fin seulement si l'on y est déjà.
- **Interdits** : effet par ligne, animation sur réception, recalcul de toute la liste.
- **Écriture du texte** : uniquement par `textContent`. `innerHTML` est interdit (garde).

## 7. Physical Lab (prochain jalon) : même langage

- **Instruments** (`.instrument`) : panneau carré, en-tête `.panel-head`, canvas sur `--inst-bg`.
- **Grille** : `--inst-grid` ; une division majeure sur cinq en `--inst-grid-major`.
- **Traces** :
  - CH1 `--trace-1` (jaune, CAN_H) ;
  - CH2 `--trace-2` (cyan, CAN_L) ;
  - math `--trace-3` (violet N7RT) ;
  - quatrième `--trace-4`.
- **Lectures** en `.readout` (mono 22 px, chiffres tabulaires) : multimètre, mesures de l'oscilloscope.
- **Schéma** (canvas de l'éditeur, convention complète dans `docs/physical-lab-v1.md` §1 et §2.2) :
  - traits de 1,5 px CSS (arrondis au pixel physique), angles droits, jointures `miter`, extrémités `butt` ou `square` ;
  - **aucun cercle ni arc** : jonctions en carrés pleins (≥ 3 conducteurs), broche non reliée en carré creux. Le garde `check-ui` refuse `arc`, `arcTo`, `ellipse`, `roundRect`, les ombres, `filter` et `lineDashOffset` dans le code Canvas ;
  - jetons `--sch-*` (`styles/tokens.css`), lus par le canvas via `getComputedStyle` :

    | Jeton | Rôle |
    |---|---|
    | `--sch-bg`, `--sch-grid`, `--sch-grid-major` | fond (= `--inst-bg`), points de grille, une ligne majeure sur dix |
    | `--sch-symbol`, `--sch-pin` | corps des symboles ; pattes et noms de broches |
    | `--sch-wire` | fils et carrés de jonction |
    | `--sch-label`, `--sch-value` | désignateurs (R1) ; valeurs des paramètres (10 kΩ) |
    | `--sch-live`, `--sch-live-bg` | valeurs calculées pendant RUN, dans un cartouche : couleur de lecture neutre, **jamais** une teinte de trace (`--trace-1..4`), pour ne pas être confondue avec une voie d'oscilloscope |
    | `--sch-select`, `--sch-select-fill`, `--sch-hover` | sélection (violet N7RT), survol |
    | `--sch-invalid`, `--sch-warn` | refus et erreurs (`--err`) ; flottant, ouvert (`--warn`) |

  - **Texte du schéma** : seule exception aux trois tailles (§3). Plex Mono, mis à l'échelle avec le zoom, masqué sous 7 px, toujours lisible à l'endroit (horizontal à 0° et 180°, vertical de bas en haut à 90° et 270°).
  - Désignateur et valeur hors du corps : si leur place propre touche le corps, une patte, une broche ou un nom de broche d'un composant voisin, ils glissent le long de leur côté par quarts de leur longueur, puis passent au côté opposé (`SceneModel.settle`, `src/lab/canvas/model.ts`). Seules les places propres et la géométrie fixe servent d'obstacles : le résultat ne dépend que du document.
  - Un circuit ouvert est ajusté à la taille où il s'affiche : ouvert pendant que la vue est masquée, il l'est à la première image visible, jamais sur une taille périmée.
  - Les états réels (contact ouvert ou fermé, curseur du potentiomètre) se dessinent dans le symbole lui-même, jamais par une animation.
  - Cartouches des valeurs vivantes : au-delà de la broche, déplacés (avec un trait de rappel) hors des symboles voisins, de leurs textes, des marqueurs et des autres cartouches. Les marqueurs de diagnostic et les aperçus de geste se dessinent par-dessus.
- **Analyseur CAN** : réutilise `.grid-table` et les formats de `src/format.ts`.
  - Les colonnes direction et fréquence par ID y prendront place : en V1, toutes les trames vont vers le nœud étudiant, et le cockpit n'agrège pas par ID.
- **Vue Physical Lab** (`styles/lab.css`, `src/lab/view.ts`, `src/lab/panels/`) : même coque que le cockpit.
  - Bibliothèque et fichier | barre de l'espace de travail, ligne de message, schéma, console (Diagnostics, Nets, Journal) | inspecteur. Activée par la barre d'activité ; une seule vue visible.
  - Les emplacements de la grille existent une fois par vue (`[data-view]`), l'inactive est `hidden` ; chaque vue garde sa disposition (`n7rt-can-lab.layout.v1`, `n7rt-can-lab.layout.v1.phys`) et ses titres de bascules (Bibliothèque, Console, Inspecteur).
  - Un clic souris sur un bouton de la vue (ou du cadre commun pendant qu'elle est affichée) laisse le focus en place : Espace reste au schéma (pan, poussoir sélectionné). Un champ focalisé est quitté, donc validé. Le focus clavier (Tab) ne change pas.
  - Inspecteur : identité, paramètres, commandes (interrupteur, bouton maintenu, curseur `.range` plus champ, sortie ou carte alimentée), brochage (broche, nom, rôle, net, V, I ; nom et rôle s'effacent sur un panneau étroit), mesures, état de fonctionnement, sources.
- **Valeurs affichées** (`src/lab/units.ts`, contrat physical-lab-v1 §4) :
  - préfixe d'ingénieur, point décimal, U+00A0 entre nombre et unité, micro U+00B5 (jamais U+03BC), moins U+2212 ;
  - 6 chiffres significatifs : zéros de fin retirés pour les paramètres (« 10 kΩ », « 4.7 µF »), gardés pour les lectures (« 1.50000 V ») pour une largeur stable ; valeur complète en infobulle ;
  - champ en cours d'édition : représentation exacte (« 4.7µ », « 1000G »), relue au bit près par le cœur.
  - Tout caractère que le formateur peut produire est vérifié par un test contre les tables `cmap` des polices embarquées (Plex latin + N7RT Symbols).

## 8. Ajouter un écran sans casser la cohérence

1. Réutiliser les classes existantes ; une nouvelle classe ne prend ses valeurs que dans `tokens.css`.
2. Données dynamiques : `textContent` uniquement, via `el()` et `setText()` (`src/dom.ts`).
3. Un nouveau jeton (couleur, taille) se justifie dans ce document. Une quatrième taille de texte ou une nouvelle couleur d'état se discutent d'abord.
4. Tout contrôle créé en TypeScript porte sa classe du système de design : `el("button", "btn …")`, `el("input", "input …")`, `el("select", "select …")`, `el("textarea", "textarea …")` (ou une classe de composant qui le dessine entièrement : `.tab`, `.lab-item`, `.range`…).
5. Lancer `npm run check` (`scripts/check-ui.mjs`). La garde refuse :
   - rayons, ombres, flou, animations, transitions de plus de 150 ms, `innerHTML` et les constructions interdites du canvas ;
   - un bouton, une liste ou un texte long créé sans classe, ou avec une classe littérale sans celle du système de design (une case à cocher peut s'en passer : `label.check` la dessine ; une classe calculée à l'exécution n'est pas vérifiée) ;
   - dans `styles/*.css`, un `var(--x)` non déclaré et une couleur brute `#…` hors de `tokens.css` ;
   - l'absence de `color-scheme: dark` dans `base.css` (la WebView dessinerait les contrôles natifs en clair).
6. Aperçus (`ui/preview/`, non livré aux étudiants) : `node ui/preview/shoot.mjs --lint` photographie chaque état aux tailles demandées et vérifie la mise en page (règles L1 à L7) ; chaque état doit passer à toutes les tailles.
