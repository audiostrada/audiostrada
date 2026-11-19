#!/bin/sh
# Builds the APT and RPM repositories of packages.audiostrada.com into <out>, from the .deb and
# .rpm files in <packages>, signed with gpg's default key (the packages key), and its home page.
# Needs apt-ftparchive (apt-utils), createrepo_c and gpg.
set -eu
packages=$(realpath "$1")
out=$(realpath -m "$2")
cd "$(dirname "$0")/../.."
rm -rf "$out"
pool=$out/deb/pool/main/a/audiostrada
mkdir -p "$pool" "$out/deb/dists/stable/main/binary-amd64" "$out/rpm"
cp "$packages"/*.deb "$pool/"
cp "$packages"/*.rpm "$out/rpm/"
cp packaging/linux/audiostrada-packages.asc packaging/linux/audiostrada.sources \
    packaging/linux/audiostrada.repo "$out/"
# The home page, listing the packages, newest first.
list=$(for dir in deb/pool/main/a/audiostrada rpm; do
    (cd "$out/$dir" && ls | sort -rV) | sed "s|.*|    <li><a href=\"$dir/&\">&</a></li>|"
done)
awk -v list="$list" '$0 == "@PACKAGES@" { print list; next } 1' packaging/linux/repo-index.html \
    > "$out/index.html"
cd "$out/deb"
apt-ftparchive packages pool > dists/stable/main/binary-amd64/Packages
gzip -9kn dists/stable/main/binary-amd64/Packages
# Written aside first: the Release file must not list itself.
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=AudioStrada \
    -o APT::FTPArchive::Release::Label=AudioStrada \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    release dists/stable > "$out/Release"
mv "$out/Release" dists/stable/Release
gpg --batch --yes --clearsign -o dists/stable/InRelease dists/stable/Release
gpg --batch --yes --armor --detach-sign -o dists/stable/Release.gpg dists/stable/Release
createrepo_c --quiet "$out/rpm"
gpg --batch --yes --armor --detach-sign -o "$out/rpm/repodata/repomd.xml.asc" \
    "$out/rpm/repodata/repomd.xml"
