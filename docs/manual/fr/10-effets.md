# Effets

AudioStrada fournit trente-deux effets et instruments intégrés, la série Strada. Chacun a des presets
d'usine, des presets personnels et la comparaison A/B en haut de sa fenêtre. Ce chapitre les
parcourt famille par famille ; les tableaux de paramètres donnent la plage et la valeur par défaut
de chaque réglage.

## Presets et chaînes

- **Presets** : la barre en haut de la fenêtre d'un effet montre le preset chargé, avec un point
  quand ses réglages ont changé depuis. Les flèches à côté, ou [[Left]] et [[Right]], parcourent
  les presets ; clic droit sur une flèche › *Apprentissage MIDI*, puis appuyez sur un bouton de
  votre contrôleur MIDI pour les parcourir depuis celui-ci, fenêtre ouverte ou non. Cliquez sur son nom pour le panneau des presets : une recherche, les catégories,
  vos favoris, les presets d'usine et les vôtres à part. L'oreille à droite d'un preset le fait
  entendre (un second clic l'arrête), comme le parcourir avec [[Up]] et [[Down]] ; [[Enter]] le garde, [[Escape]] revient au réglage
  d'avant. Clic droit sur l'un des vôtres pour le renommer ou le supprimer. L'étoile ajoute le
  preset à vos favoris, la disquette enregistre les réglages (tapez *Catégorie : nom* pour le
  ranger). **A** / **B** retiennent deux réglages à comparer, le bouton à côté copie l'un sur
  l'autre.
- **Les presets d'usine** commencent par *Par défaut*, les réglages propres à l'effet, puis
  vont par source ou par usage (*Voix*, *Batterie*, *Basse*, *Bus*, *Master*,
  *Correction*...), de trois à six sous chacun. Ils sont réglés sur des enregistrements de ce
  qu'ils nomment, pour marcher tout de suite sur une piste dont les crêtes tournent autour de
  -6 dBFS : ceux qui façonnent un son le rendent au volume où il est entré, et le seuil d'un
  compresseur est le premier réglage à bouger pour une piste plus faible ou plus forte. Ceux
  rangés sous *Send* sont à 100 % d'effet, pour un effet sur un bus alimenté par des sends ;
  les presets *Sidechain* et *Ducker* attendent la clé d'une autre piste.
- **Bypass**, à droite de la barre, s'allume en ambre tant que l'effet est bypassé.
- **Les chaînes d'effets** gardent toute une liste d'effets avec leurs réglages : clic droit sur
  un effet (ou menu de la piste) › *Enregistrer les effets comme chaîne* (sa fenêtre liste les
  effets enregistrés et prévient quand le nom remplace une de vos chaînes), puis ajoutez-la à
  n'importe quelle piste par *+ insert* › *Chaînes*. Chaînes d'usine : voix, bus batterie, guitare
  acoustique, master pour le streaming.

Les fenêtres d'effets sont des fenêtres à part, qui peuvent aller sur un autre écran ; recliquer
sur un insert ramène sa fenêtre au premier plan.

## Chaînes de mastering et de voix

### Strada-Master

![Strada-Master](img/fx/mastering.webp)

Une chaîne de mastering derrière six gros potards : **Timbre** (plus sombre ou plus brillant :
deux plateaux et un plateau d'air dans les dernières bandes de Strada-EQ), **Cohésion** (les seuils de Strada-MB-Comp), **Couleur**
(Strada-Tape), **Largeur** (Strada-Wide, le grave gardé au centre), **Sub** (plus ou moins de la
dernière octave : un plateau grave à 70 Hz, la dernière bande de Strada-EQ, Strada-Wide la gardant au centre) et **Volume** (l'attaque de Strada-Max, Strada-Limit
gardant les crêtes vraies sous le plafond). Au-dessus, un **style** (*Clair*, *Chaleureux*,
*Ouvert*, *Percutant*) donne aux étages un point de départ, et une **cible** règle le plafond du
limiteur et le volume visé (en choisir une déplace le potard Volume de l'écart entre les cibles,
puis l'ajuste à l'oreille sur quelques secondes de musique) : *Streaming* -14, *Apple Music, podcast* -16, *CD* -9 (crêtes
jusqu'à -0,3 dB), *Club* -7, *Vinyle* -12 (graves en mono, marge sous les crêtes), *TV, radio*
-23 LUFS (EBU R128), et *Pré-master*, pour un ingénieur de mastering : rien de maximisé, crêtes
tenues à -6 dBFS. **Suivre** atteint la cible, puis
la garde à chaque réglage changé (un potard, le style, les réglages d'un étage) : chaque
changement est mesuré sur quelques secondes, la sortie contre l'entrée, et compensé. Les
nuances de la musique ne le font jamais bouger : passages doux et forts gardent leur écart. **Avant, même volume** fait entendre le mix sans
Strada-Master, porté à son volume, pour juger le traitement sans que le volume parle à sa
place ; il s'arrête à la fermeture de la fenêtre.

L'affichage dessine le spectre du morceau dans le tunnel du style, son volume des dernières
secondes face à la cible, l'image stéréo, la crête vraie et la réduction du maximiseur.

La barre du haut est la chaîne : son premier onglet, *Strada-Master*, montre les potards ; les
autres ouvrent l'éditeur de chaque étage, presets compris. Glissez un étage pour changer l'ordre
dans lequel le son les traverse ; Strada-Limit reste le dernier. Chaque potard agit autour des
réglages de l'étage qu'il pilote, sans les changer ; l'éditeur d'un étage montre ses réglages
tels qu'ils sont joués, potards compris, et ce que vous y changez devient la base autour de
laquelle les potards agissent. **Écouter et régler**, proposé à la première
ouverture, fait entendre à l'assistant huit secondes du passage le plus fort : il règle Timbre,
Cohésion, Largeur, Sub et Volume pour le mix et la cible, puis ajuste le volume une fois la sortie
stabilisée. Le curseur entre *Vos réglages* et *Assistant* dose sa proposition avec ce que vous
aviez ; *Annuler* remet vos réglages.

<!-- effect:mastering -->

### Strada-Vocals

![Strada-Vocals](img/fx/vocals.webp)

Une chaîne vocale derrière six gros potards : **Justesse** (Strada-Correct : plus il est fort,
plus la correction est rapide et ferme), **Timbre** (plus chaud ou plus brillant, avec de l'air
en haut) et son petit potard **Saturation** (Strada-Drive, d'une touche de chaleur au grain),
**Comp** (le seuil de Strada-Comp), **Dé-ess** (Strada-DeEss), **Doublage** (Strada-Doubler) et
**Espace** (Strada-Delay et Strada-Verb, qui s'effacent sous la voix). La **voix** (*Chant*,
*Rap*, *Parlé*) et l'**espace** (*Pièce*, *Plaque*, *Salle*, *Slapback*, *Écho*) donnent aux
étages un point de départ. L'affichage dessine le spectre de la voix face à sa cible, la
justesse des dernières secondes, la note, la compression et le dé-essage. **Prise**, pendant
l'enregistrement, laisse de côté la justesse et l'espace, et avec eux leur latence.

Comme dans Strada-Master, la barre de la chaîne ouvre et réordonne les étages, et **Écouter et
régler** règle Timbre, Comp et Dé-ess pour la voix entendue, les seuils du compresseur et du
dé-esseur à son niveau, et la tonalité de Strada-Correct d'après les notes chantées.

<!-- effect:vocals -->

## Dynamique

### Strada-Comp

![Strada-Comp](img/fx/compressor.webp)

Un compresseur à quatre caractères : **Clair** pour un contrôle transparent, **Bus** pour la
cohésion, **Opto** pour un nivellement doux et **FET** pour le punch. Seuil, taux, attaque et
relâchement comme d'habitude ; le relâchement automatique suit la musique, la couleur ajoute la
saturation du caractère, le mélange remet du signal sec (compression parallèle), le filtre du
sidechain évite que les graves fassent pomper.

<!-- effect:compressor -->

### Strada-Gate

![Strada-Gate](img/fx/gate.webp)

Gate, expandeur et ducker, avec hystérésis (il s'ouvre et se ferme à des niveaux différents, donc
il ne bégaie pas), anticipation, maintien et plage. Les filtres du sidechain et l'écoute du
sidechain l'aident à n'entendre que le fût voulu.

<!-- effect:gate -->

### Strada-Transient

![Strada-Transient](img/fx/transient.webp)

Plus ou moins de punch et de sustain sur les batteries et les sons pincés, sans seuil : l'attaque
renforce ou adoucit les coups, le sustain la résonance qui suit.

<!-- effect:transient -->

### Strada-DeEss

![Strada-DeEss](img/fx/deesser.webp)

Adoucit les S et les T agressifs des voix. Réglez la fréquence avec l'écoute activée, puis le
seuil ; le mode agit sur les aigus seuls ou sur toute la voix.

<!-- effect:deesser -->

### Strada-MB-Comp

![Strada-MB-Comp](img/fx/mb_comp.webp)

Un compresseur multibande : jusqu'à quatre bandes, séparées aux fréquences de coupure, chacune
compressée par le haut et relevée par le bas. Le solo fait entendre une bande seule.

<!-- effect:mb_comp -->

### Strada-Limit et Strada-Max

![Strada-Max](img/fx/maximizer.webp)

Les deux ont l'allure d'un maximiseur de mastering : l'indicateur d'entrée à gauche avec le
**seuil** (descendez-le pour pousser l'entrée plus fort), l'indicateur de sortie à droite avec le
**plafond**, et entre les deux la musique qui défile, en rouge là où le limiteur retire des
crêtes. En dessous : réduction de gain, crête de sortie, sonie à court terme, et le gain de sonie
obtenu.

**Apprendre** écoute pendant que vous jouez le passage le plus fort et règle le seuil pour que la
sortie atteigne la sonie choisie (-14 LUFS pour le streaming, par exemple). Strada-Max est le
limiteur de mastering, avec un plafond en crête réelle et une anticipation ; Strada-Limit est un
limiteur de crêtes pour les pistes et les bus. **Avant, même volume** fait entendre l'entrée sans le limiteur, portée au
volume de sa sortie, pour juger la limitation elle-même ; il s'arrête à la fermeture de la
fenêtre.

<!-- effect:maximizer -->

<!-- effect:limiter -->

### Strada-Trigger

![Strada-Trigger](img/fx/trigger.webp)

Renforce ou remplace une grosse caisse ou une caisse claire par des fûts échantillonnés, coup pour
coup. Les coups de la piste s'affichent en direct sous un seuil à tirer. Choisissez le fût et son
son (studio, garage, profond, boom, vintage, rock… ou votre échantillon), puis deux faders : micros
de proximité vers la pièce, et la piste vers le trigger.

<!-- effect:trigger -->

### Strada-Channel

Une tranche de console dans un seul insert : filtres, gate, compresseur, égaliseur à quatre bandes
et saturation, dans l'ordre que vous choisissez (voir *Mixage*). **Auto**, à côté de la sortie,
garde la sortie au volume de l'entrée, quoi que les modules ajoutent ou retirent, pour comparer à
l'oreille sans que le volume vous trompe ; le bouton *Sortie* l'ajuste.

<!-- effect:channel -->

## Égaliseur et filtre

### Strada-EQ

![Strada-EQ](img/fx/eq.webp)

Jusqu'à 16 bandes sur la courbe. Double-cliquez sur la courbe pour ajouter une bande (un coupe-bas
ou coupe-haut près des bords, une cloche ailleurs), glissez une bande pour la déplacer,
[[Ctrl]]-glisser (vers le haut plus étroite, vers le bas plus large) ou la molette sur elle règle
son Q, un double-clic la supprime.

Le clic droit sur une bande donne sa forme, dessinée (cloche, plateaux grave et aigu, coupe-bas et
coupe-haut, coupe-bande, passe-bande, pente inclinée), la pente d'une coupe (6 à 48 dB par octave), son
placement (deux canaux, gauche, droite, mid ou côtés), *Dynamique*, *Solo* et *Bypass*, *Gain à
0 dB* et *Inverser le gain*. Sous la courbe, la bande sélectionnée a les mêmes formes dessinées, sa
pente et son placement, puis sa fréquence, son gain et son Q.

- Une bande **dynamique** écoute ses propres fréquences à l'entrée (autour de sa fréquence pour une
  cloche, en dessous pour un plateau grave, au-dessus pour un plateau aigu) et bouge son gain tant
  qu'elles dépassent le *Seuil* : d'autant de dB qu'elles le dépassent, jusqu'à l'*Étendue*, à la
  vitesse réglée par l'*Attaque* et le *Relâchement*. Une étendue négative baisse quand c'est fort
  (une résonance qui ne sonne que par moments, les sifflantes, une note qui bourdonne) ; une étendue
  positive monte quand c'est fort (le claquant d'une caisse claire, sur ses coups seulement). Pour
  monter quand c'est faible, donnez à la bande un gain et une étendue négative de même taille : elle
  monte au repos et revient à plat quand ses fréquences deviennent fortes. Le panneau dit ce que
  fait la bande et son gain du moment ; sur la courbe, son étendue s'affiche en trait sous la bande,
  son gain du moment en anneau.
- **Solo** ne fait entendre que les fréquences où agit la bande, pour trouver quoi couper ; il
  s'arrête quand la fenêtre se ferme.

<!-- effect:eq -->

### Strada-Tube EQ

![Strada-Tube EQ](img/fx/vintage_eq.webp)

Un égaliseur passif à lampes : la fameuse astuce du grave relevé et coupé à la même fréquence (les
graves gonflent pendant que le bas-médium se dégage), des aigus soyeux et la chaleur des lampes.

<!-- effect:vintage_eq -->

### Strada-Filter

![Strada-Filter](img/fx/filter.webp)

Un filtre résonant à balayage, avec saturation, LFO (calé sur le tempo ou libre) et suiveur
d'enveloppe : du mouvement discret au balayage sauvage.

<!-- effect:filter -->

## Espace

### Strada-Verb

![Strada-Verb](img/fx/reverb.webp)

Choisissez un espace : Pièce, Salle, Espace, Plaque, Ressort ou Shimmer (survolez un bouton pour
savoir comment il sonne). Le Shimmer est un vaste espace dont la traîne monte d'une octave en
résonnant ; son bouton Shimmer en règle la quantité.

Puis une couleur, le caractère d'une époque : **Clair** garde la traîne pleine et lisse ;
**Années 80**, les réverbes numériques des années 80, la limite à 11 kHz avec le grain de 12
bits et un chorus ample ; **Vintage**, les premières réverbes numériques et les plaques des années 70, la
rend plus sombre (7 kHz), plus granuleuse et moins dense, avec un léger souffle qui suit la
traîne.

L'affichage montre combien de temps chaque fréquence résonne, en secondes, sur le spectre de la
traîne (dans la teinte de la couleur) et celui de l'entrée (une ligne) ; les coupes ombrent ses
bords, et des pointillés marquent un temps et une mesure au tempo du projet. Glissez ses points :
le vert pour le déclin, l'orange pour la durée des graves et jusqu'où ils vont, le bleu pour les
aigus, le rouge et le violet pour le coupe-bas et le coupe-haut. Double-cliquez sur un point pour
le remettre à zéro. *Caler sur le tempo* règle le déclin sur des quarts de mesure entiers et le
pré-délai sur une valeur de note.

Les réglages principaux sont les plus grands : le *Déclin*, avec *Geler* (la traîne tenue pour
toujours, pour des nappes), le *Pré-délai*, en millisecondes ou, avec *Synchro*, en valeur de
note, et le *Mix*. À côté viennent les réflexions (taille, précoces / tardives, diffusion), le
mouvement (profondeur et vitesse de la modulation), le ducking (de combien la traîne baisse
pendant que l'entrée joue, et à quelle vitesse elle revient), la tonalité (les boutons des points
de l'affichage), la largeur et la quantité de Shimmer. Ceux qu'un espace n'utilise pas sont
grisés.

<!-- effect:reverb -->

### Strada-Delay

![Strada-Delay](img/fx/delay.webp)

Choisissez une machine : **Numérique** donne des répétitions nettes, et un nouveau temps arrive
en fondu ; **Bande** des répétitions qui ondulent, saturent et s'assombrissent, et un changement
de temps glisse comme un varispeed ; **Analogique** (bucket brigade) des répétitions chaudes,
plus sombres à mesure que le temps s'allonge.

À gauche, les échos sur la grille du tempo, la gauche en haut et la droite en bas, s'allument
quand on les entend ; le ping-pong dessine leur trajet. Glissez le premier écho de côté pour le
temps, en hauteur pour la réinjection, et le premier écho de droite de côté pour son décalage. À
droite, la tonalité des répétitions sur leur spectre : la courbe jaune est un passage dans la
boucle, les plus pâles les passages suivants, plus sombres à chaque fois ; glissez les points
rouge et violet pour le coupe-bas et le coupe-haut.

Les réglages principaux sont les plus grands : le *Temps*, en millisecondes avec *Tap*, ou avec
*Synchro* en valeur de note au tempo du projet ; la *Réinjection*, avec *Ping-pong* et *Geler* (ce
qui est dans le delay se répète pour toujours, sans l'entrée) ; et le *Mix*. À côté viennent les
coupes, le wow & flutter et le drive (bande et analogique seulement), le décalage de la droite,
et le ducking (les échos baissés pendant que l'entrée joue, revenant dans les blancs).

<!-- effect:delay -->

## Modulation

### Strada-Chorus

![Strada-Chorus](img/fx/chorus.webp)

De courts délais modulés pour la largeur et le scintillement, à une voix, deux, ou en ensemble ;
avec de la réinjection, il devient un flanger.

<!-- effect:chorus -->

### Strada-Flanger

Le flanger « avion à réaction » classique, avec réinjection positive ou négative, balayage stéréo
et *Through zero*, le flanging à bande des années soixante.

<!-- effect:flanger -->

### Strada-Phaser

Un phaser tourbillonnant de 4 à 12 étages, avec réinjection et écart stéréo.

<!-- effect:phaser -->

### Strada-Mod

![Strada-Mod](img/fx/modulator.webp)

Trémolo, panoramique automatique, découpe rythmique (un séquenceur pas à pas du niveau), vibrato
et modulation en anneau, calés sur le tempo.

<!-- effect:modulator -->

### Strada-Pitch

![Strada-Pitch](img/fx/pitch.webp)

Un transpositeur à deux voix, avec délai et réinjection : harmonies, octaves, élargissement, et
échos scintillants.

<!-- effect:pitch -->

## Couleur

### Strada-Drive

![Strada-Drive](img/fx/drive.webp)

Choisissez un type (Overdrive, Distorsion, Fuzz, Lampe, Bande, Transformateur, Redresseur). Le
*Drive* va du son propre à l'excès par pas réguliers : le premier tiers colore un peu une basse ou
un bus, le haut donne une fuzz. Les crêtes gardent leur niveau quel que soit le drive ; le *Gain
auto* égale le volume à la place, pour juger la couleur et non le volume. *Sortie* ajuste le
niveau, *Mix* remêle le son direct.

L'affichage montre le spectre de l'entrée (une ligne) et celui de la sortie, où brillent les
harmoniques ajoutées. La ligne orange montre à quel point chaque fréquence est poussée : glissez
ses points pour le drive, le *Coupe-bas* qui garde les graves hors de la saturation, et le
*Focus*, une bande poussée plus fort (pour faire ressortir une basse) ou moins que le reste, sans
changer sa tonalité. La ligne blanche est la tonalité après le drive : glissez ses points pour les
*Basses*, les *Médiums* (et leur fréquence) et la *Tonalité*, un coupe-haut. Double-cliquez sur un
point pour le remettre à zéro. Le *Bias* rend l'écrêtage asymétrique (plus chaud, harmoniques
paires).

<!-- effect:drive -->

### Strada-Tape

![Strada-Tape](img/fx/tape.webp)

Un magnétophone, dessiné avec ses bobines qui tournent à la vitesse choisie et ses VU-mètres.
Quatre machines : **Studio A80** (magnéto de studio 2 pouces : graves percutants, médiums
fermes), **Mastering 102** (magnéto de mastering demi-pouce : large, propre et doux), **Vintage
J37** (quatre pistes : plus sombre, plus rond, sature tôt), **Cassette** (aigus doux, pleurage et
souffle). Trois bandes : **GP9** (moderne, la plus grande marge), **456** (la classique, compresse
quand on la pousse), **250** (vintage, sature tôt). Vitesse, bosse de tête, pleurage et
scintillement, souffle et bias la complètent.

<!-- effect:tape -->

## Stéréo

### Strada-Wide

![Strada-Wide](img/fx/imager.webp)

La largeur par bande, des graves en mono et un goniomètre : des mix larges qui tiennent en mono.
La stéréoïsation crée une image stéréo à partir d'une source mono.

<!-- effect:imager -->

### Strada-Doubler

![Strada-Doubler](img/fx/doubler.webp)

Le doublage : des copies désaccordées, retardées et qui dérivent, étalées dans la stéréo, pour
des voix et des guitares enregistrées une seule fois.

<!-- effect:doubler -->

## Utilitaires

### Strada-Utility

![Strada-Utility](img/fx/utility.webp)

Gain, balance, largeur, mono et mono sous une fréquence, polarité par canal, inversion des
canaux, alignement temporel en millisecondes et rotation de phase.

<!-- effect:utility -->

### Strada-Correct

La correction de hauteur automatique pour la voix, de la touche légère à la voix en paliers,
synthétique. L'écran fait défiler la hauteur chantée (en gris) et la hauteur corrigée (en bleu)
sur les notes de la gamme ; à sa droite, la note chantée et son écart, la note visée, et une
barre qui montre de combien de cents la voix est déplacée.

- **Naturel** suit les réglages : la *vitesse de correction* est le temps que met la voix à
  rejoindre sa note (0 ms, c'est immédiat ; de 20 à 60 ms, on entend un chanteur juste),
  *Humaniser* corrige le centre d'une note tenue et lui laisse son vibrato, la *tolérance*
  laisse tranquille une note déjà aussi proche, la *quantité* ne corrige qu'une partie du
  chemin.
- **Escalier** pose chaque note à plat sur sa cible, tout de suite, quels que soient ces
  réglages : la voix avance par paliers, sans vibrato ni glissé, comme sur les premiers
  disques corrigés automatiquement. Choisissez une gamme de peu de notes pour des marches
  plus larges.
- **Harmonie** corrige la voix comme *Naturel* et y ajoute une seconde voix qui chante dans
  la tonalité : une tierce, une quinte, une sixte ou une octave au-dessus, une tierce, une
  quarte ou une octave en dessous, comptées dans les notes de la gamme (celles de la gamme
  majeure de la tonalité quand la gamme est chromatique). *Niveau* dose la seconde voix
  contre la première ; les deux ensemble gardent le volume d'une seule.

Choisissez la tonalité et la gamme (ou cliquez des notes sur le clavier pour une gamme
personnalisée). Pendant que la voix joue, l'effet écoute la tonalité de ce qui est chanté et
l'affiche sous le clavier (*Entendu : A mineur*) ; **Utiliser cette tonalité** règle la
tonalité et la gamme dessus. *Transposer* déplace toute la voix par demi-tons, formants
conservés.

<!-- effect:pitch_correct -->

### Strada-Scope

![Strada-Scope](img/fx/analyzer.webp)

Un analyseur de spectre, avec maintien des crêtes, pente et choix de ce qu'il montre.

<!-- effect:analyzer -->

### Strada-Tuner

Un accordeur chromatique pour la guitare, la basse et la voix ; il peut couper la piste pendant
l'accordage.

<!-- effect:tuner -->

## Ampli et baffle

### Strada-Amp

![Strada-Amp](img/fx/amp_rig.webp)

Tout un équipement de guitare ou de basse : jusqu'à huit pédales, un ampli et un baffle. La
fenêtre a trois sections : pédales, ampli et baffle.

- **Pédales** : cliquez sur **+** pour en ajouter une ; au survol d'une pédale, le crayon la
  change et la croix la retire ; glissez une pédale pour la déplacer. Son interrupteur l'allume ou
  l'éteint. 21 pédales d'après les classiques : Tube Screamer, Klon, Blues Driver, SD-1, DS-1,
  RAT, Metal Zone, Fuzz Face, Big Muff, Rangemaster, wah Cry Baby, compresseur, modulations, delay
  analogique et à bande, réverbération.
- **Amplis** : les flèches à côté de l'ampli les font défiler ; cliquez sur son nom pour la liste
  complète. 16 modèles **intégrés** d'après les amplis les plus joués (Twin, Super et Deluxe
  Reverb, Tweed Deluxe, Bassman, AC30, JTM45, Plexi, JCM800, Friedman, Dumble, Dual Rectifier,
  5150, Diezel, SVT, B-15), chacun avec son préampli, son correcteur et son étage de puissance, et
  un baffle assorti.
- **Ma bibliothèque** liste vos captures Neural Amp Modeler par catégorie (Amplis, Amplis +
  baffle, Pédales, Autres, Favoris). *Ajouter des captures…* copie des fichiers `.nam` dans la
  bibliothèque d'AudioStrada, rangés par matériel ; le clic droit sur une carte la renomme, change
  sa catégorie, la met en favori ou la retire. Un dossier de captures d'un même ampli forme un seul
  ampli : son bouton Gain passe de l'une à l'autre. Une capture fige l'ampli à un réglage : Graves,
  Médiums, Aigus, Présence et Master façonnent le son après elle.
- **TONE3000** : *Parcourir TONE3000…* ouvre ses captures gratuites par genre (amplis, effets,
  baffles) : tendances, nouveautés, vos favoris et vos téléchargements. Un clic télécharge un son
  et le charge à sa place. La connexion (compte gratuit) montre vos favoris.

<!-- effect:amp_rig -->
