# Mixage

![Le Mixer](img/mixer-close.webp)

## Tranches

Chaque piste a sa tranche dans le **Mixer**, dans l'ordre où le signal la traverse : l'entrée avec
son gain et sa polarité (**Ø**), le channel strip, les inserts, les sends, la sortie (et le VCA),
puis le panoramique à côté du mode d'automation, le fader et l'indicateur, le muet (**M**), le solo
(**S**), l'armement (**R**) et le nom de la piste, marqué **BUS** pour un dossier et **VCA** pour un
VCA. Le fader prend la hauteur que le panneau laisse.

- **Ancré** dans la fenêtre principale, la tranche laisse sa hauteur au fader : les boutons **FX**
  et **Send** à côté de l'indicateur (allumés, avec leur nombre, quand la liste contient quelque
  chose) ouvrent les effets et les départs de la tranche dans une fenêtre à part. Le master a
  **FX** et **Mon** (effets d'écoute).
- **Dans sa propre fenêtre** (Affichage › *Mixer dans sa propre fenêtre*, [[Ctrl+M]]), chaque
  tranche montre ses emplacements d'effets et de départs : cliquez sur un emplacement vide pour
  ajouter un effet ou un départ. Les listes ont la hauteur de la plus longue, plus un emplacement
  vide, et laissent le reste aux faders. [[Ctrl+M]] à nouveau le remet en place. *Mixer compact*
  ([[Shift+Alt+C]]) rétrécit les tranches.

![Le Mixer dans sa propre fenêtre](img/mixer-window.webp)

## Faders et panoramique

- **Les faders** suivent une loi de console (0 dB aux trois quarts de la course) : glissez-les,
  [[Shift]] pour la précision, utilisez la molette, double-cliquez pour 0 dB.
- **Panoramique** : la barre sous la sortie. Glissez le long ([[Shift]] pour des pas fins ; le
  centre retient le pointeur), la molette donne des pas de 1 %, un double-clic recentre.

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
- [[Shift]]-clic met un effet en bypass, [[Ctrl]]-clic le retire, le clic droit donne les autres
  options.
- Glissez un effet pour le réordonner, ou sur une autre tranche pour l'y copier ([[Alt]] : le
  déplacer). Déposé sur la liste, il va à l'endroit choisi ; ailleurs sur la tranche, en dernier.
- **Bypass** au-dessus des inserts met en bypass tous les effets de la tranche, et reste allumé
  tant qu'ils le sont ; sur le master, **ALL** met en bypass tous les effets de la session (un
  instrument en tête de piste continue de jouer).

**Les fenêtres d'effets** montrent le niveau qui entre et qui sort (crêtes et sonie) et le gain de
**Sortie** après l'effet, gardé dans le mix et l'export. **Match** est une aide à l'écoute : tant
qu'il est allumé, la sortie est ramenée à la sonie de l'entrée, pour comparer l'effet et son
bypass à niveau égal. Il n'est jamais enregistré ni exporté et s'éteint quand la fenêtre se ferme ;
**Garder** reporte sa correction dans la Sortie.

## Le rack

L'onglet **Rack** ([[Alt+6]]) montre la chaîne de la piste sélectionnée (ou d'une tranche
épinglée) en modules côte à côte, chacun avec ses réglages principaux, A/B, et son éditeur à un
clic. Les chaînes parallèles séparent le signal en branches et les remélangent.

![Le rack](img/rack.webp)

## Départs, dossiers et bus

- **Les départs** vont vers n'importe quelle piste, qui sert alors de bus (pré ou post fader). Un
  emplacement de départ vide ajoute le départ à toutes les pistes sélectionnées ; cochez d'abord
  *Couper la sortie vers le master* pour les pistes qu'on n'entend que par le bus.
- **Dossiers** : une piste peut en contenir d'autres ; sa tranche est leur bus. Le menu de sortie
  range les pistes sélectionnées dans un dossier, dans leur ordre.

## Groupes

Sélectionnez des pistes et appuyez sur [[Ctrl+G]], ou cochez *Le dossier sert de groupe* dans le
menu d'un dossier. Le menu Groupes règle ce que partagent les membres : niveau, panoramique et
départs (en relatif), muet, solo, armement, bypass des inserts, mode d'automation, polarité, et
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

| Console | Caractère |
|---|---|
| **Neutre** | aucune couleur : le mix tel quel |
| **British Warm** | transformateurs : graves ronds, aigus soyeux (inspirée des consoles Neve) |
| **British Bright** | serrée et précise, une touche d'air (inspirée des consoles SSL) |
| **American Punch** | amplis discrets : punch et attaque (inspirée des consoles API) |
| **Nashville** | douce et chaude, très faible distorsion (inspirée des consoles Harrison) |

*Dosage de la console* règle la quantité de couleur, *Souffle et diaphonie* la part de bruit de
la console. Chaque piste peut prendre une autre console (*Console de la piste*). Le bouton
marche/arrêt à côté du menu coupe toutes les couleurs au même niveau, pour comparer.

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

## Instantanés

Les **Instantanés** enregistrent l'état de la console et le rappellent, pour comparer deux mix.

## Master et référence

- Le **master** affiche la sonie (LUFS) et la plus haute crête d'échantillon à côté de son
  indicateur, remises à zéro par **Réinit.** sous le fader.
- **MONITOR** contient les effets qu'on entend mais qui ne sont pas exportés : correction de
  pièce, référence.
- **REF** à côté (aussi dans la barre de transport, [[Alt+R]]) compare le mix à une chanson finie
  (Transport › *Piste de référence…* pour la choisir). Cliquez dessus pour entendre la référence à
  la place du mix, au même endroit de la chanson et, par défaut, à la sonie du mix, pour comparer
  le son et non le niveau. Elle n'est jamais exportée.
