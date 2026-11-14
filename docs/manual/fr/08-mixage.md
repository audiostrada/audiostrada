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
  (destination, pré ou post fader, niveau, muet, *Supprimer le send*), la molette règle son
  niveau. Glissez un send pour le réordonner, ou sur une autre tranche pour l'y copier ([[Alt]] :
  le déplacer).
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
Chacune a une signature qui grandit avec la quantité : British Warm arrondit les graves et adoucit
le haut ; British Bright resserre le bas, avance le haut-médium avec un peu d'air, et son bus de
mix colle le mix (un compresseur doux, 1 à 2 dB à 50 %) ; American Punch donne du punch dans le
bas-médium et du mordant ; Nashville réchauffe le bas-médium, adoucit les aigus et élargit un peu
les bus.

*Quantité* : 25 % est la console telle que ses références la mesurent, aussi discrète qu'une
vraie au niveau de travail ; 50 % la pousse à chaud, comme sur les disques, là où elle s'entend ;
au-delà, son caractère est exagéré (6 dB plus chaud et trois fois la signature à 100 %) ; 0 %,
rien. Les bus et le master sont poussés deux fois moins qu'une piste, puisqu'ils reçoivent tout le
mix. La console garde le volume en continu : l'allumer ou changer la quantité ne change que la
couleur. Les projets enregistrés avant gardent leur son (leurs quantités sont divisées par deux à
l'ouverture). *Souffle et diaphonie* ajoute le bruit de fond de chaque tranche (souffle, un peu
de ronflement) et les côtés des bus qui débordent l'un sur l'autre. Chaque piste est une tranche à
part : ses composants diffèrent un peu de ceux de ses voisines, ce qui élargit le mix comme sur
une vraie console. Chaque piste peut prendre une autre console (*Console de la piste*). Le bouton
marche/arrêt à côté du menu coupe toutes les couleurs au même niveau, pour comparer.

**Test en aveugle › Lancer…** (ou Transport › *Test en aveugle des consoles…*) dit à partir de
quelle quantité vous entendez une console. Choisissez la console testée (celle du projet, ou une
des quatre), et écoutez d'abord librement le mix avec la console poussée à fond et sans elle. Puis
le duel : deux prises du mix, 1 et 2 (les touches 1 et 2 passent de l'une à l'autre), l'une passe
par la console, tirée au hasard ; dites laquelle. Le test commence à 100 % et descend par paliers
(75, 50, 35, 25, 15 %) : trois bonnes réponses passent un palier, deux erreurs sur un palier
l'arrêtent. Chaque réponse dit quelle prise passait par quelle console et à quelle quantité ; la
fin dit à partir de quelle quantité vous l'entendez, comparé au réglage de votre projet, et garde
les résultats de chaque console. Le test ne change rien au projet.

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

## Express Mix

**Express Mix**, sous le **Channel Strip** d'une tranche, règle la piste, ou toutes les pistes
sélectionnées : un clic la nettoie et la révèle, un clic droit (ou le clic droit sur la
piste, ou Piste › *Express Mix*) choisit l'un des deux ou retire l'Express Mix. Une fois
l'Express Mix appliqué, le bouton est allumé, et l'interrupteur à côté bypasse tout ce que
l'Express Mix a fait (les modules du Channel Strip qu'il a réglés, les effets ajoutés, le suivi
du volume, le pan, la couleur de console, l'envoi vers l'espace), pour comparer avec la piste
telle qu'elle était ; un second clic remet tout en place.
**Express Mix : toutes les pistes**, en haut du mixer, le fait pour toutes les pistes d'un coup
(pas les bus).

Piste › *Express Mix* sans piste sélectionnée prend toutes les pistes audio. L'Express Mix s'ouvre
dans une fenêtre à lui, que vous pouvez déplacer sur un autre écran.

Avec plusieurs pistes pas encore préparées, la fenêtre demande d'abord de **Vérifier les pistes**
pendant qu'elles sont écoutées : l'instrument de chacune trouvé d'après son nom et son son (à
corriger, ou à choisir quand rien ne le dit), si elle est incluse, son mute et son solo, et s'il
faut **Regrouper en bus**, avec les bus que cela ferait (allumé sauf si les pistes sont déjà dans
des bus). **Continuer** mène aux réglages, où tout cela reste modifiable.

Les réglages ont deux onglets. **Mix** règle toutes les pistes à la fois :

- **Style** : le son voulu pour le mix, avec une **Intensité** réglée sur toute la largeur de la
  fenêtre. Chaque style penche le timbre, dose la compression et les attaques, avance certains
  instruments, et apporte sa couleur de console (écrite sur sa tuile), son espace, son ambiance
  et sa largeur, que vous changez ensuite :
  - **Transparent** (par défaut) : comme dans la pièce, rien d'appuyé ; une main plus légère, sans
    couleur de console ajoutée (celle du projet reste).
  - **Chaleureux** : rond et doux, vintage ; aigus adoucis, bas-médiums plus pleins, compression
    plus lente ; British Warm.
  - **Clair** : défini et moderne, jamais agressif ; bas-médiums resserrés, un peu plus de
    présence, la voix devant ; British Bright.
  - **Percutant** : la rythmique devant ; plus de poids et d'attaque sur la grosse caisse, la
    caisse claire et la basse, une compression plus ferme, la batterie en avant ; American Punch.
  - **Ample** : grand, large et aérien ; plus de graves et d'air, une image plus large, une salle
    live ; Nashville.

  L'intensité dose aussi la console : 25 % (la console telle que mesurée) à 50 % d'intensité,
  50 % (poussée à chaud) à fond.
- Puis un potard par partie ; un double-clic revient à sa place habituelle :
  - **Nettoyer** : un passe-haut bien sous l'énergie qui compte (une octave sous la note la plus
    grave qui compte, doux, 12 dB par octave), un passe-bas sur la basse, la grosse caisse et les
    guitares électriques, des coupes seulement aux résonances qui ressortent vraiment, les
    plosives et les sifflantes d'une voix tenues, et un gate ou un expandeur doux là où il y a de
    la repisse entre les coups ou les phrases (batterie, voix, chœurs, guitares acoustiques) : la
    repisse d'un micro de chœur ne remonte plus avec le compresseur. Une piste avec de longs
    silences reçoit la suggestion de les supprimer.
  - **Révéler** : chaque instrument amené à son timbre habituel. AudioStrada mesure la
    part de l'énergie de la piste dans ses graves, ses bas-médiums, sa présence et son air, la
    compare à ce qu'a cet instrument dans un mix qui sonne juste, et ne fait bouger l'EQ du
    Channel Strip qu'une partie du chemin, seulement là où la piste s'en écarte nettement, dans
    des limites douces ; une piste déjà juste est laissée telle quelle. La grosse caisse et la
    basse reçoivent aussi un peu de poids. Puis un compresseur quand le niveau bouge (toujours sur
    une grosse caisse, une caisse claire et une basse), un Strada-Transient sur la batterie et les
    percussions et, si vous le cochez, un suivi du volume.
  - **Balance**, à 0 tant que vous ne la montez pas : amène les faders vers un mix équilibré, à
    la place habituelle de chaque instrument par rapport à la voix lead (grosse caisse un peu
    au-dessus, basse à son niveau, chœurs et guitares en dessous…), déplacée par le style. Le mix
    garde son volume.
  - **Espace** : combien de réverbe reçoit chaque instrument. La grosse caisse, la basse, le
    charley et les overheads restent secs.
  - **Ambiance** : quel espace, en quatre zones du potard, la taille grandissant dans chacune :
    **Studio** (sec et proche, une petite pièce), **Naturel** (une pièce pour la batterie et les
    percussions, une plaque pour les voix et la caisse claire, une salle pour le reste), **Live**
    (une salle de concert pour tout, un slapback court sur la voix lead), **Spatial** (une réverbe
    vaste et flottante, un écho à la croche pointée sur la voix lead, la batterie gardée plus
    près). Les queues suivent le tempo, leurs graves coupés pour ne jamais boueuser. Changer de
    zone pendant la lecture fait passer les réverbes par un fondu, pour que rien ne claque.
  - **Largeur** : les paires restées au centre (deux guitares, des chœurs, des toms…) s'écartent
    autour, en suivant les L et R de leurs noms ; une piste pannée à la main est laissée telle
    quelle.

Pour une seule piste, la fenêtre est la page de cette piste, avec ses réglages dans un bandeau
au-dessus : le style, puis **Nettoyer**, **Révéler**, **Espace** et, tant que le projet n'a pas
encore d'espace, **Ambiance**. Ce qui appartient à tout le mix est laissé de côté : le style ne
change ni la console du projet ni les bus, et il n'y a ni balance, ni largeur, ni marge pour le
master.

**Pistes** liste les pistes, sous les bus où elles vont, avec l'interrupteur **Regrouper en bus**
en haut : faites glisser un bus pour changer leur ordre. Chaque piste a son interrupteur (coupé,
elle reste comme elle est), son mute et son solo, et son instrument. Cliquez sur l'une pour voir
ce qui lui est proposé : sa place dans le mix (gain d'entrée, bus, espace, écho, pan), son spectre
avant et après, et les étapes de chaque partie, chacune avec son interrupteur et son dosage (50 %
est ce qui est proposé).

- **Gain staging**, toujours : le gain d'entrée de chaque piste l'amène d'abord à -18 dBFS là où
  elle joue, le niveau qu'attendent sa console et ses compresseurs ; les seuils proposés suivent,
  et le niveau entendu reste le même.
- **Même niveau**, toujours : ce que les traitements changent est rendu sur la sortie du Channel
  Strip, pour que chaque piste garde son volume et que plus fort ne passe jamais pour mieux. Le
  fader, c'est la Balance.
- **Marge pour le master**, d'elle-même quand le master n'a pas d'effets à lui : quand le mix
  dépasse -6 dBFS, un Strada-Utility en tête du master le baisse, et un Strada-Max en dernier le
  rend (12 dB au plus) avec ses crêtes sous -1 dB.
- **Modules réglés à la main** : un module du Channel Strip (filtres, gate, EQ, compresseur) que
  vous avez allumé et réglé vous-même est laissé tel quel.
- **Rien à zéro** : avec Nettoyer, Révéler, Balance, Espace et Largeur à 0 et le style
  Transparent, rien n'est touché : le mix est identique à l'original.

AudioStrada rend chaque piste seule, telle qu'elle arrive à son Channel Strip, plusieurs à la
fois, et devine l'instrument d'après le nom de la piste et son son : une voix d'homme ou de femme
par sa hauteur, un sifflement au-dessus de toute voix chantée, une guitare acoustique par les
aigus qu'aucun ampli ne donne, une voix à côté d'une piste nommée comme la voix lead (*Lead*,
*Main*…) comme des chœurs, et les cordes pincées comme un cuatro ou un ukulélé comme une guitare
acoustique. Quand rien ne le dit, la piste est mise en *Autre chose* et signalée.

Un bus (ou un dossier) est préparé comme une somme : un passe-haut doux et un compresseur lent qui
le colle (2:1), plus léger à mesure que ses pistes sont préparées, et sur la batterie la légère
montée des graves et des aigus que donne un EQ à lampes. Une fois l'instrument connu, l'icône de
la piste le suit (sauf si vous en avez choisi une).
- **Les deux**, et **Retirer l'Express Mix**.

Tant que la fenêtre est ouverte, la proposition s'entend : *Écouter* lance la session, **Avant** et
**Après** comparent. Au-dessus, une barre montre où en est la lecture, sur le profil du morceau,
avec ses marqueurs : cliquez ou glissez pour aller à un passage, cliquez sur le nom d'un marqueur
pour l'écouter. *Appliquer* écrit tout en une seule étape d'annulation ; *Annuler* ou fermer
la fenêtre laisse les pistes comme elles étaient.

Les réglages sont ceux du Channel Strip, plus un Strada-EQ après lui pour les résonances et les
plosives (ses bandes dynamiques quand elles vont et viennent), un Strada-DeEss et un
Strada-Transient. Relancer l'Express Mix, sur une piste ou toutes, met à jour ce qui a été fait :
rien n'est ajouté deux fois, et la fenêtre s'ouvre avec les derniers réglages appliqués, gardés
avec le projet. Ce sont des points de départ, à ajuster à l'oreille.

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
  le son et non le niveau. Sa fenêtre montre la sonie des deux et de combien de dB elles
  diffèrent. Elle n'est jamais exportée.
