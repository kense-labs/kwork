#!/usr/bin/env bash
# Build a launchable, ad-hoc signed macOS app from the native Kar workbench.
set -euo pipefail

if [[ $# != 2 || ! -x "$1" ]]; then
    echo 'Usage: macos-kwrok-app.sh KAR_DESKTOP_BINARY DESTINATION_DIRECTORY' >&2
    exit 2
fi

binary="$1"
app="$2/Kwrok.app"
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -1)"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 "$binary" "$app/Contents/MacOS/Kwrok"
install -m644 "$root/assets/icons/Kwork.icns" "$app/Contents/Resources/Kwork.icns"
"$root/scripts/macos-dictation.sh" "$app"
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
echo "$app"
