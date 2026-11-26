#!/usr/bin/env bash
# Package the native engine Kar invokes and its multilingual starter model.
set -euo pipefail
app="$1"
cache="${XDG_CACHE_HOME:-$HOME/Library/Caches}/Kwork/dictation/0.4.1"
mkdir -p "$cache" "$app/Contents/Helpers" "$app/Contents/Resources/Dictation"
tmp="$(mktemp -d "$cache/package.XXXXXXXX")"
trap 'rm -rf "$tmp"' EXIT

asset() {
    local name="$1" digest="$2" url="$3"
    if ! [[ -f "$cache/$name" ]] || [[ "$(shasum -a 256 < "$cache/$name" | cut -d' ' -f1)" != "$digest" ]]; then
        curl --fail --silent --show-error --location --retry 3 --connect-timeout 20 --max-time 300 \
            "$url" -o "$tmp/$name.download"
        if [[ "$(shasum -a 256 < "$tmp/$name.download" | cut -d' ' -f1)" != "$digest" ]]; then
            echo "Checksum mismatch for dictation asset: $name" >&2
            return 1
        fi
        mv -f "$tmp/$name.download" "$cache/$name"
    fi
}

rev=4d7c1ccefd39e5a9ea36cca5f8fefc6dcc14b99b
asset source.tar.gz 77bfafa35bd7eee447ec37b832e99c342c62b7db0c6cb18c36e763563b586b1f \
    "https://codeload.github.com/handy-computer/transcribe.cpp/tar.gz/$rev"
src="$tmp/transcribe.cpp-$rev"
tar -xzf "$cache/source.tar.gz" -C "$tmp" \
    "transcribe.cpp-$rev/examples" "transcribe.cpp-$rev/include" "transcribe.cpp-$rev/catalog/whisper-base.json"

for arch in $(lipo -archs "$app/Contents/MacOS/Kwrok"); do
    case "$arch" in
        arm64) tuple=macos-arm64-metal; hash=7369e36dc2a5fc5d52eee5766fa03ad22d3f533c8688bf1deccf3ffdc202ea24 ;;
        x86_64) tuple=macos-x86_64-cpu; hash=513a3976a2f0ebc0666c9670551e96c226e39ed14090a51f2efa0874f41258d8 ;;
        *) echo "Unsupported macOS dictation architecture: $arch" >&2; exit 1 ;;
    esac
    archive="transcribe-native-0.4.1-$tuple.tar.gz"
    asset "$archive" "$hash" "https://github.com/handy-computer/transcribe.cpp/releases/download/v0.4.1/$archive"
    tar -xzf "$cache/$archive" -C "$tmp"
    native="$tmp/transcribe-native-$tuple"
    libs="$app/Contents/Frameworks/Dictation-$arch"
    mkdir -p "$libs"
    for lib in "$native"/*.dylib; do
        install -m755 "$lib" "$libs/"
        codesign --force --sign - "$libs/$(basename "$lib")"
    done
    cp -R "$native/licenses" "$app/Contents/Resources/Dictation/"
    # Upstream distributes the inference libraries and the matching CLI sources separately.
    helper="$app/Contents/Helpers/transcribe-$arch"
    xcrun clang++ -std=c++17 -O2 -arch "$arch" -I"$src/include" -I"$src/examples/common" \
        "$src"/examples/cli/*.cpp "$src/examples/common/wav.cpp" -L"$libs" -ltranscribe \
        "-Wl,-rpath,@executable_path/../Frameworks/Dictation-$arch" -o "$helper"
    codesign --force --sign - "$helper"
done

model=whisper-base-Q8_0.gguf
asset "$model" 81c069428bc8a24551a8169cf31cf09bcfd9d4cf50389ae281323c9aa9648c81 \
    "https://huggingface.co/handy-computer/whisper-base-gguf/resolve/e0f69524f648720eca44c024d1d0dbb7027d1fa0/$model"
asset Apache-2.0.txt cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30 \
    https://www.apache.org/licenses/LICENSE-2.0.txt
install -m644 "$cache/$model" "$src/catalog/whisper-base.json" "$cache/Apache-2.0.txt" "$app/Contents/Resources/Dictation/"
