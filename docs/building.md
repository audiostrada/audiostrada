# Compiler AudioStrada sous Windows et macOS

Points communs :
- La version de Rust est imposée par `rust-toolchain.toml` ; `rustup` l'installe tout seul au premier `cargo`.
- Aucun SDK audio n'est nécessaire : l'ASIO passe par COM avec les pilotes installés, et CoreAudio est fourni par le système.
- LV2 est facultatif : `lilv` est chargé au lancement, pas au build.

## Windows (x64)

### Prérequis

1. **Visual Studio Build Tools 2022** avec la charge de travail « Développement Desktop en C++ »
   (MSVC et Windows SDK). Le SDK fournit `rc.exe`, qui intègre l'icône dans l'exe ; sans lui,
   le build passe mais affiche un avertissement.
2. **rustup** (https://rustup.rs) avec la toolchain par défaut `x86_64-pc-windows-msvc`.
3. **Git**.

### Build et lancement

```powershell
git clone <repo> AudioStrada
cd AudioStrada
cargo build --release -p audiostrada-app --locked
.\target\release\audiostrada.exe
# ou directement :
cargo run --release -p audiostrada-app
```

### Installeur (`AudioStrada-<version>-x64-setup.exe`)

1. Installer **Inno Setup 6** (https://jrsoftware.org/isinfo.php ou `choco install innosetup`).
2. Lancer :

```powershell
cargo build -p audiostrada-app --profile dist --locked
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" "/DVersion=$version" packaging\windows\audiostrada.iss
```

L'installeur est écrit dans `target\installer\`. Comme il n'est pas signé, SmartScreen affiche
un avertissement au premier lancement.

### LV2 (facultatif)

Placer `lilv-0.dll` dans le `PATH` ou à côté de `audiostrada.exe`.

## macOS (Apple Silicon ou Intel)

### Prérequis

1. **Xcode**, ou au minimum les Command Line Tools (`xcode-select --install`). Ils fournissent
   `cc`, `lipo` et `codesign`.
2. **rustup** :

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Build et lancement

```sh
git clone <repo> AudioStrada && cd AudioStrada
cargo build --release -p audiostrada-app --locked
./target/release/audiostrada
```

Lancé depuis le Terminal, c'est le Terminal qui demande l'autorisation d'utiliser le micro, pas
AudioStrada.

### App universelle et `.dmg`

```sh
sh packaging/macos/bundle.sh
```

Le script ajoute lui-même les cibles `aarch64-apple-darwin` et `x86_64-apple-darwin`, compile
les deux en profil `dist`, les fusionne avec `lipo`, puis crée `target/macos/AudioStrada.app` et
`target/macos/AudioStrada-<version>.dmg`.

### Signature (facultative)

Définir ces variables avant de lancer le script :

```sh
export AUDIOSTRADA_SIGN_IDENTITY="Developer ID Application: Nom (TEAMID)"
export AUDIOSTRADA_NOTARY_PROFILE=audiostrada        # profil créé avec xcrun notarytool store-credentials
export AUDIOSTRADA_BUNDLE_ID=com.audiostrada.AudioStrada  # facultatif
```

Sans signature, Gatekeeper bloque le premier lancement. Il faut alors faire clic droit › Ouvrir,
ou lancer :

```sh
xattr -dr com.apple.quarantine target/macos/AudioStrada.app
```

### LV2 (facultatif)

`brew install lilv`. Sur Apple Silicon, Homebrew installe dans `/opt/homebrew/lib`, un dossier
que le chargeur ne parcourt pas par défaut. Pour un lancement depuis le Terminal :

```sh
DYLD_FALLBACK_LIBRARY_PATH=/opt/homebrew/lib ./target/release/audiostrada
```
