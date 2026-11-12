# MIDI et piano roll

![Le piano roll](img/piano.webp)

## Clips MIDI

Piste › *Ajouter une piste instrument* ([[Ctrl+Shift+I]]) crée une piste avec le synthé intégré.
Double-cliquez dans un endroit vide d'une piste instrument, ou Piste › *Nouveau clip MIDI*
([[Ctrl+Shift+M]]), pour créer un clip, puis éditez-le dans le **Piano roll** ([[Alt+2]] ;
[[Ctrl+P]] le met dans sa propre fenêtre). Un clip s'agrandit pour accueillir des notes collées
au-delà de sa fin.

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
panoramiques et des sends du mixeur appartiennent au projet.

**Les changements de programme** envoyés par un clavier (ses boutons de presets) choisissent le
preset de l'instrument intégré des pistes armées : le programme 1 est le premier preset de son
menu.
