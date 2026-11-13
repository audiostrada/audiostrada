# Préférences

Affichage › *Préférences…* ([[Ctrl+,]]) liste ses pages à gauche : Apparence et Général ; Audio,
MIDI et Enregistrement ; Raccourcis clavier et Plugins. Chaque page regroupe ses réglages sur des
cartes, le nom et ce qu'il fait à gauche, le réglage à droite. Les changements s'appliquent tout
de suite et sont conservés ; *Terminé* ferme la fenêtre.

*Chercher un réglage*, en haut de la liste, ne garde que les pages qui contiennent ce que vous
tapez, avec les réglages trouvés sous chacune ; un clic (ou [[Enter]] pour la première) ouvre la
page. Tapé là, le nom d'une commande ouvre les raccourcis clavier déjà filtrés.

![Préférences](img/prefs.webp)

## Apparence

- **Thème** : quatre thèmes, en petites images : **Sombre**, **Studio gris**, **Clair** (tranches
  et boutons blancs brillants) et **Personnalisé**. *Personnaliser…* ouvre votre thème à côté d'un
  aperçu en direct : *Partir de* l'un des trois, puis choisissez le fond, l'accent et le
  contraste ; toutes les autres couleurs suivent, et celles qui ne se liraient pas sont corrigées
  (le fond reste nettement sombre ou nettement clair, l'accent ressort et s'écarte du rouge
  d'enregistrement). Les coins, les vue-mètres et les capuchons de fader se règlent aussi.
- **Langue** : anglais ou français ; *Automatique* suit la langue du système.
- **Échelle de l'interface** s'applique avec *Appliquer* (*Annuler* remet le curseur).
- **Master** : la tranche master à gauche ou à droite des pistes dans le mixer.
- **Mixer** : *Mixer compact* rétrécit les tranches ; un clic en ouvre une en pleine largeur.
- **Timeline** : la piste master en haut de la timeline, pour dessiner son automation.
- **Transport** : en haut ou en bas, et en grand pour le lire de loin.

## Général

- **Touche de lecture** : le stop revient là où la lecture a commencé, ou reste où il est.
- **Plusieurs écrans** : remet chaque fenêtre sur son écran (mode X11, voir *Plugins*).
- **Nouvelles versions** : *Vérifier une fois par jour* vous prévient quand une nouvelle version
  officielle sort. Rien ne s'installe tout seul : AudioStrada vous dirige vers la page de
  téléchargement.

## Audio

Le pilote (PipeWire, JACK, ALSA ; WASAPI ou ASIO sous Windows ; CoreAudio sous macOS), les
périphériques de sortie et d'entrée, la fréquence d'échantillonnage, la taille du buffer et les
canaux, avec la latence qui en résulte. Le son de test joue 440 Hz à -18 dBFS ; les indicateurs
d'entrée montrent ce qui arrive. Sous Linux, **Vérification du système** lance les mêmes contrôles
que `audiostrada diagnose` et indique comment corriger chaque problème.

## MIDI

Les entrées MIDI : cochez les claviers et contrôleurs à utiliser. *Actualiser* trouve un appareil
branché après le démarrage.

## Enregistrement

L'écoute de l'entrée des pistes armées (toujours, ou automatique), et la compensation de latence
d'enregistrement.

## Raccourcis clavier

Choisissez un jeu de raccourcis (**AudioStrada**, **Pro Tools**, **Logic**, **Reaper**) et
changez n'importe quel raccourci : cliquez sur *Changer*, appuyez sur les nouvelles touches
([[Escape]] annule) ; *Ajouter* donne un raccourci de plus à une commande, *Effacer* retire ses
touches, *Réinit.* lui rend celles du jeu. Les commandes sont groupées comme dans les menus, leurs
touches dessinées comme des touches. La recherche trouve une commande par son nom.
*Réinitialiser tous les raccourcis* rend à chaque commande les touches du jeu.

## Plugins

Les plugins trouvés, les boutons d'analyse, les dossiers standard et les vôtres (*Ajouter un
dossier…*), le bac à sable (chaque plugin dans son processus), et *Désactiver les plugins tiers*.
