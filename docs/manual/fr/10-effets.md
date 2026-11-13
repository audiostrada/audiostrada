# Effets

AudioStrada fournit trente effets et instruments intégrés, la série Strada. Chacun a des presets
d'usine, des presets personnels et la comparaison A/B en haut de sa fenêtre. Ce chapitre les
parcourt famille par famille ; les tableaux de paramètres donnent la plage et la valeur par défaut
de chaque réglage.

## Presets et chaînes

- **Presets** : la barre en haut de la fenêtre d'un effet montre le preset chargé, avec un point
  quand ses réglages ont changé depuis. Les flèches à côté, ou [[Left]] et [[Right]], parcourent
  les presets ; clic droit sur une flèche › *Apprentissage MIDI*, puis appuyez sur un bouton de
  votre contrôleur MIDI pour les parcourir depuis celui-ci, fenêtre ouverte ou non. Cliquez sur son nom pour le panneau des presets : une recherche, les catégories,
  vos favoris, les presets d'usine et les vôtres à part. Le parcourir à la souris ou avec [[Up]]
  et [[Down]] fait entendre chaque preset ; [[Enter]] le garde, [[Escape]] revient au réglage
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
  un effet (ou menu de la piste) › *Enregistrer les effets comme chaîne*, puis ajoutez-la à
  n'importe quelle piste par *+ insert* › *Chaînes*. Chaînes d'usine : voix, bus batterie, guitare
  acoustique, master pour le streaming.

Les fenêtres d'effets sont des fenêtres à part, qui peuvent aller sur un autre écran ; recliquer
sur un insert ramène sa fenêtre au premier plan.

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
limiteur de crêtes pour les pistes et les bus.

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

Le clic droit sur une bande donne son type (cloche, plateaux grave et aigu, coupe-bas et coupe-haut
de 6 à 48 dB par octave, réjecteur, passe-bande, bascule), *Bande dynamique*, *Solo* et *Bypass*.
Sous la courbe : le type, la pente, la fréquence, le gain et le Q de la bande sélectionnée.

- Une bande **dynamique** bouge son gain jusqu'à sa *Plage* quand ses fréquences dépassent le
  *Seuil* : une plage négative calme une résonance seulement quand elle sonne. Sa plage s'affiche
  en trait sous la bande, son gain du moment en anneau.
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
savoir comment il sonne). Le Shimmer est un vaste espace dont la queue monte d'une octave en
résonnant ; son bouton Shimmer en règle la quantité.

La décroissance est en secondes ; le pré-délai retarde la queue par rapport au son direct ; le
coupe-haut et le coupe-bas la taillent ; la largeur l'étale ; le ducking la baisse pendant que
l'entrée joue (la queue revient dans les blancs) ; le mélange règle ce qu'on entend. *Freeze*
tient la queue pour toujours, pour des nappes.

Les petits boutons règlent la taille, premières réflexions / queue, l'amortissement des aigus et
des graves, la diffusion et la modulation ; ceux qu'un espace n'utilise pas sont grisés.
L'affichage montre comment s'éteignent les graves et les aigus.

<!-- effect:reverb -->

### Strada-Delay

![Strada-Delay](img/fx/delay.webp)

Des échos numériques, à bande ou analogiques. **Numérique** donne des répétitions nettes ;
**Bande** des répétitions qui ondulent, saturent et s'assombrissent, et un changement de temps
glisse comme un varispeed ; **Analogique** (bucket brigade) des répétitions plus sombres à mesure
que le temps s'allonge. Calage sur le tempo, ping-pong, décalage du canal droit, ducking et
freeze.

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

Choisissez un type (Overdrive, Distorsion, Fuzz, Lampe, Bande, Transformateur, Redresseur) ; la
courbe montre comment il plie le signal. Drive, Tonalité, Niveau et Mélange sont les boutons de la
pédale ; le coupe-bas resserre les graves avant la saturation, le Bias rend l'écrêtage asymétrique
(plus chaud, harmoniques paires), Graves et Médiums façonnent le son après.

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

La correction de hauteur automatique pour la voix, du naturel à l'effet robot. Choisissez la
tonalité et la gamme (ou des notes personnalisées) ; une vitesse de correction de 0 ms donne
l'effet dur, des valeurs plus hautes une correction naturelle, *Humaniser* laisse un peu de vie
aux notes tenues.

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
