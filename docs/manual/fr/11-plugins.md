# Plugins

![Le navigateur d'effets avec des plugins installés](img/plugins.webp)

## Trouver les plugins

AudioStrada trouve les plugins **CLAP**, **VST3**, **VST2** et **LV2** (et AU sous macOS) dans les
dossiers standard et dans ceux ajoutés dans **Préférences › Plugins**. Les dossiers VST2 sont ceux
de la variable `VST_PATH` et, sous Windows, celui que les installeurs écrivent dans le registre,
`VSTPlugins` et `VST2` dans Program Files ; les fichiers WaveShell de Waves listent chacun de
leurs plugins. Les plugins 32 bits ne tournent pas. *Rechercher de nouveaux plugins*
cherche les nouveaux ; *Tout réanalyser* oublie la liste en cache et analyse à nouveau chaque
plugin. Les plugins Windows ne tournent pas sous Linux.

Les nouveaux plugins sont analysés en arrière-plan au démarrage d'AudioStrada. Pendant le scan,
le navigateur d'effets montre où il en est (Préférences › Plugins aussi) ; les plugins déjà
connus restent utilisables, les nouveaux apparaissent à la fin.

Dans un terminal, `audiostrada plugins` liste les plugins installés, `--rescan` les analyse à
nouveau.

## Le navigateur d'effets

Cliquez sur *+ insert* ou sur un emplacement vide. Le navigateur liste les effets intégrés par
famille (dynamique, égaliseur et filtre, espace, modulation, couleur, stéréo, utilitaires, ampli
et baffle, instrument), puis les plugins installés dans les mêmes familles. La famille d'un
plugin vient de ce qu'il déclare (fonctions CLAP, sous-catégories VST3, catégorie VST2, classe
LV2), sinon d'une liste d'un millier de plugins connus (tirée de
[PluginDB](https://github.com/blakebratcher/plugindb), qui reconnaît aussi leurs versions
suivantes : Pro-Q 4 est rangé comme Pro-Q 3), sinon des mots de son nom (*EQ*, *Verb*, *Comp*…).
Les autres vont dans **Autres**.

- Clic droit sur la carte d'un plugin › *Famille* pour le ranger ailleurs (*Automatique* revient
  à la famille trouvée).
- La recherche lit les noms, les familles et ce qu'elles contiennent, en anglais et en français,
  sans tenir compte des accents : *reverb* trouve Strada-Verb.
- Les boutons de format et le menu **Éditeur** en haut gardent les effets d'un format ou d'un
  éditeur ; cliquer sur l'éditeur d'une carte fait de même.

## Sécurité

Chaque plugin tourne dans son propre processus (Préférences › Plugins › *Lancer chaque plugin
dans son propre processus*). Si un plugin plante, AudioStrada continue de jouer : l'insert devient
rouge et son menu propose *Recharger le plugin*.

Pour démarrer sans aucun plugin tiers, lancez `audiostrada --disable-third-party-plugins`, ou
cochez *Désactiver les plugins tiers* dans Préférences › Plugins (à partir du démarrage suivant).
Les effets intégrés restent, et les inserts de plugins gardent leurs réglages.

## Fenêtres des plugins

Un plugin ouvre sa propre fenêtre quand on l'ajoute et quand on clique sur son insert (à nouveau,
la fenêtre passe au premier plan). Clic droit › *Presets, niveaux et paramètres…* ouvre la fenêtre
d'AudioStrada pour lui : presets, A/B et Bypass en haut comme pour les effets intégrés, niveaux et
Match en bas, et tous les paramètres ; *Interface* ouvre ou ferme la fenêtre du plugin. Un plugin
sans fenêtre affichable par AudioStrada ouvre directement celle d'AudioStrada.

Un plugin qui a une entrée **sidechain**, comme beaucoup de compresseurs et de gates, peut être
piloté par une autre piste, comme le compresseur intégré : choisissez la piste dans la barre du
bas de la fenêtre d'AudioStrada, ou clic droit sur l'insert › *Sidechain*. Cela marche avec les
plugins CLAP et VST3. Un plugin à entrée mono entend les deux côtés d'une piste stéréo, mélangés.

Les fenêtres de plugins suivent la piste sélectionnée ; épinglez-en une pour la garder sur sa
piste. Elles restent au-dessus d'AudioStrada pendant que vous l'utilisez, sous Windows et X11.
Sous Wayland, AudioStrada tourne en mode X11 pour cela (Préférences › Général › *Plusieurs
écrans*) ; décochez-le si une échelle d'affichage fractionnaire (125 %, 150 %) rend les fenêtres
floues, au prix de fenêtres qui peuvent passer derrière.

Dans la fenêtre d'un plugin, les raccourcis du transport ([[Space]], [[R]], [[Home]], [[L]]…) vont
à AudioStrada plutôt qu'au plugin, sous Linux ; les autres touches restent au plugin.
