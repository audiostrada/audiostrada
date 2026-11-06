# Préférences

Affichage › *Préférences…* ([[Ctrl+,]]) a cinq onglets : Général, Audio, Clavier, Plugins et
MIDI.

![Préférences](img/prefs.webp)

## Général

- **Apparence** : les thèmes, en petites images : Modern, Midnight, Console (angles vifs,
  indicateurs éclairés, capuchons de fader striés), Neon, Vintage, Studio gray, Light et Paper.
  Chacun a ses couleurs et son allure. *Couleur d'accent personnalisée* remplace l'accent du thème
  par le vôtre.
- **Langue** : anglais ou français ; *Automatique* suit la langue du système.
- **Échelle de l'interface** s'applique avec *Appliquer* (*Annuler* remet le curseur).
- **Espace de travail** : la touche de lecture (le stop revient là où la lecture a commencé, ou
  reste où il est), l'écoute de l'entrée des pistes armées, la compensation de latence
  d'enregistrement.
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

## Clavier

Choisissez un jeu de raccourcis (**AudioStrada**, **Pro Tools**, **Logic**, **Reaper**) et
changez n'importe quel raccourci : cliquez sur *Changer*, appuyez sur les nouvelles touches
([[Escape]] annule). La recherche trouve une commande par son nom.

## Plugins

Les plugins trouvés, les boutons d'analyse, les dossiers standard et les vôtres (*Ajouter un
dossier…*), le bac à sable (chaque plugin dans son processus), et *Désactiver les plugins tiers*.

## MIDI

Les entrées MIDI : cochez les claviers et contrôleurs à utiliser. *Actualiser* trouve un appareil
branché après le démarrage.
