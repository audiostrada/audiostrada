# Export

![La fenêtre d'export](img/export.webp)

## Exporter le mix

Fichier › Exporter ([[Ctrl+Shift+E]]) ouvre la fenêtre d'export. Les **Réglages tout prêts** la
remplissent en un clic : *Streaming* (WAV 24 bits à -14 LUFS), *CD* (WAV 16 bits, 44,1 kHz, avec
dither, crêtes à -1 dBFS), *Mastering* (WAV 32 bits flottant, sans traitement) et *Stems* (un
fichier par piste). Puis :

| Réglage | Choix |
|---|---|
| **Format** | WAV 16 bits, 24 bits ou 32 bits flottant ; FLAC 16 ou 24 bits ; MP3 ; Opus |
| **Fréquence d'échantillonnage** | celle du projet, ou une autre |
| **Dither** | triangulaire (TPDF), pour descendre à 16 bits |
| **Normaliser** | sur une crête ou une sonie (LUFS) |
| **Étendue** | tout le projet ou la zone de boucle |
| **Traîne** | des secondes ajoutées après la fin, pour les réverbérations et les delays |
| **Stems** | un fichier par piste, en solo, en plus ou à la place du mix |
| **Effets du master** | affiché quand le master a des effets : exporter avec ou sans eux |

La barre du bas résume ce qui sera écrit (format, fréquence, durée, nombre de fichiers) avant
*Exporter…*. Les exports vont plus vite que le temps réel, en arrière-plan. Le métronome et les effets d'écoute
ne sont jamais exportés.

Quand des effets sont actifs sur le master, la fenêtre les nomme : les fichiers passent par eux.
*Exporter sans eux* les laisse de côté pour cet export seulement ; *Mastering* le coche, car un
ingénieur de mastering attend un mix sans limiteur. Un effet pour l'écoute seulement (correction
de casque ou de pièce) va dans les effets d'écoute du master, qui ne sont jamais exportés.

## File de rendu

*Mettre en file…* garde un export, avec le projet tel qu'il est à ce moment, pour le rendre plus
tard avec d'autres. La **File de rendu** les liste ; rendez-les tous d'un coup, quand vous
quittez l'ordinateur.

## Une seule piste

Le menu de l'en-tête de piste exporte une piste seule, après ses effets.

## Geler et bounce

- **Geler** rend une piste avec ses effets et joue le résultat à la place, pour économiser du
  calcul. Dégelez-la pour changer ses effets.
- **Bounce** rend une piste sur une nouvelle piste.

## Depuis un terminal

`audiostrada render chanson.strada --output mix.wav` mixe un projet (ou des fichiers audio) en un
fichier WAV sans ouvrir la fenêtre.
