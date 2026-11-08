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

Un synthétiseur polyphonique : deux oscillateurs avec unisson, un sous-oscillateur et du bruit,
une dérive analogique, un filtre, un glide et le MPE. Sa fenêtre tient sans défilement :
Tonalité, Brillance et Jeu côte à côte, Forme en dessous (tirez les coins de l'enveloppe), puis
le clavier.

- **Les presets** contiennent des sons prêts : claviers doux, nappes, cordes, cuivres, pluck,
  cloche, lead, basses.
- **Le clavier** joue le synthé sans armer la piste : cliquez ou glissez sur les touches (plus bas
  sur une touche joue plus fort), ou activez *Clavier de l'ordinateur* pour jouer avec les
  lettres : la rangée du milieu pour les touches blanches, celle du dessus pour les noires, deux
  touches de la rangée du bas pour l'octave. Les touches comptent par leur position : AZERTY et
  QWERTY jouent pareil.
- **Contrôleur MPE** est pour les contrôleurs MPE (Roli Seaboard, LinnStrument, Osmose), qui
  envoient chaque note sur son propre canal.

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
  **Départs** : vers le delay et la réverbération du kit.

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
