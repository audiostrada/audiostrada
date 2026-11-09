# Prise en main

AudioStrada enregistre, édite et mixe la musique, de la première prise au master. Ce chapitre
l'installe, règle le son et fait parcourir une première chanson en dix minutes. Chaque chapitre
suivant approfondit une partie du programme.

## Installer

Téléchargez le paquet de votre système sur **audiostrada.com/download** :

| Système | Paquet |
|---|---|
| Linux | `.deb` (Debian, Ubuntu), `.rpm` (Fedora, openSUSE) ou `.AppImage` (toute distribution) |
| Windows | l'installeur `-setup.exe` |
| macOS | l'image disque `.dmg` |

Ouvrez un projet en double-cliquant sur un fichier `.strada`, ou lancez AudioStrada depuis le
menu de votre système. Sans licence, AudioStrada tourne en mode évaluation : tout fonctionne,
export compris (voir *Licence et mises à jour*).

## Régler le son

Sous Linux, AudioStrada joue par PipeWire sans réglage ; JACK et ALSA sont aussi disponibles.
Sous Windows, choisissez WASAPI (partagé ou exclusif) ou ASIO ; sous macOS, CoreAudio.

**Préférences › Audio** ([[Ctrl+,]]) choisit le pilote, le périphérique, la fréquence
d'échantillonnage et la taille du buffer, affiche la latence qui en résulte, joue un son de test
et montre les niveaux d'entrée.

> **Astuce** Si le son craque, augmentez la taille du buffer. Sous Linux, `audiostrada diagnose`
> vérifie le système pour l'audio à faible latence (priorité temps réel, gouverneur du
> processeur, quantum PipeWire) et affiche la correction exacte de chaque problème.

## Une première chanson en dix minutes

1. **Créez un projet** : Fichier › Nouveau projet. Choisissez un dossier : tout ce que la chanson
   utilise y vivra.
2. **Amenez du son** : déposez des fichiers audio sur la timeline, ou Fichier › Importer de
   l'audio ([[Ctrl+I]]). Chaque fichier a sa piste, à son nom.
3. **Enregistrez** : sur une piste vide, choisissez l'entrée dans l'en-tête (*In 1*), armez-la
   avec son bouton **R** et appuyez sur [[R]]. [[Space]] arrête.
4. **Ajoutez une batterie** : Piste › Ajouter une piste boîte à rythmes. Dans l'onglet **Motif**
   de la boîte à rythmes, cliquez sur *Nouveau motif*, puis *Partir d'un rythme*.
5. **Éditez** : glissez les clips pour les déplacer, leurs bords pour les rogner, leurs coins
   hauts pour les fondus. [[S]] scinde à la tête de lecture, [[Ctrl+Z]] annule tout.
6. **Mixez** : dans le **Mixer** sous la timeline, réglez les niveaux aux faders, cliquez sur
   *+ insert* pour ajouter un effet (un compresseur sur la voix, une réverbération sur un send).
7. **Exportez** : Fichier › Exporter ([[Ctrl+Shift+E]]), choisissez WAV ou MP3, et exportez.

AudioStrada enregistre tout seul pendant que vous travaillez, et chaque enregistrement garde la
version précédente : rien n'est jamais perdu.

## Trouver une commande

Chaque commande est dans les menus et dans la **palette de commandes** ([[Ctrl+Shift+P]]), qui
la trouve par son nom et affiche son raccourci. **Aide › Raccourcis clavier** ([[Ctrl+/]]) les
liste tous, et le clic droit montre les options de presque tout : clips, en-têtes de pistes,
règle, tranches du Mixer, inserts, faders et boutons.

> **Remarque** Sous macOS, [[Cmd]] remplace [[Ctrl]] dans tous les raccourcis de ce manuel.
