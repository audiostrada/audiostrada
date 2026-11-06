# Dépannage

## Problèmes courants

| Problème | Que faire |
|---|---|
| **Craquements ou décrochages** | Augmentez la taille du buffer dans Préférences › Audio. Sous Linux, lancez `audiostrada diagnose`. Gelez les pistes lourdes. Surveillez l'indicateur **DSP** de la barre de transport. |
| **Pas de son** | Vérifiez le périphérique dans Préférences › Audio, que le master n'est pas muet, et qu'aucune piste n'est en solo. Le son de test vérifie la sortie. |
| **Pas d'entrée** | Vérifiez l'entrée de la piste (*In 1*), qu'elle est armée, et son bouton d'écoute. |
| **L'enregistrement est en retard** | Réglez la *Compensation de latence d'enregistrement* dans Préférences › Général ; `audiostrada latency` la mesure avec un câble de la sortie 1 à l'entrée 1. |
| **De courtes coupures toutes les dix minutes** | C'est le mode évaluation : saisissez une licence (Aide › *Licence…*). |
| **Un plugin se comporte mal** | Clic droit sur son insert › *Recharger le plugin*, ou retirez-le. Les plugins ne peuvent pas faire planter AudioStrada. |
| **AudioStrada plante au démarrage à cause d'un plugin** | Lancez-le avec `audiostrada --disable-third-party-plugins`, puis retirez le plugin. |
| **Fenêtres floues sous Wayland** | Décochez *Plusieurs écrans* dans Préférences › Général. |
| **Un assistant n'arrive pas à se connecter** | Vérifiez que le bouton robot est vert, puis redémarrez l'assistant. |

## Ligne de commande

| Commande | Effet |
|---|---|
| `audiostrada diagnose` | vérifie le système pour l'audio à faible latence et affiche les corrections |
| `audiostrada devices` | liste les périphériques audio |
| `audiostrada tone` | joue un son de test sur chaque sortie |
| `audiostrada monitor` | affiche les niveaux d'entrée |
| `audiostrada latency` | mesure la latence aller-retour |
| `audiostrada plugins [--rescan]` | liste les plugins installés |
| `audiostrada play FICHIER…` | joue des fichiers audio ou un projet |
| `audiostrada render FICHIER… --output SORTIE.wav` | mixe des fichiers audio ou un projet en un fichier WAV |
| `audiostrada compare A B` | compare deux fichiers audio échantillon par échantillon (test d'annulation) |

## Signaler un problème

Lancez AudioStrada depuis un terminal avec `AUDIOSTRADA_LOG=debug audiostrada` et joignez la
sortie quand vous écrivez à **contact@audiostrada.com**, avec votre système, votre interface audio
et ce que vous faisiez juste avant.

## Questions

**Puis-je ouvrir mes projets d'un autre logiciel ?** AudioStrada importe les fichiers audio et
MIDI : exportez vos pistes en stems depuis l'autre logiciel, puis déposez-les sur la timeline.

**Où sont mes enregistrements ?** Dans le dossier `Audio` du projet.

**Puis-je utiliser mes plugins ?** Oui : CLAP, VST3 et LV2 (et AU sous macOS), chacun dans son
propre processus.

**AudioStrada a-t-il besoin d'internet ?** Non. Il ne se connecte que pour vérifier les nouvelles
versions si vous l'avez permis, et pour TONE3000 quand vous le parcourez.

**Puis-je revenir à une version précédente de ma chanson ?** Oui : Fichier › *Versions du
projet…*.

**Mon travail est-il sauvé si AudioStrada plante ?** Oui : il enregistre tout seul, et propose de
récupérer le travail au démarrage suivant.
