# Projets

## Ce que contient un projet

Un projet est un dossier qui contient :

- le fichier `.strada`, qui décrit la chanson ;
- un dossier `Audio` : les enregistrements et les fichiers importés y sont copiés ;
- un dossier `Models` : captures d'amplis et réponses impulsionnelles ;
- un dossier `Versions` : les enregistrements précédents ;
- un cache des images de formes d'onde, refait s'il manque.

Fichier › Nouveau projet, Ouvrir, Enregistrer ([[Ctrl+S]]) et *Enregistrer sous*. AudioStrada
enregistre aussi tout seul ; après un plantage, il propose de récupérer le travail non
enregistré au démarrage suivant.

## Annulation et versions

Tout s'annule ([[Ctrl+Z]]) et se rétablit ([[Ctrl+Shift+Z]] ou [[Ctrl+Y]]), autant d'étapes
que la session en contient.

Chaque enregistrement garde le fichier qu'il remplace dans le dossier `Versions` du projet (les
30 derniers). Fichier › *Versions du projet…* les liste et en restaure un en une seule étape,
que [[Ctrl+Z]] annule.

## Importer de l'audio

Fichier › Importer de l'audio ([[Ctrl+I]]) ou déposez des fichiers sur la timeline. Chaque
fichier a sa piste, à son nom : le plus rapide pour amener une série de stems. Un fichier stéréo
donne une piste stéréo.

AudioStrada lit le WAV, l'AIFF, le CAF, le FLAC, l'ALAC, le MP3, le MP2, l'AAC (`.m4a`,
`.aac`), l'Ogg Vorbis, et l'audio des fichiers MP4, MOV, MKV et WebM.

Le **Navigateur** ([[Alt+3]]) liste les fichiers audio de vos dossiers, avec un filtre. Avec
*Lire à la sélection* coché, sélectionner un fichier le joue. Glissez un fichier sur la
timeline, ou double-cliquez dessus pour l'ajouter sur une nouvelle piste.

![Le Navigateur](img/browser.webp)

## Tout garder ensemble

- Fichier › *Rassembler les médias dans le dossier du projet* copie dans le projet les fichiers
  qu'il utilise ailleurs (audio, captures d'amplis, réponses impulsionnelles) : le dossier
  contient alors tout.
- Fichier › *Archiver le projet…* emballe le projet et tous ses fichiers dans un seul fichier
  `.tar.gz`, à envoyer ou garder, sans changer le projet.

## Fichiers MIDI

Fichier › *Importer un fichier MIDI* ([[Ctrl+Alt+I]]) crée une piste par partie. Fichier ›
*Exporter un fichier MIDI* écrit la piste sélectionnée, ou toutes.
