# MIDI et piano roll

![Le piano roll](img/piano.webp)

## Clips MIDI

Piste › *Ajouter une piste instrument* ([[Ctrl+Shift+I]]) crée une piste avec le synthé intégré.
Double-cliquez dans un endroit vide d'une piste instrument, ou Piste › *Nouveau clip MIDI*
([[Ctrl+Shift+M]]), pour créer un clip, puis éditez-le dans le **Piano roll** ([[Alt+2]] ;
[[Ctrl+P]] le met dans sa propre fenêtre). Un clip s'agrandit pour accueillir des notes collées
au-delà de sa fin.

## L'éditeur de notes

Double-cliquez sur un clip MIDI, ou Affichage › *Éditeur de notes du clip MIDI sélectionné*,
pour éditer ses notes dans une grande fenêtre au-dessus de la timeline ; [[Escape]] la ferme.
Elle est organisée comme l'éditeur d'automation, avec les mêmes touches :

- En haut, de gauche à droite : les outils (**P** crayon, **S** sélection, **B** pinceau, **C**
  ciseaux, **E** gomme) et *Accord*, l'accord que le crayon et le pinceau posent au lieu d'une
  note ; puis ce qui s'applique aux notes sélectionnées (*Transformer*, supprimer), les grilles
  (le magnétisme et son pas, la gamme), et la vue (**Z** zoom, dézoomer, cadrer les notes). Les
  touches sont rappelées à droite.
- [[F11]] (ou le bouton à droite de la barre, ou un double-clic sur son titre) fait remplir à
  l'éditeur la fenêtre principale ; [[Escape]] revient à la fenêtre, un second la ferme. [[F]]
  cadre les notes, comme il cadre la session dans la timeline.
- Ce que fait l'outil en main est écrit en haut des notes.
- L'outil zoom : glissez sur une zone pour qu'elle remplisse la vue, cliquez pour zoomer,
  [[Alt]]+clic ou clic droit pour dézoomer.
- Le reste fonctionne comme dans le piano roll du bas : le clavier à gauche, la lane sous les
  notes, la molette et les commandes du clavier. Les autres clips n'apparaissent qu'une fois
  ajoutés (ci-dessous) : pas de notes fantômes ici.

### Plusieurs pistes à la fois

L'éditeur de notes montre tous les clips MIDI sélectionnés dans la timeline (Ctrl+clic ou
Shift+clic pour en sélectionner d'autres), sur un seul clavier, chacun dans la couleur de sa
piste : la basse sous les accords, dans les mêmes hauteurs.

- La liste à gauche nomme les pistes. Les nouvelles notes vont dans la piste **active**,
  marquée dans la liste : cliquez sur une piste de la liste, ou sur une de ses notes, pour la
  rendre active. Ses notes sont pleinement visibles, celles des autres pistes atténuées mais
  toujours éditables ; une note sélectionnée est toujours pleinement visible. Une note dessinée là où la piste active a un clip sélectionné va dans ce clip ;
  ailleurs, son clip s'agrandit.
- Déplacer, transposer, quantifier, les transformations, la lane de vélocité et les commandes
  du clavier agissent sur les notes sélectionnées de toutes les pistes à la fois ; chaque note
  reste dans son clip. Le tout fait une seule étape d'annulation.
- **+ Ajouter une piste** ajoute les clips MIDI d'une autre piste, et sa croix retire une
  piste. La sélection de la timeline suit.
- Le clavier à gauche joue la piste active ; une note glissée ou ajoutée joue la sienne.

Les éditeurs de notes, audio et d'automation se relaient : ouvrir l'un ferme les autres. Tous
ont la même barre, les mêmes touches, le même plein écran et le même zoom.

## Outils

Touches [[1]] à [[5]] quand le piano roll a le focus :

| Outil | Effet |
|---|---|
| **Pointeur** | sélectionne, déplace ([[Alt]]-glisser copie) et redimensionne les notes par un bord, trace un cadre de sélection dans le vide, ajoute une note par double-clic |
| **Crayon** | ajoute une note et en tire la longueur |
| **Pinceau** | peint une note à chaque pas de grille |
| **Ciseaux** | coupent les notes là où on clique |
| **Gomme** | supprime ce qu'elle survole |

Le piano roll a sa propre grille, jusqu'à la triple croche et en triolets ; maintenez [[Ctrl]]
pour placer les notes librement.

## Navigation

La molette fait défiler les touches ([[Shift]] : dans le temps), [[Ctrl]]+molette zoome dans le
temps et [[Alt]]+molette sur les touches, autour du pointeur. Le bouton du milieu déplace la vue ;
le bouton cadre ajuste aux notes. La vue suit la tête de lecture pendant la lecture ; cliquez
dans la règle pour la déplacer.

Les notes sonnent quand vous cliquez sur les touches à gauche, ajoutez ou déplacez une note ; les
touches s'allument quand les notes jouent. Un double-clic sur une touche sélectionne ses notes.

## Gamme et accords

- **Gamme** grise les touches hors de la gamme, peut *replier* le clavier sur ses notes et celles
  utilisées, et *garder les notes dans la gamme* quand on les dessine, les déplace ou les
  transpose. *Détecter d'après les notes* la trouve.
- **Accords** : le bouton d'accord fait ajouter un accord entier au crayon et au pinceau : celui
  que la gamme construit sur la note (triade ou septième), ou un type choisi, renversé et disposé
  (serré, ouvert, drop 2, étalé). La bande au-dessus de la règle nomme les accords des notes.
- **Notes fantômes** : les autres clips MIDI apparaissent en transparence ; double-cliquez sur
  l'une pour éditer son clip.

## Transformer

Par la barre d'outils ou le menu du clic droit. Quantifier (force, swing, fins de notes),
humaniser, arpéger à la guitare (strum), arpéger, harmoniser, accords d'après les notes,
variation, aléatoire, forme et étirement des vélocités ouvrent un panneau qui montre le résultat
sur les notes ; [[Enter]] applique, [[Escape]] annule. Legato, inverser, miroir, coller et muet
s'appliquent tout de suite. [[Q]] quantifie la sélection.

## La bande du bas

La bande sous les notes édite la **vélocité** ou la **probabilité** (une note sous 100 % ne joue
qu'une partie des fois) en barres : glissez-en une (les sélectionnées suivent), glissez en travers
pour peindre, [[Shift]]-glisser trace une rampe droite, clic droit pour des formes (crescendo,
compression, accents).

Elle dessine aussi les contrôleurs et, avec une seule note sélectionnée, sa propre hauteur, sa
pression ou son timbre (MPE), tracés dans la note ; *Canaux propres* donne à chaque note son
canal.

## Touches

| Touches | Commande |
|---|---|
| [[Ctrl+C]] / [[Ctrl+X]] / [[Ctrl+V]] | copier, couper, coller à la tête de lecture |
| [[Ctrl+D]] | dupliquer juste après |
| [[Left]] / [[Right]] | décaler d'un pas de grille |
| [[Up]] / [[Down]] | transposer ([[Shift]] : une octave) |
| [[Q]] | quantifier |

## Contrôleurs et apprentissage MIDI

Les boutons d'un clavier tournent ceux du synthé intégré des pistes armées, par les contrôleurs
standard : CC 74 coupure, 71 résonance, 73 attaque, 75 décroissance, 72 relâchement, 7 volume (en
mode MPE, le CC 74 façonne plutôt chaque note).

**Apprentissage MIDI** : clic droit sur un fader, un bouton de panoramique, un niveau de send ou
un paramètre d'effet › *Apprentissage MIDI*, puis bougez une commande de votre contrôleur MIDI.
Un contrôleur appris ne fait que ce pour quoi il a été appris. Les flèches des presets d'un effet
apprennent un bouton de la même façon.

Un contrôleur appris sur un paramètre d'effet est gardé pour tous les projets, par type d'effet :
appris sur la première macro de Strada-Synth, il tourne la première macro du Strada-Synth des
pistes armées (sinon des pistes sélectionnées), dans n'importe quel projet. Ceux des faders, des
panoramiques et des sends du mixeur appartiennent au projet. Un contrôleur tourne un temps ou une
fréquence comme son potard, finement en bas de sa course.

Un contrôleur déplace un réglage d'autant qu'il tourne, à partir de la valeur du réglage : un
potard sans fin, ou un potard resté ailleurs après un changement de preset, ne le fait jamais
sauter. Son premier message indique seulement où il se trouve. Un potard ou un fader à butées
se recale après un aller-retour complet.

**Les changements de programme** envoyés par un clavier (ses boutons de presets) choisissent le
preset de l'instrument intégré des pistes armées : le programme 1 est le premier preset de son
menu.
