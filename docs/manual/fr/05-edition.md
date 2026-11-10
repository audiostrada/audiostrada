# Édition

![La timeline et sa barre d'outils](img/timeline.webp)

## La barre d'outils

La barre au-dessus des pistes contient, de gauche à droite :

1. **Les outils**, touches [[1]] à [[5]] : intelligent, plage, ciseaux, gomme et muet.
2. **Le mode d'édition** (Slip, Shuffle, Grille), la **valeur de grille** et l'**Aimant**, avec
   le menu de ce qui attire les éditions.
3. **Les commandes d'édition** : scinder, dupliquer, supprimer, recoller, consolider, supprimer
   les silences, traiter les clips. Dans une fenêtre étroite, elles se rangent sous le menu **»**.
4. **Marqueurs et navigation** : aller au début, marqueur précédent, ajouter un marqueur,
   marqueur suivant, aller à la fin, afficher la tête de lecture.
5. **L'affichage**, à droite : le menu de forme d'onde, la hauteur des pistes et le zoom
   horizontal, *Zoomer sur toute la session* et *Zoom sur la sélection*.

Chaque bouton donne son nom, ses touches et son geste au survol.

## Les outils

| Outil | Touche | Effet |
|---|---|---|
| **Intelligent** | [[1]] | déplace, rogne, pose les fondus et sélectionne des plages, selon l'endroit du clip sous le pointeur (ci-dessous) |
| **Plage** | [[2]] | glisser sélectionne une plage de temps partout, à travers clips et pistes |
| **Ciseaux** | [[3]] | un clic scinde le clip à cet endroit ; [[Alt]]-clic scinde toutes les pistes |
| **Gomme** | [[4]] | un clic supprime un clip ; glisser supprime tous les clips survolés |
| **Muet** | [[5]] | un clic rend un clip muet ou le rétablit ; glisser fait de même sur tous les clips survolés |

Maintenir [[Ctrl+Alt]] donne les ciseaux le temps de l'appui. [[Escape]] revient à l'outil
intelligent.

### L'outil intelligent

L'effet d'un glisser dépend de l'endroit où il commence sur un clip :

| Endroit | Glisser fait |
|---|---|
| Moitié basse du corps, ou bande du nom | Déplacer ([[Alt]] : copier, aussi vers une autre piste) |
| Moitié haute du corps | Sélectionner une plage, à travers clips et pistes |
| Bord gauche ou droit | Rogner |
| Bord droit avec [[Alt]] | Étirer dans le temps, hauteur conservée |
| Coins hauts | Fondu d'entrée, fondu de sortie |
| Milieu de la courbe d'un fondu | Courber le fondu |
| Milieu du bord haut | Volume du clip |
| Corps avec [[Ctrl]] | Volume du clip |
| Corps avec [[Ctrl+Alt]] | Glisser l'audio dans le clip, qui reste en place |

Sur les pistes de moins de 56 pixels de haut, tout le corps déplace le clip. [[Shift]]-glisser
sélectionne toujours une plage.

Pendant le glisser, une étiquette près du pointeur affiche la valeur : le déplacement en mesures
et en millisecondes et la piste visée, la nouvelle longueur, la durée et la forme du fondu, le
volume en dB, l'étirement en pourcentage.

## Sélectionner

Un clic sélectionne un clip ; [[Ctrl]]-clic en ajoute ou en retire un ; [[Shift]]-clic
sélectionne tous les clips du dernier cliqué à celui-ci, à travers les pistes entre les deux. Les
rognages, fondus et volumes tracés sur un clip sélectionné s'appliquent à tous les clips
sélectionnés, de la même quantité. [[Ctrl+A]] sélectionne tous les clips, [[Escape]] efface la
sélection et la plage.

Un clic sur un clip le sélectionne sans déplacer la tête de lecture ; un clic dans un endroit vide
la déplace.

## Modes d'édition et aimant

| Mode | Touche | Comportement |
|---|---|---|
| **Slip** | [[F2]] | les clips se déplacent librement |
| **Shuffle** | [[F1]] | supprimer ou rogner referme le trou ; les clips suivants se rapprochent |
| **Grille** | [[F4]] | déplacements, rognages et plages s'aimantent à la grille |

[[G]] active ou désactive l'aimant dans les autres modes. [[Ctrl+Shift+=]] et [[Ctrl+Shift+-]]
rendent la grille plus fine ou plus large.

Le menu **Aimant** choisit ce qui attire les éditions : la grille, les bords des clips, les
marqueurs, la tête de lecture et les transitoires. Quand une position s'aimante, une ligne fine
montre ce qui l'a attirée.

## Fondus et fondus enchaînés

- **Fondus** : glissez les coins hauts d'un clip. Tirez le milieu de la courbe vers le haut ou le
  bas pour la courber, [[Alt]]-clic sur la courbe fait défiler les formes, un double-clic la
  redresse, et le clic droit donne la liste des formes : puissance constante, linéaire, rapide,
  lente, en S.
- **Fondus enchaînés** : les clips qui se chevauchent se fondent tout seuls. Le chevauchement
  s'affiche en X avec une poignée : tirez son bord haut pour allonger ou raccourcir le fondu, son
  corps pour le faire glisser. Clic droit pour sa forme, ou pour le retirer.
- **Fondus sur une plage** : avec une plage sélectionnée, [[Ctrl+F]] pose les fondus d'entrée et
  de sortie des clips sur les bords de la plage ; sur une coupure, il crée un fondu enchaîné. Avec
  des clips sélectionnés et sans plage, il leur pose de courts fondus.

## Plages

Une plage est une portion de temps sur une ou plusieurs pistes. Tracez-la avec l'outil plage,
dans la moitié haute d'un clip avec l'outil intelligent, ou en [[Shift]]-glisser. Ensuite :

- [[Shift+R]] crée une plage sur les clips sélectionnés ; le menu Édition en crée aussi une
  d'après la boucle, ou sur toutes les pistes.
- [[Ctrl+Shift+R]] rogne le projet à la plage.
- [[Ctrl+Shift+Delete]] supprime le passage et rapproche la suite, sur toutes les pistes.
- [[Shift+Delete]] vide le passage et laisse le blanc.
- [[Shift+L]] boucle la sélection ; le clic droit sur la plage donne les autres commandes
  (insérer du silence, scinder aux bords, définir le punch).

## Menu des clips

Clic droit sur un clip : renommer, scinder ici ([[S]] scinde à la tête de lecture), dupliquer
([[Ctrl+D]]), supprimer, durées et formes des fondus, pas de volume et *Remettre à 0 dB*,
normaliser, rendre muet, boucler ces clips, étirer (de 50 % à 200 %, revenir à la vitesse
d'origine, ou caler sur le tempo du projet, qui détecte le tempo du clip), transposer (par
demi-tons et octaves, ou revenir à la hauteur d'origine), glisser le contenu, et *Ouvrir dans le
piano roll* pour un clip MIDI.

L'**Inspecteur** ([[Alt+4]]) règle les valeurs exactes, dont le pourcentage d'étirement et la
hauteur au cent près.

## Traiter les clips

Par le menu Édition, ou par un clic droit sur des clips ou une plage :

- **Supprimer les silences** ([[Ctrl+U]]) : une fenêtre règle le seuil, le silence le plus court
  et ce qui est gardé avant et après le son ; la timeline assombrit ce qui va partir. Gardez le
  son, les silences, ou coupez seulement aux silences.
- **Recoller** ([[Ctrl+H]]) réunit en un seul les clips coupés dans le même audio quand ils sont
  encore à leur place.
- **Consolider** ([[Ctrl+Shift+C]]) rend les clips sélectionnés (ou la plage) de chaque piste en
  un seul nouveau fichier, avec volume, fondus, étirement et hauteur appliqués.
- **Traiter les clips…** ([[Alt+P]]) rend des clips à travers un effet intégré, les inverse ou
  les normalise, en nouveaux fichiers. Les originaux restent dans le projet ; [[Ctrl+Z]] revient
  en arrière.
- **Beat Detective…** ([[Alt+B]]), avec une plage sur des pistes de batterie, trouve les coups,
  affichés en lignes rouges, sur toutes les pistes sélectionnées ensemble, pour que les micros
  d'une batterie soient coupés aux mêmes endroits. *Séparer aux attaques* coupe les clips ;
  l'autre choix déplace aussi chaque tranche vers la grille, selon la force choisie, avec de
  courts fondus enchaînés.
- **Groove…** ([[Alt+G]]) cale les coups de certaines pistes (une basse, des guitares, des
  claviers) sur ceux d'une référence (la batterie), en étirant l'audio entre eux. Les pistes
  traitées ensemble sont coupées aux mêmes endroits pour garder leur phase ; chacune reçoit le
  résultat dans une nouvelle playlist. En dessous de 100 %, une part du jeu d'origine reste.

## Pistes

Double-cliquez sur le nom d'une piste pour la renommer ([[Enter]] valide, [[Escape]] annule). Un
nom trop long pour l'en-tête finit par « … » et s'affiche en entier au survol.

Le **menu de l'en-tête de piste** contient : renommer, couleur, *Piste stéréo*, dupliquer,
supprimer, déplacer, geler, bounce sur une nouvelle piste, exporter la piste seule, nouvelle
playlist, automation.

- Piste › *Ajouter une piste audio* ([[Ctrl+Shift+T]]) ajoute une piste sous la sélection,
  *Ajouter des pistes audio…* ([[Ctrl+Shift+N]]) plusieurs d'un coup.
- [[Ctrl+Up]] et [[Ctrl+Down]] sélectionnent la piste précédente ou suivante, [[Alt+Up]] et
  [[Alt+Down]] la déplacent.
- Avec deux pistes sélectionnées, *Convertir en piste stéréo* en fait une seule (« Overhead L »
  et « Overhead R » deviennent « Overhead »). Les noms disent la gauche et la droite, sinon la
  piste du haut est la gauche ; [[Ctrl+Z]] ramène les deux.

## Navigation et zoom

- **Zoom** : [[+]] et [[-]], ou [[Ctrl]]+molette, autour du pointeur quand il est sur les clips,
  sinon de la tête de lecture. [[F]] ajuste toute la session.
- **Zoom sur la sélection** : [[Z]] zoome sur la plage ou les clips sélectionnés ; [[Z]] à
  nouveau revient à la vue précédente.
- **Hauteur des pistes** : tirez le bord bas de l'en-tête d'une piste (un double-clic rend la
  hauteur par défaut), ou [[Alt]]+molette et [[Ctrl+Shift+Up]] / [[Ctrl+Shift+Down]] pour toutes
  les pistes.
- **Vue d'ensemble** : la bande sous les pistes montre toute la chanson en miniature, la partie
  visible encadrée. Glissez le cadre pour défiler, ses bords pour zoomer, double-cliquez pour tout
  voir.
- **Transitoires** : [[Tab]] et [[Shift+Tab]] amènent la tête de lecture au coup suivant ou
  précédent des clips de la piste sélectionnée.
- **Marqueurs** : [[M]] ajoute un marqueur à la tête de lecture ; [[.]] et [[,]] passent d'un
  marqueur à l'autre. Clic droit sur la règle ou sur un endroit vide d'une piste pour *Ajouter un
  marqueur ici*. Cliquez sur un marqueur, son drapeau ou son nom, pour y placer la tête de
  lecture ; glissez-le pour le déplacer ; double-cliquez dessus pour le renommer (Entrée garde le
  nom, Échap annule).

## Formes d'onde

Le **menu de forme d'onde** de la barre d'outils choisit le dessin de l'audio :

- **Échelle** : linéaire, logarithmique (les détails faibles se voient) ou redressée (niveau
  seul, deux fois plus haute).
- **Couleurs** : celle de la piste, ou les couleurs spectrales (graves en rouge, médiums en vert,
  aigus en bleu).
- **Canaux** : séparés ou fusionnés ; *Chaque clip aussi haut que possible* remplit la hauteur de
  la piste.

[[Alt+Shift+Up]] / [[Alt+Shift+Down]] ou [[Alt+Shift]]+molette zoome verticalement sur la forme
d'onde, [[Alt+Shift+0]] la remet à zéro. Les crêtes qui atteignent 0 dBFS sont marquées en rouge.
En zoomant assez, la forme d'onde montre chaque canal et les échantillons eux-mêmes, pour
vérifier la phase entre deux pistes.

Un bus montre les clips de ses pistes en miniature, une fine bande chacune.
