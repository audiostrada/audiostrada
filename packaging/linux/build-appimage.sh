#!/bin/sh
# Builds target/appimage/AudioStrada-<version>-x86_64.AppImage.
# PipeWire, ALSA and JACK libraries come from the system, as their daemons must match.
# Needs appimagetool on PATH (https://github.com/AppImage/appimagetool/releases).
# AUDIOSTRADA_FEATURES adds cargo features (the release CI builds with "official").
set -eu
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build -p audiostrada-app --profile dist --locked ${AUDIOSTRADA_FEATURES:+--features "$AUDIOSTRADA_FEATURES"}
cargo build -p audiostrada-mp3 --profile dist --locked
app=target/appimage/AudioStrada.AppDir
rm -rf "$app"
mkdir -p "$app/usr/bin" "$app/usr/share/applications" "$app/usr/share/mime/packages" \
    "$app/usr/share/metainfo"
cp target/dist/audiostrada target/dist/audiostrada-mp3 "$app/usr/bin/"
cp packaging/linux/audiostrada.desktop "$app/usr/share/applications/"
cp packaging/linux/audiostrada.desktop "$app/"
cp packaging/linux/audiostrada.xml "$app/usr/share/mime/packages/"
cp packaging/linux/com.audiostrada.AudioStrada.metainfo.xml "$app/usr/share/metainfo/com.audiostrada.AudioStrada.metainfo.xml"
for size in 16 32 48 64 128 256 512; do
    mkdir -p "$app/usr/share/icons/hicolor/${size}x${size}/apps"
    cp "packaging/icons/audiostrada-$size.png" "$app/usr/share/icons/hicolor/${size}x${size}/apps/audiostrada.png"
done
cp packaging/icons/audiostrada-256.png "$app/audiostrada.png"
cat > "$app/AppRun" <<'RUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/audiostrada" "$@"
RUN
chmod +x "$app/AppRun"
ARCH=x86_64 appimagetool "$app" "target/appimage/AudioStrada-$version-x86_64.AppImage"
