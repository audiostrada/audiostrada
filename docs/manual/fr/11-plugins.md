# Plugins

![Le navigateur d'effets avec des plugins installés](img/plugins.webp)

## Trouver les plugins

AudioStrada trouve les plugins **CLAP**, **VST3** et **LV2** (et AU sous macOS) dans les dossiers
standard et dans ceux ajoutés dans **Préférences › Plugins**. *Rechercher de nouveaux plugins*
cherche les nouveaux ; *Tout réanalyser* oublie la liste en cache et analyse à nouveau chaque
plugin. Les plugins Windows ne tournent pas sous Linux.

Dans un terminal, `audiostrada plugins` liste les plugins installés, `--rescan` les analyse à
nouveau.

## Le navigateur d'effets

Cliquez sur *+ insert* ou sur un emplacement vide. Le navigateur liste les effets intégrés par
famille (dynamique, égaliseur et filtre, espace, modulation, couleur, stéréo, utilitaires, ampli
et baffle, instrument), puis les plugins installés dans les mêmes familles, d'après ce que chacun
déclare (fonctions CLAP, sous-catégories VST3, classe LV2). Ceux qui ne déclarent rien de précis
vont dans **Autres**, et **Plugins** les liste tous.

- Clic droit sur la carte d'un plugin › *Famille* pour le ranger ailleurs (*Automatique* revient à
  ce qu'il déclare).
- Les menus **Éditeur** et **Format** à côté de la recherche gardent les effets d'un éditeur ou
  d'un format ; cliquer sur l'éditeur d'une carte fait de même.

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
d'AudioStrada pour lui : presets, A/B, niveaux, Match, et tous les paramètres. Un plugin sans
fenêtre affichable par AudioStrada ouvre directement celle d'AudioStrada.

Les fenêtres de plugins suivent la piste sélectionnée ; épinglez-en une pour la garder sur sa
piste. Elles restent au-dessus d'AudioStrada pendant que vous l'utilisez, sous Windows et X11.
Sous Wayland, AudioStrada tourne en mode X11 pour cela (Préférences › Général › *Plusieurs
écrans*) ; décochez-le si une échelle d'affichage fractionnaire (125 %, 150 %) rend les fenêtres
floues, au prix de fenêtres qui peuvent passer derrière.

Dans la fenêtre d'un plugin, les raccourcis du transport ([[Space]], [[R]], [[Home]], [[L]]…) vont
à AudioStrada plutôt qu'au plugin, sous Linux ; les autres touches restent au plugin.
