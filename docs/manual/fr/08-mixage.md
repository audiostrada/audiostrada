# Mixage

![Le Mixer](img/mixer-close.webp)

## Tranches

Chaque piste a sa tranche dans le **Mixer**, dans l'ordre où le signal la traverse : l'entrée avec
son gain et sa polarité (**Ø**), le channel strip, les inserts, les sends, la sortie (et le VCA),
puis le panoramique à côté du mode d'automation, le fader et l'indicateur, le muet (**M**), le solo
(**S**), l'armement (**R**) et le nom de la piste, marqué **BUS** pour un bus et **VCA** pour un
VCA. Le fader prend la hauteur que le panneau laisse. Un clic droit dans l'espace vide à côté des
tranches ajoute une piste, un bus ou un VCA ; un clic y vide la sélection.

Le solo garde audible ce vers quoi va la piste soloée. Un bus en solo fait aussi entendre les
pistes routées vers lui, et celles qui lui envoient un send, par leur send seulement : un bus de
réverbe en solo fait entendre la réverbe seule, sans le son direct de ce qui l'alimente.

- **Ancré** dans la fenêtre principale, la tranche laisse sa hauteur au fader. À côté de
  l'indicateur, dans l'ordre du signal : le bouton du channel strip, puis **FX** avec une pastille
  par effet (un clic ouvre l'effet, [[Shift]]-clic le bypasse, [[Ctrl]]-clic le retire) et **+**
  pour en ajouter un, puis **Sends** avec une pastille par send qui montre son niveau. **FX** et
  **Sends** (allumés, avec leur nombre, quand la liste contient quelque chose) ouvrent toute la
  liste juste en dessous, pour réordonner ou ajouter ; un clic ailleurs la referme. Le master a
  son channel strip, **FX** et **Mon** (effets d'écoute).
- **Dans sa propre fenêtre** (Affichage › Mixer › *Dans sa propre fenêtre*, [[Ctrl+M]]), chaque
  tranche montre ses emplacements d'effets et de sends : cliquez sur un emplacement vide pour
  ajouter un effet ou un send. Les listes ont la hauteur de la plus longue, plus un emplacement
  vide, et laissent le reste aux faders. [[Ctrl+M]] à nouveau le remet en place. *Mixer compact*
  ([[Shift+Alt+C]], aussi dans Préférences › Apparence) rétrécit les tranches. Un clic droit sur le
  master le place à gauche des pistes, ou le remet à leur droite.

![Le Mixer dans sa propre fenêtre](img/mixer-window.webp)

## Faders et panoramique

- **Les faders** suivent une loi de console (0 dB aux trois quarts de la course) : glissez-les,
  [[Shift]] pour la précision, double-cliquez pour 0 dB.
- **Panoramique** : le potard sous la sortie, sa valeur en dessous. Glissez vers le haut ou la
  droite ([[Shift]] pour des pas fins ; le centre retient le pointeur), un double-clic recentre.
- **La molette** règle le fader (un demi-dB par cran, [[Shift]] : un dixième), le panoramique
  (1 % par cran) et les sends de la piste **sélectionnée** seulement : passer la souris sur le
  mixer ne bouge rien.

## Sélectionner des pistes

Cliquez sur une plaque de nom (ou un en-tête de piste dans la timeline) ; [[Ctrl]] ajoute ou
retire, [[Shift]] étend. Couleur, dupliquer et supprimer du menu de piste agissent sur toutes les
pistes sélectionnées, tout comme le menu de sortie et le fader : bouger le fader d'une piste
sélectionnée bouge les autres d'autant de dB. Glissez une plaque de nom pour déplacer des pistes.
[[Shift+Alt+M]], [[Shift+Alt+S]] et [[Shift+Alt+R]] rendent muettes, mettent en solo et arment
les pistes sélectionnées.

## Inserts

- Cliquez sur *+ insert* ou sur un emplacement vide pour ouvrir le **navigateur d'effets** ;
  cliquez sur un effet pour ouvrir sa fenêtre.
- Dans le navigateur, la colonne de gauche choisit une famille, vos favoris ou les derniers effets
  ajoutés ; la recherche parcourt tout, les boutons du haut gardent un format ou un éditeur. Un
  clic choisit un effet et montre ses presets en bas, un double-clic ou *Ajouter* ([[Enter]])
  l'ajoute. Les flèches passent d'une carte à l'autre, [[/]] va à la recherche, [[F]] met l'effet
  choisi en favori.
- [[Shift]]-clic met un effet en bypass, [[Ctrl]]-clic le retire, le clic droit donne les autres
  options ; *Sidechain* y choisit la piste qui pilote un effet doté d'une entrée sidechain.
- Glissez un effet pour le réordonner, ou sur une autre tranche pour l'y copier ([[Alt]] : le
  déplacer). Déposé sur la liste, il va à l'endroit choisi ; ailleurs sur la tranche, en dernier.
- **Bypass** au-dessus des inserts met en bypass tous les effets de la tranche, et reste allumé
  tant qu'ils le sont ; sur le master, **ALL** met en bypass tous les effets de la session (un
  instrument en tête de piste continue de jouer).

**Les fenêtres d'effets** montrent, dans leur barre du bas, la piste qui pilote la détection pour
les effets qui ont un **Sidechain**, le niveau qui entre et qui sort (crêtes et sonie) et le gain
de **Sortie** après l'effet, gardé dans le mix et l'export. **Match** est une aide à l'écoute : tant
qu'il est allumé, la sortie est ramenée à la sonie de l'entrée, pour comparer l'effet et son
bypass à niveau égal. Il n'est jamais enregistré ni exporté et s'éteint quand la fenêtre se ferme ;
**Garder** reporte sa correction dans la Sortie.

## Le rack

L'onglet **Rack** ([[Alt+6]]) montre la chaîne de la piste sélectionnée (ou d'une tranche
épinglée) en modules côte à côte, chacun avec ses réglages principaux, A/B, et son éditeur à un
clic. Les chaînes parallèles séparent le signal en branches et les remélangent.

![Le rack](img/rack.webp)

## Sorties, bus et sends

- **Sortie** : le bouton au-dessus du panoramique dit où va le son : **Master**, un bus, ou *Pas
  de sortie*. Son menu route toutes les pistes sélectionnées d'un coup, et *Nouveau bus pour n
  piste(s)* crée un bus pour elles, placé là où était la première et nommé d'après elles
  (*Batterie*). Piste › *Router la sélection vers un nouveau bus* ([[Ctrl+Shift+B]]) fait de même.
  Le menu est aussi dans le clic droit de la piste et dans l'Inspecteur.
- **Les bus** additionnent les pistes routées vers eux à travers leurs propres inserts, sends et
  fader, puis vont au master. Ils ne portent pas de clips. Dans la timeline, les pistes d'un bus
  s'affichent sous lui et se replient avec lui ; glisser une piste parmi elles la route vers ce
  bus. Piste › Ajouter › *Ajouter un bus* en crée un vide, pour une réverbe alimentée par des
  sends.
- **Les sends** vont vers n'importe quelle piste ou bus (pré ou post fader) ; le menu d'un
  emplacement de send vide liste d'abord les bus, et *Nouveau bus d'effet* en crée un alimenté par
  le send. Le send s'ajoute à toutes les pistes sélectionnées ; cochez d'abord *Couper la sortie
  vers le master* pour les pistes qu'on n'entend que par le send. Chaque send est un bouton comme
  un insert, avec son niveau (et PRÉ avant le fader) : un clic ouvre sa fenêtre de réglages
  (destination, niveau, pré ou post fader, muet), la molette règle son niveau. Glissez un send pour
  le réordonner, ou sur une autre tranche pour l'y copier ([[Alt]] : le déplacer).
- **VCA** : le clic droit de la piste choisit le VCA qui la pilote ; un **V** sous le fader le
  signale.

## Groupes

Sélectionnez des pistes et appuyez sur [[Ctrl+G]]. Un groupe n'a rien à voir avec les bus : les
pistes d'un bus restent indépendantes tant qu'elles ne sont pas groupées. Le menu Groupes règle ce que partagent les membres : niveau, panoramique et
sends (en relatif), muet, solo, armement, bypass des inserts, mode d'automation, polarité, et
édition (un clic sur un clip ou une piste dans la timeline prend aussi ceux des membres : coupes,
déplacements, rognages et fondus s'appliquent à tous).

Maintenez [[Ctrl]] pour agir sur une seule piste ou sélectionner un seul clip. [[Ctrl+Shift+G]]
suspend ou réactive tous les groupes.

## VCA

Piste › *Ajouter un VCA*, avec des pistes sélectionnées, crée un fader qui bouge leurs niveaux et
leur muet ensemble, en gardant leur équilibre.

- **La tranche du VCA** liste les pistes qu'il contrôle : un clic en sélectionne une, la × au
  survol la retire du VCA. *Assigner la sélection* ajoute les pistes sélectionnées.
- **Les faders des pistes** sont au niveau entendu, le leur plus celui du VCA, et bougent quand le
  VCA bouge. Bouger le fader d'une piste ne change que son niveau propre ; son infobulle montre la
  répartition.
- **Automation** : un VCA a une piste d'automation de volume et un mode d'automation, comme toute
  piste. Sa courbe s'ajoute au niveau de chaque piste qu'il contrôle, qu'elles aient leur propre
  automation ou non.

## Couleur de console

![Couleur de console](img/console-color.webp)

Le menu **Console** de la barre du Mixer donne à tout le mix le son d'une console analogique :
chaque piste passe par l'étage d'entrée d'une de ses tranches, les bus et le master par sa
sommation.

| Console | Caractère |
|---|---|
| **Neutre** | aucune couleur : le mix tel quel |
| **British Warm** | transformateurs : graves ronds, aigus soyeux (inspirée des consoles Neve) |
| **British Bright** | serrée et précise, une touche d'air (inspirée des consoles SSL) |
| **American Punch** | amplis discrets : punch et attaque (inspirée des consoles API) |
| **Nashville** | douce et chaude, très faible distorsion (inspirée des consoles Harrison) |

Chaque console est un circuit d'étages analogiques (transformateurs, amplis en classe A, amplis
op, VCA), calibré sur les mesures des plugins qui modélisent la console dont elle s'inspire.
Comme les vraies, elles colorent subtilement : au niveau de travail, les harmoniques d'une tranche
sont 60 à 90 dB sous le signal, et montent quand il est plus fort. Les transformateurs de British
Warm épaississent surtout les graves ; la tranche de British Bright ajoute des harmoniques
impaires, égales sur tout le spectre, et son bus de mix est le plus coloré ; American Punch ajoute
des harmoniques paires ; la tranche de Nashville est presque propre, son bus sature les graves.

*Quantité* pousse le circuit : 50 % est la console à son niveau de travail, 0 % douze
dB plus doucement, 100 % douze dB plus fort, toujours au même niveau de sortie, pour que seule la
couleur change. *Souffle et diaphonie* ajoute le bruit de fond de chaque tranche (souffle, un peu
de ronflement) et les côtés des bus qui débordent l'un sur l'autre. Chaque piste est une tranche à
part : ses composants diffèrent un peu de ceux de ses voisines, ce qui élargit le mix comme sur
une vraie console. Chaque piste peut prendre une autre console (*Console de la piste*). Le bouton
marche/arrêt à côté du menu coupe toutes les couleurs au même niveau, pour comparer.

**Test en aveugle › Lancer…** dit si vous entendez les consoles ou si vous croyez les entendre :
A est le mix avec ses consoles, B sans elles, X l'un des deux tiré au hasard. Lancez la lecture,
écoutez A, B et X autant de fois que vous voulez, puis dites lequel est X. Après dix essais, la
fenêtre dit combien de fois le hasard ferait aussi bien : 9 justes sur 10 n'arrivent au hasard
qu'environ une fois sur cent. Le test ne change rien au projet ; fermer sa fenêtre rend les
consoles.

Le caractère d'une console dépend du niveau qu'elle reçoit : **Réglage des gains › Appliquer**
règle le gain d'entrée de chaque piste audio pour qu'elle joue vers -18 dBFS (les pistes
sélectionnées, ou toutes).

## Strada-Channel

![Strada-Channel](img/channel-strip.webp)

Strada-Channel est une tranche de console entière dans un seul insert : filtres, gate,
compresseur, égaliseur à quatre bandes et saturation. Sa fenêtre montre la courbe d'égalisation
avec ses poignées et les indicateurs, la console, le gain d'entrée et la polarité de la piste,
puis les modules dans l'ordre où ils jouent ; glissez un module par sa poignée pour le déplacer.
Voir *Effets* pour ses paramètres.

## Préparer une piste

La baguette dans l'en-tête du **Channel Strip** d'une tranche prépare la piste, ou toutes les
pistes sélectionnées : un clic la nettoie et la fait ressortir, un clic droit (ou le clic droit
sur la piste, ou Piste › *Préparer la piste*) choisit l'un des deux ou retire la préparation.
**Tout préparer**, en haut du mixer, le fait pour toutes les pistes d'un coup (pas les bus) :
dans la fenêtre, coupez les pistes à laisser comme elles sont.

- **Nettoyer** : un passe-haut sous l'énergie utile (et un passe-bas pour la basse, la grosse
  caisse et les guitares saturées), des coupes aux résonances qui ressortent, les plosives et
  les sifflantes d'une voix retenues (un de-esser est aussi proposé sur les overheads et le
  charleston), et un gate sur la grosse caisse et les toms quand il y a de la repisse entre les
  coups. Une piste avec de longs silences reçoit plutôt la suggestion de les supprimer.
- **Faire ressortir** : les gestes qu'un ingénieur fait sur cet instrument, sur l'EQ du Channel
  Strip (poids, boue, clic sur une grosse caisse ; boue, présence, air sur une voix…), chacun
  placé là où la piste l'a et dosé selon ce qu'elle en a ; un compresseur (toujours sur une
  grosse caisse, une caisse claire et une basse, sinon quand le niveau varie beaucoup) ; et, si
  vous le cochez, un suivi du volume écrit en automation de la sortie du Channel Strip (à 6 dB
  près).
- **Les deux**, et **Retirer ce qui a été préparé**.

AudioStrada rend la piste seule, telle qu'elle arrive à son Channel Strip, et devine
l'instrument d'après le nom de la piste, son instrument ou son son (voix d'homme ou de femme
d'après sa hauteur) ; changez-le après *L'instrument est* et la proposition suit. L'image montre
le spectre mesuré et ce que la proposition en fait, chaque geste marqué ; les deux cartes
listent les étapes, chacune avec son interrupteur et son dosage (50 % est ce qui est proposé ;
double-clic pour y revenir). **Intensité** (léger, normal, appuyé) règle jusqu'où tout cela
va.

Tant que la fenêtre est ouverte, la proposition s'entend : *Écouter* joue la session, **Avant**
et **Après** comparent la piste comme elle était et la piste préparée, et **Même niveau** amène
la piste préparée au volume qu'elle avait, pour que « plus fort » ne passe pas pour « mieux ».
*Appliquer* écrit le tout en une seule étape d'annulation, ce gain gardé sur la sortie des
effets ; *Annuler* laisse la piste comme elle était.

Les réglages sont ceux du Channel Strip, plus un Strada-EQ après lui pour les résonances et les
plosives (avec des bandes dynamiques quand elles vont et viennent) et un Strada-DeEss. Préparer
à nouveau remplace ce qui a été fait ; la baguette reste allumée sur une piste préparée. Ce sont
des points de départ, posés par des règles tirées de la pratique du mixage, à ajuster à
l'oreille.

## Instantanés

Les **Instantanés** enregistrent l'état de la console et le rappellent, pour comparer deux mix.

## Master et référence

- Le **master** affiche à côté de son indicateur la sonie du mix depuis **Réinit.** (**LUFS**,
  intégrée ; les plateformes de streaming ramènent les morceaux vers -14) et sa plus haute crête
  (**PEAK**, en rouge au-dessus de -1 dB, où elle peut saturer une fois encodée).
- **M** en haut, puis son mode d'automation, comme sur les pistes. **M**, **Dim** et **Mono** ne
  changent que ce qu'on entend, jamais l'export : M coupe les enceintes, Dim les baisse de 20 dB
  (pour parler, ou répondre au téléphone), Mono additionne gauche et droite pour vérifier le mix
  en mono. Ils sont éteints à l'ouverture du projet.
- Un clic sur la tranche master la sélectionne : le rack et l'Inspecteur l'affichent.
- **MONITOR** contient les effets qu'on entend mais qui ne sont pas exportés : correction de
  pièce, référence.
- **REF** à côté (aussi dans la barre de transport, [[Alt+R]]) compare le mix à une chanson finie
  (Transport › *Piste de référence…* pour la choisir). Cliquez dessus pour entendre la référence à
  la place du mix, au même endroit de la chanson et, par défaut, à la sonie du mix, pour comparer
  le son et non le niveau. Elle n'est jamais exportée.
