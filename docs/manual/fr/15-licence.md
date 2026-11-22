# Licence et mises à jour

AudioStrada est un logiciel libre sous licence GNU GPL : son code source est ouvert, et qui le
compile lui-même l'utilise sans limite. Les versions officielles (installeurs signés et leurs mises
à jour) viennent avec un abonnement annuel, Personnel ou Commercial, qui finance le développement.

## Évaluation

Sans licence, les versions officielles tournent en mode évaluation : tout fonctionne, export
compris. Après 20 minutes, le son se coupe un instant toutes les 10 minutes. Une licence supprime
ces coupures.

## Saisir une licence

Aide › *Licence…* montre l'état de cet exemplaire : évaluation, ou la licence avec sa formule,
son titulaire et jusqu'à quand les mises à jour sont incluses.

- *Saisir une licence* : collez la licence, ou *Ouvrir un fichier…*, puis *Activer*. Déposer le
  fichier de licence sur la fenêtre marche aussi. La fenêtre se ferme alors et *Licence activée*
  montre la formule, le titulaire et les mises à jour incluses.
- *Acheter une licence* ouvre la boutique sur audiostrada.com.
- *Retrouver ma licence* et *Gérer mon abonnement* ouvrent votre compte sur audiostrada.com, pour
  récupérer la licence, changer l'abonnement ou le résilier.
- *Retirer la licence*, en bas à gauche, l'enlève de cet ordinateur après confirmation.

| Licence | Pour |
|---|---|
| **Personnelle** | les musiciens qui gagnent moins de 20 000 € par an grâce à la musique ou à l'audio, même s'ils vendent leurs morceaux ou jouent des concerts payés |
| **Commerciale** | les studios, les entreprises et les professionnels, pour un travail commercial de toute taille |

C'est le même logiciel. Les versions reçues pendant l'abonnement restent à vous : à la fin de
l'abonnement, la fenêtre indique jusqu'à quand les mises à jour étaient comprises, et AudioStrada
continue de fonctionner.

## Mises à jour

Avec *Vérifier une fois par jour* coché (Préférences › Général), AudioStrada vous prévient quand
une nouvelle version sort : cliquez sur *Version … disponible* en haut à droite. Sous Windows et
macOS, *Télécharger et installer* télécharge l'installeur et le vérifie (les sommes de contrôle
de la version, et la signature de l'éditeur d'AudioStrada) ; sous Windows, AudioStrada se ferme
ensuite en proposant d'enregistrer vos projets, et l'installeur démarre ; sous macOS, l'image
disque s'ouvre : glissez AudioStrada sur Applications en remplaçant l'ancien. Projets,
préférences et presets restent.

Aide › *Rechercher des mises à jour* vérifie tout de suite, quelle que soit la préférence, et
vous dit quand votre version est la dernière.

Après une mise à jour, Aide › *Quoi de neuf* s'ouvre tout seul avec les notes de la nouvelle
version ; il se rouvre à tout moment.

Sous Linux, les paquets `.deb` et `.rpm` ajoutent aussi le dépôt de paquets d'AudioStrada : les
nouvelles versions arrivent ensuite avec les mises à jour du système (`sudo apt upgrade`,
`sudo dnf upgrade` ou le gestionnaire de mises à jour), et la fenêtre de mise à jour donne la
commande. Sinon, elle télécharge le bon paquet. L'AppImage se met à jour comme les versions
Windows et macOS : *Télécharger et installer* vérifie la nouvelle AppImage, la met à la place de
celle qui est installée (en l'installant si elle ne l'était pas), et AudioStrada redémarre.

### Garder une version

Les dépôts de paquets, Flathub et le Snap Store installent les nouvelles versions tout seuls, même
celles sorties après la fin de vos mises à jour, qui tournent en mode évaluation. Un mois avant la
fin, AudioStrada vous prévient et donne la commande qui garde la version installée ; après une
telle mise à jour, il donne celle qui revient à la dernière version couverte par votre licence.

| Installé avec | Garder cette version | Réautoriser les mises à jour |
|---|---|---|
| `.deb` | `sudo apt-mark hold audiostrada` | `sudo apt-mark unhold audiostrada` |
| `.rpm` | `sudo dnf versionlock add audiostrada` | `sudo dnf versionlock delete audiostrada` |
| Snap Store | `sudo snap refresh --hold audiostrada` | `sudo snap refresh --unhold audiostrada` |
| Flathub | `flatpak mask com.audiostrada.AudioStrada` | `flatpak mask --remove com.audiostrada.AudioStrada` |

Pour revenir à la version 1.2.0, par exemple : `sudo apt install --allow-downgrades
audiostrada=1.2.0-1` ou `sudo dnf downgrade audiostrada-1.2.0` ; `sudo snap revert audiostrada`
revient à la version précédente. Sur Flathub, `flatpak remote-info --log flathub
com.audiostrada.AudioStrada` liste les versions précédentes, et `flatpak update
--commit=<commit> com.audiostrada.AudioStrada` en installe une. Sur les anciennes versions de
Fedora, `versionlock` vient avec le paquet `python3-dnf-plugin-versionlock`.
