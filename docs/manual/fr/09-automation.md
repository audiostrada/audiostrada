# Automation

L'automation fait bouger une commande dans le temps : une montée de volume, un balayage de
filtre, un send qui s'ouvre sur le refrain.

## Pistes d'automation

Affichez les pistes d'automation avec le bouton **A** de l'en-tête et ajoutez-en avec **+** :
volume, panoramique, muet, sends, et chaque paramètre de ses effets. Un VCA a une piste de
volume, qui bouge toutes les pistes qu'il contrôle.

- Cliquez sur une piste d'automation pour ajouter un point, glissez les points pour les déplacer,
  double-cliquez sur un point pour le supprimer.
- Clic droit sur un point pour la forme de la courbe jusqu'au suivant : linéaire, douce ou en
  escalier.
- Tirez le bord bas d'une piste d'automation pour l'agrandir.

Affichage › *Piste master dans la timeline* montre le master en haut, sous la règle. Son volume
est sa ligne principale, deux fois plus haute qu'une piste d'automation (un fondu en fin de
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
