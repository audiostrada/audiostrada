#!/bin/sh
# Builds target/snap/audiostrada_<version>_amd64.snap.
# Needs snapcraft and Ubuntu 24.04 (core24), where it builds in place (--destructive-mode).
# SNAPCRAFT overrides the command (the CI runs it with sudo).
# AUDIOSTRADA_FEATURES adds cargo features (the CI builds with "official").
set -eu
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build -p audiostrada-app --profile dist --locked ${AUDIOSTRADA_FEATURES:+--features "$AUDIOSTRADA_FEATURES"}
cargo build -p audiostrada-mp3 --profile dist --locked
out=target/snap
root=$out/root
rm -rf "$out"
mkdir -p "$root/usr/bin" "$root/usr/share/applications" "$root/usr/share/mime/packages" \
    "$root/usr/share/metainfo"
cp target/dist/audiostrada target/dist/audiostrada-mp3 "$root/usr/bin/"
cp packaging/linux/audiostrada.desktop "$root/usr/share/applications/"
cp packaging/linux/audiostrada.xml "$root/usr/share/mime/packages/"
cp packaging/linux/com.audiostrada.AudioStrada.metainfo.xml "$root/usr/share/metainfo/"
for size in 16 32 48 64 128 256 512; do
    mkdir -p "$root/usr/share/icons/hicolor/${size}x${size}/apps"
    cp "packaging/icons/audiostrada-$size.png" "$root/usr/share/icons/hicolor/${size}x${size}/apps/audiostrada.png"
done
sed "s/@VERSION@/$version/" packaging/snap/snapcraft.yaml > "$out/snapcraft.yaml"
cd "$out"
${SNAPCRAFT:-snapcraft} pack --destructive-mode --output "audiostrada_${version}_amd64.snap"
