# La fenêtre

![La fenêtre principale : barre de transport, timeline et Mixer](img/session.webp)

## Barre de transport

De gauche à droite :

- **Transport** : aller au début, lecture / pause ([[Space]] ; la lecture s'arrête d'elle-même après le dernier clip, sauf en
  boucle ou en enregistrement), stop, enregistrement, et
  **Panique**, qui coupe toutes les notes.
- **Boucle**, **punch**, **métronome**, **REF** (écouter une piste de référence) et le
  chronomètre du **mode basse latence** (voir *Enregistrement*).
- **Le compteur** : mesures.temps.ticks, avec le temps d'horloge en dessous.
- **Tempo et signature** : glissez ou tapez le tempo, choisissez la signature rythmique. **TAP**
  règle le tempo d'après vos clics : cliquez en rythme, au moins deux fois ; après deux secondes
  de pause, le comptage repart de zéro. Les clips MIDI gardent leurs mesures et leurs temps quand
  le tempo change, les clips audio leur durée.
- **DSP** montre la charge de calcul et les décrochages ; l'**indicateur** de droite montre le
  niveau du master et sa sonie à court terme.

Les effets résonnent après un stop : les queues de réverbération et de delay s'éteignent
naturellement. Appuyez à nouveau sur stop à l'arrêt pour les couper.

## Barre du haut

Les menus (Fichier, Édition, Clip, Piste, Transport, Affichage, Aide), le périphérique audio
utilisé et, à droite, le **bouton robot** des assistants IA : gris quand ils sont désactivés,
vert quand ils peuvent piloter AudioStrada. Un clic ouvre leur fenêtre (voir *Assistants IA*).

## Panneaux

- **Timeline** : les pistes et leurs clips, la règle avec les marqueurs, la boucle et le punch.
- **Mixer**, **Piano roll** et **Rack** en dessous, en onglets.
- **Navigateur** (fichiers audio), **Inspecteur** (détails du clip ou de la piste sélectionnés)
  et **Marqueurs**, par le menu Affichage ou [[Alt+3]], [[Alt+4]], [[Alt+5]].

Glissez un onglet pour réorganiser les panneaux. Clic droit sur un onglet › *Ouvrir dans sa
propre fenêtre* met un panneau sur un autre écran (le Mixer en général, [[Ctrl+M]]) ; fermer
cette fenêtre le remet en place. Affichage › *Réinitialiser la disposition* revient à
l'agencement par défaut.

AudioStrada rouvre sa fenêtre, et les panneaux dans leurs propres fenêtres, à la taille et dans
l'état où ils ont été laissés. Sous Wayland, c'est le système qui place les fenêtres.

## Onglets de projets

Plusieurs projets peuvent être ouverts à la fois, chacun dans son onglet : Fichier › *Nouvel
onglet de projet* ([[Ctrl+Alt+N]]) ou *Ouvrir un projet dans un nouvel onglet*
([[Ctrl+Alt+O]]). Un seul projet joue à la fois ; les autres attendent en arrière-plan avec
leurs panneaux tels quels. [[Ctrl+Tab]] passe à l'onglet suivant, [[Ctrl+W]] ferme l'onglet
actuel.

## Palette de commandes

[[Ctrl+Shift+P]] ouvre une recherche sur toutes les commandes. Tapez quelques lettres (elles
s'allument dans les noms trouvés), choisissez avec les flèches, puis [[Enter]]. Sans rien taper,
les dernières commandes lancées viennent en tête, puis toutes par catégorie. La palette affiche le
raccourci de chaque commande : c'est le plus rapide pour les apprendre.

## Plein écran

[[F11]] (Affichage › *Plein écran*) remplit l'écran où se trouve la fenêtre, avec les fenêtres
du Mixer et du piano roll quand elles sont détachées.
