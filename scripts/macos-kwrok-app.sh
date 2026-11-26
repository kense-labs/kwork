#!/usr/bin/env bash
# Build a launchable, ad-hoc signed macOS app from the native Kar workbench.
set -euo pipefail

if [[ $# != 2 || ! -x "$1" ]]; then
    echo 'Usage: macos-kwrok-app.sh KAR_DESKTOP_BINARY DESTINATION_DIRECTORY' >&2
    exit 2
fi

binary="$1"
mkdir -p "$2"
destination="$(cd "$2" && pwd)/Kwrok.app"
stage="$(mktemp -d "$(dirname "$destination")/.kwrok-package.XXXXXXXX")"
app="$stage/Kwrok.app"
cleanup() {
    if [[ -e "$stage/previous.app" && ! -e "$destination" ]]; then
        mv "$stage/previous.app" "$destination" || return 1
    fi
    rm -rf "$stage"
}
trap cleanup EXIT
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -1)"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/Licenses"
install -m755 "$binary" "$app/Contents/MacOS/Kwrok"
install -m644 "$root/assets/icons/Kwork.icns" "$app/Contents/Resources/Kwork.icns"
install -m644 "$root/LICENSE-MIT" "$root/LICENSE" "$root/NOTICE" "$app/Contents/Resources/Licenses/"
"$root/scripts/macos-dictation.sh" "$app"
python3 "$root/scripts/third-party-licenses.py" "$app/Contents/Resources/Licenses/THIRD_PARTY_LICENSES.txt" kar_desktop kwrok_dictation
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>NSMicrophoneUsageDescription</key><string>Dictate a message into the conversation composer.</string>
<key>CFBundleName</key><string>Kwrok</string>
    <key>CFBundleDisplayName</key><string>Kwrok</string>
    <key>CFBundleIdentifier</key><string>com.kense-labs.kwrok.desktop</string>
    <key>CFBundleExecutable</key><string>Kwrok</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleIconFile</key><string>Kwork.icns</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
plutil -lint "$app/Contents/Info.plist" >/dev/null
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
if [[ -e "$destination" ]]; then
    mv "$destination" "$stage/previous.app"
fi
mv "$app" "$destination"
echo "$destination"
