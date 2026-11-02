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
  n'embarque pas ces bibliothèques non plus : leurs démons doivent correspondre.
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
  téléchargement), lu par la vérification des mises à jour des binaires officiels.
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
- **Linux** : rien à signer pour les paquets eux-mêmes ; `SHA256SUMS` permet de vérifier un
  téléchargement. Un dépôt APT/RPM signé viendra plus tard. Flatpak est écarté pour
  l'instant : l'accès aux plugins installés sur le système y est limité.

## Publier une version

1. Monter `version` dans `Cargo.toml`, ajouter les notes de version, commiter.
2. `git tag v0.2.0 && git push origin v0.2.0` : le CI construit et signe, puis crée un
   brouillon de release avec les paquets, `SHA256SUMS` et `latest.json`.
3. Essayer les paquets du brouillon (au moins un par système), puis publier la release.
   Le site et la vérification des mises à jour suivent la dernière release publiée.
