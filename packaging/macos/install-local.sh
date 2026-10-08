#!/bin/bash
# Build Spotifast and replace /Applications/Spotifast.app with it, on this Mac.
#
#   packaging/macos/install-local.sh
#
# Signs with the first "Developer ID Application" identity in the keychain,
# or CODESIGN_IDENTITY when set. A stable identity keeps the app's designated
# requirement the same across builds, so the keychain grant ("Always Allow")
# for the stored sign-in survives an update. An ad-hoc signature changes with
# every build and asks for the keychain password each time.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

if [ -z "${CODESIGN_IDENTITY:-}" ]; then
    CODESIGN_IDENTITY="$(security find-identity -v -p codesigning |
        sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -n 1)"
fi
if [ -z "$CODESIGN_IDENTITY" ]; then
    echo "No Developer ID Application identity; set CODESIGN_IDENTITY." >&2
    exit 1
fi
export CODESIGN_IDENTITY
# A personal icon for this Mac's build, kept out of the repository.
if [ -z "${ICON_PNG:-}" ] && [ -f packaging/macos/local-icon.png ]; then
    export ICON_PNG="$root/packaging/macos/local-icon.png"
fi

cargo build --release --locked
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)-$(git rev-parse --short HEAD)"
staging="$root/.cache/install-local"
rm -rf "$staging"
mkdir -p "$staging"
bash packaging/macos/bundle.sh target/release/spotifast "$staging/Spotifast.app" "$version"

osascript -e 'quit app "Spotifast"' 2>/dev/null || true
while pgrep -f 'Spotifast.app/Contents/MacOS/Spotifast' >/dev/null; do sleep 1; done
rm -rf /Applications/Spotifast.app
mv "$staging/Spotifast.app" /Applications/Spotifast.app
rm -rf "$staging"
open /Applications/Spotifast.app
echo "Installed Spotifast $version"
