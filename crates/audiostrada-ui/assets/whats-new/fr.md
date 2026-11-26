## 0.3.0

- Instruments à clavier : pianos électriques Strada-Tines, Strada-Reed et Strada-EP80, orgue Strada-Tonewheel avec l'effet Strada-Rotary, Strada-FM Keys, Strada-Mallets, basse Strada-808 et Strada-Grain.
- Autres logiciels : import et export DAWproject, import des projets Reaper. Les plugins gardent leur nom, même s'ils ne sont pas installés.
- Les fichiers MIDI s'importent sur la piste d'instrument sélectionnée, ou se déposent sur la timeline.
- Aide › Rechercher des mises à jour.
- Effets revus sur des références mesurées : caractères du compresseur, égaliseurs de la tranche, Tube EQ, Tape, Drive, Chorus, Phaser, Flanger, Rotary, Verb, MB-Comp et Max.
- La lecture garde l'audio décodé en mémoire : plus de coupures quand le système manque de mémoire, et un avertissement quand un projet dépasse le budget.
- Express Mix reconnaît les pistes de clavier et garde au centre une paire stéréo nommée L/R.
- Les fenêtres de fichiers s'ouvrent là où ce type de fichier a été choisi la dernière fois.
- Les plugins installés pendant qu'AudioStrada tourne sont trouvés sans le redémarrer.
- Les dialogues s'ouvrent dans leur propre fenêtre ; les éditeurs, le navigateur et les fenêtres de plugins restent au-dessus des panneaux détachés, le dernier cliqué devant.
- Une fenêtre qui couvre un menu ouvert s'écarte jusqu'à sa fermeture ; un panneau détaché réduit revient dans la fenêtre principale.
- macOS : les fenêtres flottantes se cachent avec la fenêtre principale et s'affichent en plein écran ; cliquer sur l'emplacement d'un plugin ramène sa fenêtre devant.
- Linux : AudioStrada ne revient plus devant une autre application.
- Windows : l'installeur affiche le logo d'AudioStrada ; le bac à sable des plugins ne se bloque plus quand un plugin affiche son propre dialogue.

## 0.2.1

- L'AppImage s'installe d'elle-même : à son premier lancement, AudioStrada propose de la ranger dans vos programmes et le menu des applications, sans mot de passe administrateur, et d'ouvrir les fichiers `.strada` avec elle. Préférences › Général l'installe ou la désinstalle ensuite.
- L'AppImage se met maintenant à jour comme les versions Windows et macOS : *Télécharger et installer* la remplace par la nouvelle version et AudioStrada redémarre.
- Un mois avant la fin des mises à jour de votre licence, AudioStrada vous prévient et, si votre système installe les nouvelles versions de lui-même (dépôt de paquets, Flathub, Snap Store), donne la commande qui garde la version installée.
- macOS : les fenêtres flottantes et celles des plugins se cachent maintenant quand la fenêtre principale est réduite, et reviennent avec elle.
- Windows : l'installeur demande maintenant Windows 10 ou plus récent.

## 0.2.0

- Les mises à jour s'installent depuis AudioStrada : cliquez sur *Version … disponible*, puis sur *Télécharger et installer*. Sous Windows et macOS, AudioStrada télécharge l'installeur, le vérifie et le lance.
- Sous Linux, les paquets .deb et .rpm ajoutent le dépôt de paquets d'AudioStrada : les nouvelles versions arrivent ensuite avec les mises à jour du système (`sudo apt upgrade`, `sudo dnf upgrade`).
- Cette fenêtre, Quoi de neuf, montre ce qui a changé après chaque mise à jour. Elle se rouvre depuis le menu Aide.
- Aide › Manuel ouvre le manuel sur le site.
- Saisir une licence ferme maintenant la fenêtre Licence et la confirme, avec sa formule, son titulaire et les mises à jour incluses.
- Sans licence, les versions de test passent maintenant en mode évaluation, comme la version finale : après 20 minutes, le son se coupe un instant toutes les 10 minutes. Une licence supprime ces coupures.
- L'AppImage affiche maintenant le nom, la description et l'icône d'AudioStrada dans les logithèques.

## 0.1.0

La première préversion, pour les testeurs : enregistrement, édition, mixage, automation, les effets et instruments intégrés, et les plugins CLAP, VST3 et LV2.
