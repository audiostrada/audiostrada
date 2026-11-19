# Distribution

Les paquets sont construits par `.github/workflows/release.yml` à chaque tag `v*` (ou à la
main depuis l'onglet Actions) : un brouillon de release GitHub reçoit les fichiers. Tout se
construit aussi en local avec le profil `dist` (optimisé, sans symboles de débogage).

| Système | Fichiers | Commande locale |
|---|---|---|
| Linux | `.deb`, `.rpm`, `.AppImage` | `cargo deb -p audiostrada-app`, `cargo generate-rpm -p crates/audiostrada-app --profile dist`, `sh packaging/linux/build-appimage.sh` |
| Windows | `AudioStrada-<version>-x64-setup.exe` | `cargo build -p audiostrada-app --profile dist`, puis `iscc /DVersion=<version> packaging\windows\audiostrada.iss` |
| macOS | `AudioStrada.app` universel dans `AudioStrada-<version>.dmg` | `sh packaging/macos/bundle.sh` |

Outils : `cargo install cargo-deb cargo-generate-rpm`, [appimagetool](https://github.com/AppImage/appimagetool/releases)
sur le `PATH`, [Inno Setup 6](https://jrsoftware.org/isinfo.php) sous Windows, Xcode sous macOS.

## Ce que les paquets installent

- **Linux** : `/usr/bin/audiostrada`, l'entrée de menu (`audiostrada.desktop`), le type MIME
  `application/x-audiostrada-project` pour les fichiers `.strada`, les métadonnées AppStream et les
  icônes de 16 à 512 px. PipeWire, ALSA et JACK viennent du système (le `.deb` dépend de
  `libpipewire`, recommande `pipewire` et `liblilv-0-0` pour les plugins LV2). L'AppImage
  n'embarque pas ces bibliothèques non plus : leurs démons doivent correspondre. Les paquets
  sont construits sur Ubuntu 24.04 et demandent une distribution aussi récente (glibc 2.39 :
  Ubuntu 24.04, Debian 13, Fedora 40 ou plus). Ailleurs, compiler depuis les sources.
- **Windows** : installation dans `Program Files\AudioStrada` (ou par utilisateur), menu Démarrer,
  icône de bureau en option, association des fichiers `.strada` en option, désinstalleur.
  L'icône est intégrée à l'exécutable ; lancé depuis le menu, AudioStrada referme la console que
  Windows ouvre pour les programmes en ligne de commande.
- **macOS** : application universelle (Apple Silicon et Intel), fichiers `.strada` associés,
  autorisation du micro demandée au premier enregistrement.

## Binaires officiels et compilations maison

Le CI de release compile avec la feature `official` : sans licence, ces binaires tournent en
mode évaluation (coupures de 1,5 s toutes les 10 min après 20 min). Une compilation maison,
`cargo build` sans cette feature, n'a aucune restriction. Voir `crates/audiostrada-license`.

- `AUDIOSTRADA_LICENSE_KEY` (variable du dépôt, pas un secret) : clé publique des licences, la
  ligne `public` de `audiostrada license keygen`. La ligne `secret` va dans le serveur de licences,
  jamais ici.
- `AUDIOSTRADA_RELEASE_DATE` : date du commit du tag, posée par le CI. Une licence couvre les
  versions dont la date est antérieure à la fin de ses mises à jour.
- Le job `release` ajoute `SHA256SUMS` et `latest.json` (version, date, page de
  téléchargement), lu par la vérification des mises à jour des binaires officiels. La fenêtre de
  mise à jour télécharge l'installeur par `/get/<type>`, le vérifie avec `SHA256SUMS` et, si
  l'exemplaire en cours est signé, exige la même signature (équipe Apple, sujet du certificat
  Windows) avant de le lancer.
- Le site est `SITE` dans `crates/audiostrada-license/src/lib.rs` et `SITE_URL` dans le workflow.

## Signature

Chaque étape de signature ne tourne que si ses secrets existent : sans eux, le CI produit des
binaires non signés.

- **macOS** : sans signature, Gatekeeper bloque le premier lancement (clic droit › Ouvrir).
  Il faut un compte Apple Developer (99 €/an) et ces secrets du dépôt :
  `MACOS_CERTIFICATE` (certificat « Developer ID Application » exporté en `.p12`, en base64),
  `MACOS_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` (mot de passe
  d'application). La variable `AUDIOSTRADA_BUNDLE_ID` fixe l'identifiant (sinon
  `com.audiostrada.AudioStrada`). Les droits du runtime durci (`packaging/macos/entitlements.plist`)
  autorisent le micro et le chargement de plugins signés par leurs éditeurs.
- **Windows** : Azure Artifact Signing signe `audiostrada.exe` puis l'installeur. Il faut un compte
  Artifact Signing validé (organisation), une App Registration ayant le rôle « Artifact
  Signing Certificate Profile Signer », et :
  - secrets : `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` ;
  - variables : `AZURE_SIGNING_ENDPOINT` (celui de la région du compte, par exemple
    `https://weu.codesigning.azure.net/`), `AZURE_SIGNING_ACCOUNT`, `AZURE_CERTIFICATE_PROFILE`.

  Même signé, SmartScreen peut avertir les premières semaines, le temps que la réputation se
  construise.
- **Linux** : la clé GPG des paquets (secret `PACKAGES_GPG_KEY`, la clé privée en armure,
  sans phrase de passe ; sa clé publique est `packaging/linux/audiostrada-packages.asc`) signe
  les `.rpm` et les index des dépôts. Les variantes `official` du `.deb` et du `.rpm` ajoutent
  le dépôt et cette clé publique au système.

## Dépôts APT et RPM

Quand une release est publiée (jamais un brouillon), `.github/workflows/packages.yml` reprend
les `.deb` et `.rpm` des trois dernières, construit les dépôts (`packaging/linux/build-repos.sh`),
les signe et les envoie dans le bucket R2 `audiostrada-packages`, servi sur
`packages.audiostrada.com`, avec une page d'accueil (`packaging/linux/repo-index.html` : ajout
du dépôt à la main, clé, liste des paquets). Une règle Cloudflare de réécriture d'URL (zone
audiostrada.com) sert `/index.html` pour `/`. Il faut les secrets `R2_ACCESS_KEY_ID` et `R2_SECRET_ACCESS_KEY` (un
jeton d'API R2 en lecture et écriture sur ce bucket) et la variable `CLOUDFLARE_ACCOUNT_ID`.

## Flathub et Snap Store

Les magasins reçoivent eux aussi les binaires officiels (feature `official`, mode évaluation
sans licence). Ils mettent AudioStrada à jour eux-mêmes : sous Flatpak (`FLATPAK_ID`) ou Snap
(`SNAP`), la vérification des mises à jour n'est pas proposée (`updates::offered`).
`.github/workflows/stores.yml` construit les deux sur une pull request qui touche
`packaging/`, et à chaque release publiée.

- **Flathub** (`packaging/flatpak`) : `manifest.sh` écrit dans `target/flatpak/` le manifeste,
  ses sources Cargo (flatpak-cargo-generator) et `flathub.json`. Avec un tag (`manifest.sh
  v1.0.0`), ce sont les fichiers du dépôt Flathub : le workflow les publie en artefact
  `flathub` à chaque release. La recette compile depuis les sources avec la Rust de
  `rust-toolchain.toml` (l'extension Rust du SDK est en retard) et lilv pour LV2.
  - Plugins : ceux du dossier personnel (`~/.clap`, `~/.vst3`, `~/.lv2`) et ceux des extensions
    `org.freedesktop.LinuxAudio.Plugins.*`, montées dans `/app/extensions/Plugins`. Ceux de la
    distribution (`/usr/lib/vst3`...) restent invisibles.
  - Première soumission : une pull request sur `flathub/flathub` (branche `new-pr`) avec les
    fichiers de l'artefact. Ensuite, chaque version : une pull request sur
    `flathub/com.audiostrada.AudioStrada` avec les nouveaux fichiers. La fiche « vérifiée »
    demande un jeton dans `https://audiostrada.com/.well-known/org.flathub.VerifiedApps.txt`.
- **Snap Store** (`packaging/snap`) : `build-snap.sh` emballe les binaires dans un snap
  `core24` en confinement *classic*, comme le `.deb` : plugins partout sur le système, liés à
  ses bibliothèques (glibc 2.39 ou plus). Le confinement classic demande l'accord de Canonical
  (forum.snapcraft.io, catégorie « store-requests »). Le workflow publie sur le canal `stable`
  quand le secret `SNAPCRAFT_STORE_CREDENTIALS` existe :
  `snapcraft export-login --snaps=audiostrada --acls package_access,package_push,package_update,package_release -`.
- **Métadonnées** : `packaging/linux/com.audiostrada.AudioStrada.metainfo.xml` (captures dans
  `packaging/linux/screenshots`, servies depuis `main`) reçoit une ligne `<release>` par version.

## Publier une version

0. Mettre à jour la table des familles de plugins : `scripts/update-plugindb.py` télécharge la
   dernière base de [PluginDB](https://github.com/blakebratcher/plugindb) et dit ce qui change.
   Relire, puis commiter `crates/audiostrada-ui/assets/plugindb.tsv`. Les corrections d'un
   plugin vont dans `plugin_families_extra.tsv`, que le script ne touche pas.
1. Monter `version` dans `Cargo.toml`, écrire les notes de version (une section `## <version>`
   en tête de `crates/audiostrada-ui/assets/whats-new/en.md` et `fr.md` : la fenêtre Quoi de neuf
   les montre après la mise à jour, la release GitHub reprend l'anglais), ajouter une ligne
   `<release>` dans `packaging/linux/com.audiostrada.AudioStrada.metainfo.xml`, commiter.
2. `git tag v0.2.0 && git push origin v0.2.0` : le CI construit et signe, puis crée un
   brouillon de release avec les paquets, `SHA256SUMS` et `latest.json`.
3. Essayer les paquets du brouillon (au moins un par système), puis publier la release.
   Le site et la vérification des mises à jour suivent la dernière release publiée.
4. Flathub : ouvrir la pull request de la version avec l'artefact `flathub` du workflow Stores.
