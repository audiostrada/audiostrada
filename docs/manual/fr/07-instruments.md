# Instruments

AudioStrada fournit deux instruments, **Strada-Synth** et **Strada-Drums**, et joue tout
instrument CLAP, VST3, VST2 ou LV2. Une nouvelle piste instrument ouvre le navigateur sur les
instruments pour choisir le sien. Les instruments sont en tête de leur piste ; plusieurs peuvent
la mener, superposés : ils jouent les mêmes notes, leurs sons s'additionnent, et les effets qui
suivent traitent le mélange. Clic droit sur l'insert d'un instrument › *Ajouter un instrument…*
pour en superposer un autre, *Changer d'instrument…* pour remplacer le premier. Les pistes boîte
à rythmes commencent avec Strada-Drums et se superposent de la même façon.

## Strada-Synth

![Strada-Synth](img/fx/synth.webp)

Un synthétiseur polyphonique : deux oscillateurs avec unisson et tables d'ondes, un
sous-oscillateur et du bruit, un filtre, deux LFO, un arpégiateur et ses propres chorus, delay
et réverbe. Il s'ouvre sur sa vue **Simple**, faite pour jouer. À droite de ses vues, *Armer
la piste* arme la piste pour votre clavier MIDI sans repasser par le mixeur, et *Panique* coupe
toutes les notes, celles d'un arpège maintenu aussi. Une fenêtre
maximisée garde sa taille, son image ou son oscilloscope prenant la place restante.

- **Le son** s'affiche en haut, dessiné par son onde dans la couleur de sa famille : un paysage
  pour les nappes et les cordes, des anneaux pour les cloches et les plucks, un ruban pour les
  basses et les leads. Cliquez sur l'image pour en voir une autre, double-cliquez pour lui
  donner toute la fenêtre, dans toutes les couleurs. Choisissez un autre son dans les presets,
  ou passez de l'un à l'autre avec les flèches gauche et droite.
- **Quatre macros**, chacune nommée d'après ce qu'elle fait à ce son (*Brillance*, *Mouvement*,
  *Espace*...) : un potard pilote plusieurs réglages à la fois, si bien qu'un quart de tour
  s'entend nettement. Une macro à zéro signifie rien de ce qu'elle nomme : un son qui vient
  avec du vibrato ou de la réverbe s'ouvre avec cette macro déjà tournée.
- **Sub** et **Bruit**, à côté, ajoutent le sous-oscillateur (une sinusoïde pure sous le son) et
  du bruit (souffle, air).
- Clic droit sur l'un de ces potards pour l'assigner à un potard de votre contrôleur MIDI, ou
  l'automatiser.
- En dessous : l'**arpégiateur** (marche, maintien, ordre, vitesse, étendue et durée des
  notes), le **chorus**, le **delay** et la **réverbe**, puis le **glissement** d'un son qui
  joue une note à la fois ou le **relâchement** d'un son qui joue des accords, et le **volume**.

Quatre vues montrent le synthé lui-même, quelques panneaux chacune : **Oscillateurs** (avec les
tables d'ondes, le sub et le bruit), **Filtre et enveloppe**, **Modulation** (les LFO, la
seconde enveloppe et la liste de ce qu'ils modulent) et **Arpégiateur et effets**. En bas de
chacune, les sources (LFO, seconde enveloppe, macros, vélocité, molette de modulation,
pression) : glissez-en une sur un potard pour le moduler. **Expert** ajoute les réglages dont
peu de sons ont besoin : accord fin, FM, synchro et anneau, suivi du clavier, dérive,
profondeur des LFO et MPE.

**Macros** montre les quatre macros, chacune avec ce qu'elle pilote et de combien : cliquez sur
le nom d'une macro pour en choisir un autre, sur *Ajouter une cible* pour lui faire piloter un
réglage de plus. Une macro (comme la molette de modulation) atteint aussi les effets, les temps
de l'enveloppe, le désaccord, la vitesse et la profondeur des LFO.

- **Les presets** contiennent environ quatre-vingt-cinq sons prêts, par famille : basses, leads,
  claviers, nappes, cordes, cuivres, plucks, cloches, poly, arpèges, ambiances et effets. Tous
  jouent au même niveau.
- **Le clavier** joue le synthé sans armer la piste : cliquez ou glissez sur les touches (plus bas
  sur une touche joue plus fort), ou activez *Clavier de l'ordinateur* pour jouer avec les
  lettres : la rangée du milieu pour les touches blanches, celle du dessus pour les noires, deux
  touches de la rangée du bas pour l'octave. Les touches comptent par leur position : AZERTY et
  QWERTY jouent pareil.
- **MPE** (dans les réglages experts) est pour les contrôleurs MPE (Roli Seaboard, LinnStrument,
  Osmose), qui envoient chaque note sur son propre canal.

<!-- effect:synth -->

## Strada-Drums

![Strada-Drums](img/fx/drums.webp)

Piste › *Ajouter une piste boîte à rythmes* crée une piste avec Strada-Drums : seize pads sur les
notes de batterie General MIDI, pour que le piano roll et tout contrôleur à pads les jouent.

### Pads

Un pad s'allume quand il sonne et joue quand on clique dessus (plus bas sur le pad joue plus
fort). Le clic le sélectionne aussi, et l'**onglet du pad** en dessous montre son son :

- **Caractère** : 808, 909 ou acoustique, ou *Charger…* un échantillon à vous.
- **Son** : niveau, panoramique, accord. **Enveloppe** : attaque, décroissance. **Hauteur** :
  quantité et durée de la chute de hauteur. **Filtre** : timbre, coupe-bas, coupe-haut.
  **Sends** : vers le delay et la réverbération du kit.

Le charleston fermé coupe l'ouvert. Les presets contiennent des kits entiers par style (hip-hop,
trap, drill, house, électro, latino, afro, lo-fi, drum and bass…).

L'onglet **Effets du kit** contient la compression, la couleur, l'égaliseur, le delay et la
réverbération de tout le kit.

### Motif

L'onglet **Motif** est un séquenceur pas à pas sur le clip MIDI de la piste à la tête de
lecture : ce que vous dessinez ici est ce que montre le piano roll. Sans clip, *Nouveau motif* en
crée un à la mesure de la tête de lecture, avec le nombre de mesures choisi à côté.

- **Pas** : un clic pose ou efface un pas, glisser en travers peint, glisser un pas vers le haut
  ou le bas règle sa vélocité, le clic droit donne son roulement, son fla et sa probabilité.
- **Swing** pousse les doubles croches à contretemps vers le triolet.
- **Partir d'un rythme** écrit un motif tout prêt : boom bap, lo-fi, trap, drill, Jersey club,
  house, techno, électro, dembow, phonk, UK garage, afrobeats, amapiano, dancehall, funk, rock,
  drum and bass. Chacun affiche son tempo habituel ; le tempo du projet n'est pas touché.
- **Effacer** vide la variation affichée.

### Variations et enchaînement

Les mesures jouent des **variations** : A, B, C… jusqu'à Z, et un **Fill**. Une variation
s'édite une fois pour toutes les mesures qui la jouent.

- **+ B** met un second motif une mesure sur deux, **+ Fill** un fill à la fin de chaque groupe
  de quatre mesures, avec une montée de caisse claire comme point de départ. Une fois B créé,
  **+ C**, **+ D**… ajoutent une variation sur une nouvelle mesure à la fin, copiée de celle
  affichée.
- **L'enchaînement** montre ce que joue chaque mesure. Un clic sur une mesure passe à la variation
  suivante parmi celles utilisées ; le clic droit donne tous les choix, ou fait de la mesure une
  nouvelle variation qui garde ses notes.
- **Mesures** : le nombre au début de l'enchaînement règle la longueur du motif, de 1 à 64
  mesures. Les nouvelles mesures reprennent l'enchaînement ; celles au-delà de la fin sont
  retirées avec leurs notes.

> **Astuce** Pour varier un groove sans repartir de zéro : construisez A, ajoutez B, puis clic
> droit sur une mesure de l'enchaînement › *Nouvelle : C*, et ne changez que ce qui diffère.
