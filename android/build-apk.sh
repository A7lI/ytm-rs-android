#!/usr/bin/env bash
set -euo pipefail

work="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sdk="${ANDROID_HOME:?Set ANDROID_HOME to your Android SDK}"
tools="$sdk/build-tools/35.0.1"
platform="$sdk/platforms/android-35/android.jar"
bin="${ANDROID_NDK_HOME:-$sdk/ndk/28.2.13676358}/toolchains/llvm/prebuilt/linux-x86_64/bin"
out="${1:-$work/target/ytm-rs-android.apk}"
out="$(realpath -m "$out")"
keystore="${ANDROID_KEYSTORE:?Set ANDROID_KEYSTORE to the app signing keystore}"
alias="${ANDROID_KEY_ALIAS:-androiddebugkey}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

cd "$work"
python3 android/alignment/build_transcription.py --ndk "${ANDROID_NDK_HOME:-$sdk/ndk/28.2.13676358}"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/aarch64-linux-android26-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$bin/x86_64-linux-android26-clang"
export CC_aarch64_linux_android="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
export CC_x86_64_linux_android="$CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER"
export CXX_aarch64_linux_android="$bin/aarch64-linux-android26-clang++"
export CXX_x86_64_linux_android="$bin/x86_64-linux-android26-clang++"
export AR_aarch64_linux_android="$bin/llvm-ar" AR_x86_64_linux_android="$bin/llvm-ar"
export LIBCLANG_PATH="$bin/../lib"
export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$bin/../sysroot -resource-dir=$bin/../lib/clang/19"

if [[ "${YTM_SKIP_RUST_BUILD:-0}" != 1 ]]; then
    cargo build --locked --profile android --lib --target aarch64-linux-android --target x86_64-linux-android
fi
mkdir -p "$scratch/classes" "$scratch/dex" "$scratch/apk/lib/arm64-v8a" "$scratch/apk/lib/x86_64" "$(dirname "$out")"
sdk_jars=()
if [[ -n "${DISCORD_ANDROID_AAR:-}" ]]; then
    discord_id="${DISCORD_APPLICATION_ID:?Set DISCORD_APPLICATION_ID to your public Discord Application ID}"
    [[ "$discord_id" =~ ^[1-9][0-9]{16,18}$ ]] || { echo "Invalid Discord Application ID" >&2; exit 1; }
    mkdir -p "$scratch/discord"
    unzip -q "$DISCORD_ANDROID_AAR" 'libs/*.jar' 'prefab/modules/discord_partner_sdk/include/*' \
        'jni/arm64-v8a/libdiscord_partner_sdk.so' 'jni/x86_64/libdiscord_partner_sdk.so' -d "$scratch/discord"
    sdk_jars=("$scratch/discord/libs/"*.jar)
    for mapping in arm64-v8a:aarch64-linux-android x86_64:x86_64-linux-android; do
        abi="${mapping%:*}"
        triple="${mapping#*:}"
        lib_dir="$scratch/apk/lib/$abi"
        "$bin/llvm-strip" --strip-unneeded -o "$lib_dir/libdiscord_partner_sdk.so" \
            "$scratch/discord/jni/$abi/libdiscord_partner_sdk.so"
        "$bin/${triple}26-clang++" -std=c++20 -O2 -shared -fPIC -fvisibility=hidden -static-libstdc++ \
            "-DYTM_DISCORD_APPLICATION_ID=${discord_id}ULL" \
            -I "$scratch/discord/prefab/modules/discord_partner_sdk/include" \
            "$work/android/discord-presence.cpp" -L "$lib_dir" -ldiscord_partner_sdk -llog \
            -Wl,--no-undefined -o "$lib_dir/libytm_discord.so"
    done
fi
"$tools/aapt2" compile --dir "$work/android/res" -o "$scratch/resources.zip"
"$tools/aapt2" link -I "$platform" --manifest "$work/android/AndroidManifest.xml" \
    --java "$scratch" -o "$scratch/base.apk" "$scratch/resources.zip"
mapfile -t java_sources < <(find "$work/android/src" "$scratch/org" -name '*.java' | sort)
if [[ -n "${ANDROID_ECJ_JAR:-}" ]]; then
    java -jar "$ANDROID_ECJ_JAR" -8 -proc:none -warn:none -classpath "$platform" -d "$scratch/classes" "${java_sources[@]}"
else
    javac --release 8 -classpath "$platform" -d "$scratch/classes" "${java_sources[@]}"
fi
mapfile -t class_files < <(find "$scratch/classes" -name '*.class' | sort)
"$tools/d8" --release --lib "$platform" --min-api 26 --output "$scratch/dex" "${class_files[@]}" "${sdk_jars[@]}"
cp "$scratch/dex/"classes*.dex "$scratch/apk/"
cp target/aarch64-linux-android/android/libytm_rs.so "$scratch/apk/lib/arm64-v8a/"
cp target/x86_64-linux-android/android/libytm_rs.so "$scratch/apk/lib/x86_64/"
for mapping in arm64-v8a:aarch64-linux-android x86_64:x86_64-linux-android; do
    abi="${mapping%:*}"
    triple="${mapping#*:}"
    if "$bin/llvm-readelf" -d "$scratch/apk/lib/$abi/libytm_rs.so" | grep 'libc++_shared.so' >/dev/null; then
        cp "$bin/../sysroot/usr/lib/$triple/libc++_shared.so" "$scratch/apk/lib/$abi/"
    fi
done
mkdir -p "$scratch/apk/assets/alignment"
cp target/lyric-alignment/{english.onnx,ggml-base-q5_1.bin} "$scratch/apk/assets/alignment/"
cp -r android/alignment/notices "$scratch/apk/assets/alignment/"
for abi in arm64-v8a x86_64; do
    cp target/lyric-alignment/jni/"$abi"/{libonnxruntime.so,libytm_transcription.so} "$scratch/apk/lib/$abi/"
done
(cd "$scratch/apk" && zip -q -r "$scratch/base.apk" classes*.dex lib && zip -q -0 -r "$scratch/base.apk" assets)
"$tools/zipalign" -P 16 -f 4 "$scratch/base.apk" "$scratch/aligned.apk"
"$tools/apksigner" sign --ks "$keystore" --ks-key-alias "$alias" --ks-pass "${ANDROID_KEYSTORE_PASS:-stdin}" --out "$out" "$scratch/aligned.apk"
"$tools/apksigner" verify "$out"
