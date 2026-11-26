#!/usr/bin/env bash
# Bundle the native Rust Whisper helper and the multilingual model accepted by Kar.
set -euo pipefail

app="$1"
root="$(cd "$(dirname "$0")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
host="$(rustc -vV | sed -n 's/^host: //p')"
cache="${XDG_CACHE_HOME:-$HOME/Library/Caches}/Kwork/dictation/0.4.1"
resources="$app/Contents/Resources/Dictation"
mkdir -p "$cache" "$app/Contents/Helpers" "$resources"
tmp="$(mktemp -d "$cache/package.XXXXXXXX")"
trap 'rm -rf "$tmp"' EXIT

asset() {
    local name="$1" digest="$2" url="$3"
    if [[ ! -f "$cache/$name" ]] || [[ "$(shasum -a 256 < "$cache/$name" | cut -d' ' -f1)" != "$digest" ]]; then
        curl --fail --silent --show-error --location --retry 3 --connect-timeout 20 --max-time 300 \
            "$url" -o "$tmp/$name"
        if [[ "$(shasum -a 256 < "$tmp/$name" | cut -d' ' -f1)" != "$digest" ]]; then
            echo "Checksum mismatch for dictation asset: $name" >&2
            return 1
        fi
        mv -f "$tmp/$name" "$cache/$name"
    fi
    install -m644 "$cache/$name" "$resources/$name"
}

slices=()
for arch in $(lipo -archs "$app/Contents/MacOS/Kwrok"); do
    case "$arch" in
        arm64) triple=aarch64-apple-darwin ;;
        x86_64) triple=x86_64-apple-darwin ;;
        *) echo "Unsupported macOS dictation architecture: $arch" >&2; exit 1 ;;
    esac
    args=(--locked --release --manifest-path "$root/Cargo.toml" --target-dir "$target_dir" -p kwrok_dictation --bin kwrok-dictation)
    helper="$target_dir/release/kwrok-dictation"
    if [[ "$triple" != "$host" ]] || [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
        args+=(--target "$triple")
        helper="$target_dir/$triple/release/kwrok-dictation"
    fi
    cargo build "${args[@]}"
    lipo -verify_arch "$arch" "$helper"
    cp "$helper" "$tmp/helper-$arch"
    slices+=("$tmp/helper-$arch")
done

program="$app/Contents/Helpers/kwrok-dictation"
if [[ ${#slices[@]} == 1 ]]; then
    install -m755 "${slices[0]}" "$program"
else
    lipo -create "${slices[@]}" -output "$program"
fi
codesign --force --sign - "$program"

model=whisper-base-Q8_0.gguf
asset "$model" 81c069428bc8a24551a8169cf31cf09bcfd9d4cf50389ae281323c9aa9648c81 \
    "https://huggingface.co/handy-computer/whisper-base-gguf/resolve/e0f69524f648720eca44c024d1d0dbb7027d1fa0/$model"
asset Apache-2.0.txt cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30 \
    https://www.apache.org/licenses/LICENSE-2.0.txt
