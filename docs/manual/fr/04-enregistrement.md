# Enregistrement

## Enregistrer une prise

1. Choisissez l'entrée de la piste dans son en-tête : *In 1*, ou *In 1-2* pour une source
   stéréo. Piste › *Ajouter une piste stéréo* ([[Ctrl+Alt+T]]) crée une piste qui enregistre deux
   entrées.
2. Armez la piste avec son bouton **R** : son entrée s'entend à travers la piste et son
   indicateur montre le niveau d'entrée.
3. Appuyez sur enregistrement ([[R]]). Plusieurs pistes armées enregistrent ensemble.
4. [[Space]] arrête. La prise devient un nouveau clip ; son fichier va dans le dossier `Audio`
   du projet.

## Écoute de contrôle

Le bouton haut-parleur à côté de **R** dit quand une piste entend son entrée :

| Réglage | La piste entend son entrée |
|---|---|
| **Auto** | quand elle est armée, selon les Préférences (toujours, ou seulement à l'arrêt et en enregistrement) |
| **Entrée** | toujours, armée ou non : pour jouer à travers un simulateur d'ampli |
| **Désactivé** | jamais : pour l'écoute directe sur l'interface audio |

Le bouton est allumé quand l'entrée s'entend. L'indicateur d'une piste armée montre son niveau
d'entrée, entendu ou non.

## Brut ou traité

La ligne **Enregistrer** de l'Inspecteur dit ce qu'enregistre une piste audio armée :

- **Brut** : l'entrée telle qu'elle arrive ; ses effets peuvent encore changer ensuite.
- **Traité** : ce que ses effets font de l'entrée, sur une piste créée sous elle, *(imprimé)*.
- **Les deux** : la prise brute sur la piste, et la prise traitée sur une piste muette sous elle.

Une prise traitée demande que l'entrée passe par la piste : son écoute ne doit pas être
**Désactivé**.

## État de la séance

Quand une piste est armée, la barre de transport montre :

- **Écoute** : le temps que met ce que vous jouez à arriver à vos oreilles, à travers l'interface
  audio et les effets les plus lents entendus en direct. Réglez la compensation de latence
  d'enregistrement dans les Préférences pour un chiffre exact.
- Les coupures audio depuis qu'une piste est armée.
- Le temps d'enregistrement qui reste sur le disque.

Quand l'écoute est lente, **Réduire** choisit un tampon audio de 64 échantillons et active le mode
basse latence.

## Mode basse latence

Le chronomètre de la barre de transport met en bypass les effets qui ajoutent plus de 1 ms, sauf
sur les pistes jouées, pour que leur latence ne retarde plus ce que vous entendez. Le Mixer barre
les effets mis en bypass. Désactivez-le pour mixer.

## Métronome

Le bouton du métronome ([[K]]) clique sur chaque temps pendant la lecture. Clic droit dessus, ou
[[Shift+K]], pour sa fenêtre : volume, son, clics par temps, accents, groupes pour les mesures
asymétriques (7/8 en 2+2+3), clic seulement en enregistrement, et un décompte jusqu'à quatre
mesures. Le clic n'est jamais exporté.

## Punch et enregistrement en boucle

- **Punch** ([[P]]) : [[Shift]]-glisser dans la règle définit la zone de punch, ou clic droit
  sur la règle. L'enregistrement n'a lieu qu'à l'intérieur, pour remplacer une erreur sans toucher
  au reste.
- **Enregistrement en boucle** : avec la boucle active ([[L]]), chaque passage devient une prise.

## Prises et comping

Chaque prise audio est gardée entière dans sa propre lane sous la piste, nommée d'après son
numéro et le marqueur où elle commence (*Refrain · Prise 2*). Ce que jouait la piste avant sa
première prise a une lane *Avant l'enregistrement*. La piste joue le comping, d'abord la dernière
prise ; les lanes s'affichent dès qu'il y a un choix.

- Glissez sur une lane pour prendre ce passage, ou cliquez sur une prise pour la prendre en
  entier. **Choisir** prend toute la prise. De courts fondus adoucissent chaque raccord.
- Le ✕ supprime une lane ; ce que la piste en joue reste.
- Cliquez sur les étoiles pour noter une prise. Un **!** signale que l'entrée a saturé ou que
  l'audio a coupé pendant l'enregistrement ; les coupures sont marquées en rouge sur le clip.

L'enregistrement MIDI en boucle fusionne ses passages en un seul clip.

## Enregistrer du MIDI

Activez votre clavier MIDI dans Préférences › MIDI. Armez une piste instrument et jouez :
l'enregistrement crée un clip MIDI. Une piste instrument armée pour le MIDI n'entend jamais les
entrées audio.
