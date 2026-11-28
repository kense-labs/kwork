#!/usr/bin/env bash
# Bundle the pinned default ACP runtime helper into Contents/Helpers/aster-acp.
set -euo pipefail

app="$1"
version="9.3.1"
archive_name="aster-acp-v${version}-macos.tar.gz"
archive_url="https://github.com/aster-ai-lab/aster/releases/download/aster-acp-v${version}/${archive_name}"
archive_sha256="063e6a1ec61e4adeef7133b738ae0bf3e6860f356b544a1859c5c22265b5e6fc"
binary_sha256="d4c77e77701e34c6f0a6481575d025e371b6bad49cda8f567bf167135095b5c6"

cache="${XDG_CACHE_HOME:-$HOME/Library/Caches}/Kwork/aster/${version}"
local_managed="$HOME/.local/share/kwork/runtimes/aster/${version}/aster-acp"
mkdir -p "$cache" "$app/Contents/Helpers"
tmp="$(mktemp -d "$cache/package.XXXXXXXX")"
trap 'rm -rf "$tmp"' EXIT

sha256_of() {
    shasum -a 256 < "$1" | cut -d' ' -f1
}

if [[ ! -f "$cache/aster-acp" ]] || [[ "$(sha256_of "$cache/aster-acp")" != "$binary_sha256" ]]; then
    if [[ -f "$local_managed" ]] && [[ "$(sha256_of "$local_managed")" == "$binary_sha256" ]]; then
        cp "$local_managed" "$tmp/aster-acp"
    else
        curl --fail --silent --show-error --location --retry 3 --connect-timeout 20 --max-time 300 \
            "$archive_url" -o "$tmp/$archive_name"
        if [[ "$(sha256_of "$tmp/$archive_name")" != "$archive_sha256" ]]; then
            echo "Checksum mismatch for $archive_name" >&2
            exit 1
        fi
        tar -xzf "$tmp/$archive_name" -C "$tmp"
        found="$(find "$tmp" -type f -name 'aster-acp' | head -n 1)"
        if [[ -z "$found" ]]; then
            echo "Missing aster-acp executable in $archive_name" >&2
            exit 1
        fi
        if [[ "$found" != "$tmp/aster-acp" ]]; then
            mv -f "$found" "$tmp/aster-acp"
        fi
    fi
    if [[ "$(sha256_of "$tmp/aster-acp")" != "$binary_sha256" ]]; then
        echo "Checksum mismatch for unpacked aster-acp binary" >&2
        exit 1
    fi
    chmod 755 "$tmp/aster-acp"
    mv -f "$tmp/aster-acp" "$cache/aster-acp"
fi

main_bin="$app/Contents/MacOS/Kwork"
if [[ ! -f "$main_bin" ]]; then
    main_bin="$app/Contents/MacOS/Kwrok"
fi
for arch in $(lipo -archs "$main_bin"); do
    lipo "$cache/aster-acp" -verify_arch "$arch"
done

program="$app/Contents/Helpers/aster-acp"
install -m755 "$cache/aster-acp" "$program"
codesign --force --sign - "$program"
