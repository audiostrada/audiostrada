#!/bin/sh
# Writes the Flatpak manifest and its sources in target/flatpak/.
# With a tag (v1.0.0), they build that tag from GitHub: the files of the Flathub repository.
# Without, they build this working tree (flatpak-builder target/flatpak-build target/flatpak/*.yml).
# Needs flatpak-cargo-generator.py (flatpak-builder-tools) at $CARGO_GENERATOR, and python3
# with aiohttp and tomlkit.
set -eu
cd "$(dirname "$0")/../.."
id=com.audiostrada.AudioStrada
out=target/flatpak
rm -rf "$out"
mkdir -p "$out"
if [ $# -gt 0 ]; then
    tag=$1
    date=$(git log -1 --format=%cs "$tag")
    git show "$tag:Cargo.lock" > "$out/Cargo.lock"
    cat > "$out/audiostrada-source.json" <<EOF
{
    "type": "git",
    "url": "https://github.com/audiostrada/audiostrada.git",
    "tag": "$tag",
    "commit": "$(git rev-list -n 1 "$tag")"
}
EOF
else
    date=$(git log -1 --format=%cs)
    cp Cargo.lock "$out/Cargo.lock"
    cat > "$out/audiostrada-source.json" <<EOF
{
    "type": "dir",
    "path": "../..",
    "skip": ["target", ".git", ".flatpak-builder", "flatpak_app", "repo"]
}
EOF
fi
sed "s/@DATE@/$date/" "packaging/flatpak/$id.yml" > "$out/$id.yml"
python3 "${CARGO_GENERATOR:-flatpak-cargo-generator.py}" "$out/Cargo.lock" -o "$out/cargo-sources.json"
rm "$out/Cargo.lock"
echo '{ "only-arches": ["x86_64"] }' > "$out/flathub.json"
