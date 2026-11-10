# Automation

L'automation fait bouger une commande dans le temps : une montée de volume, un balayage de
filtre, un send qui s'ouvre sur le refrain.

## Pistes d'automation

Affichez les pistes d'automation avec le bouton **A** de l'en-tête et ajoutez-en avec **+** :
volume, panoramique, muet, sends, et chaque paramètre de ses effets. Un VCA a une piste de
volume, qui bouge toutes les pistes qu'il contrôle.

- Cliquez sur une piste d'automation pour ajouter un point ; cliquez sur la courbe pour en ajouter
  un exactement dessus. Appuyez et glissez dans un espace vide pour poser un point et le déplacer
  aussitôt.
- Glissez un point pour le déplacer. Il part de l'endroit où vous l'avez saisi, sans sauter sous
  le pointeur. Tenez **Shift** pour le déplacer dix fois plus finement (sans grille ni aimant), et
  **Ctrl** pour ne changer que sa valeur, sans toucher à sa position dans le temps.
- Glissez la courbe entre deux points pour monter ou descendre ce segment avec ses deux points.
  Avant le premier point et après le dernier, la ligne plate déplace ce point.
- **Alt**+glisser sur un segment le courbe : vers le haut, la courbe se bombe vers le haut ;
  ramenée au milieu, elle redevient droite.
- **Ctrl**+glisser sur une piste d'automation dessine à main levée : le tracé remplace les points
  qu'il recouvre.
- Une valeur déplacée est attirée par la ligne pointillée de la valeur réglée, par 0 dB (ou le
  centre pour le panoramique) et par les valeurs des points voisins, pour que les lignes plates
  restent plates ; une ligne pointillée de la couleur de la piste montre l'aimantation. Les temps
  s'aimantent à la grille, aux clips et aux marqueurs comme les clips.
- Cliquez sur un point pour le sélectionner, **Shift**+clic pour l'ajouter à la sélection,
  **Shift**+glisser (ou glisser avec l'outil plage) pour sélectionner les points d'un
  rectangle. Les points sélectionnés bougent ensemble, et **Suppr** les efface. **Alt**+clic
  supprime un point.
- Double-cliquez sur un point pour saisir sa valeur : « -3.5 », « -3.5 dB », « L 20 », « 2.4 kHz ».
- Clic droit sur un point pour la forme de la courbe jusqu'au suivant (linéaire, douce ou en
  escalier), pour tous les points sélectionnés. Clic droit ailleurs dans la piste d'automation
  pour sélectionner ou supprimer tous les points.
- Les pistes de volume suivent l'échelle du fader, plus fine autour de 0 dB. Au survol, la piste
  d'automation indique ce que ferait un clic ou un glisser à cet endroit.
- Tirez le bord bas d'une piste d'automation pour l'agrandir.

Affichage › *Piste master dans la timeline* montre le master en haut, sous la règle. Son volume
est sa ligne principale, plus haute qu'une piste d'automation (un fondu en fin de
morceau) ; son **+** ajoute une piste d'automation pour un paramètre de ses effets. Chaque ligne
s'édite de la même façon et s'agrandit par son bord bas. Son mode d'automation est
sur sa tranche dans le mixer (Read par défaut). Ses **M** et **Mono** sont ceux de sa tranche. Un
clic droit sur son nom le masque, dans l'espace vide sous les en-têtes de piste le réaffiche ; la
même option est dans Préférences › Apparence.

## Modes

Le bouton de mode à côté du panoramique (**Read**, **Touch**...) décide de l'effet d'une commande qu'on bouge :

| Mode | Comportement |
|---|---|
| **Off** | les courbes sont ignorées |
| **Read** | les courbes jouent |
| **Touch** | écrit tant que vous bougez une commande, revient à la courbe quand vous la lâchez |
| **Latch** | écrit tant que vous bougez une commande, garde la dernière valeur jusqu'à l'arrêt |
| **Write** | enregistre toutes les commandes pendant la lecture |
| **Trim** | décale la courbe existante |

Quand vous tenez une commande automatisée en Read, elle suit votre main ; elle rend la main à la
courbe quand vous la lâchez.
