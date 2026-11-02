#!/bin/sh
# Builds target/macos/AudioStrada.app (universal) and AudioStrada-<version>.dmg.
# Signing: set AUDIOSTRADA_SIGN_IDENTITY (e.g. "Developer ID Application: Name (TEAMID)").
# Notarization: also set AUDIOSTRADA_NOTARY_PROFILE, a profile stored with
# `xcrun notarytool store-credentials`. AUDIOSTRADA_BUNDLE_ID overrides the bundle identifier.
# AUDIOSTRADA_FEATURES adds cargo features (the release CI builds with "official").
set -eu
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
bundle_id=${AUDIOSTRADA_BUNDLE_ID:-com.audiostrada.AudioStrada}
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$target"
    cargo build -p audiostrada-app --profile dist --locked --target "$target" \
        ${AUDIOSTRADA_FEATURES:+--features "$AUDIOSTRADA_FEATURES"}
done
out=target/macos
app=$out/AudioStrada.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/audiostrada" \
    target/aarch64-apple-darwin/dist/audiostrada target/x86_64-apple-darwin/dist/audiostrada
cp packaging/icons/audiostrada.icns "$app/Contents/Resources/"
sed -e "s/@VERSION@/$version/g" -e "s/@BUNDLE_ID@/$bundle_id/g" \
    packaging/macos/Info.plist > "$app/Contents/Info.plist"
if [ -n "${AUDIOSTRADA_SIGN_IDENTITY:-}" ]; then
    codesign --force --options runtime --timestamp \
        --entitlements packaging/macos/entitlements.plist \
        --sign "$AUDIOSTRADA_SIGN_IDENTITY" "$app"
fi
dmg=$out/AudioStrada-$version.dmg
rm -f "$dmg"
hdiutil create -volname AudioStrada -srcfolder "$app" -ov -format UDZO "$dmg"
if [ -n "${AUDIOSTRADA_SIGN_IDENTITY:-}" ] && [ -n "${AUDIOSTRADA_NOTARY_PROFILE:-}" ]; then
    codesign --force --timestamp --sign "$AUDIOSTRADA_SIGN_IDENTITY" "$dmg"
    xcrun notarytool submit "$dmg" --keychain-profile "$AUDIOSTRADA_NOTARY_PROFILE" --wait
    xcrun stapler staple "$dmg"
fi
echo "$dmg"
