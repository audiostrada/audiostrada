# Automation

L'automation fait bouger une commande dans le temps : une montée de volume, un balayage de
filtre, un départ qui s'ouvre sur le refrain.

## Pistes d'automation

Affichez les pistes d'automation avec le bouton **A** de l'en-tête et ajoutez-en avec **+** :
volume, panoramique, muet, départs, et chaque paramètre de ses effets. Un VCA a une piste de
volume, qui bouge toutes les pistes qu'il contrôle.

- Cliquez sur une piste d'automation pour ajouter un point, glissez les points pour les déplacer,
  double-cliquez sur un point pour le supprimer.
- Clic droit sur un point pour la forme de la courbe jusqu'au suivant : linéaire, douce ou en
  escalier.
- Tirez le bord bas d'une piste d'automation pour l'agrandir.

## Modes

Le bouton de mode sous **M** / **S** / **R** décide de l'effet d'une commande qu'on bouge :

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
