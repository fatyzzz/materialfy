#!/usr/bin/env bash
# Сборка Materialfy для Android (arm64-v8a): Rust (cxx-qt) → libmaterialfy_arm64-v8a.so → androiddeployqt → APK.
#
#   QT_ANDROID=~/Qt/6.12.0/android_arm64_v8a QT_HOST=~/Qt/6.12.0/gcc_64 \
#   ANDROID_SDK_ROOT=~/Android/Sdk ANDROID_NDK_ROOT=~/Android/Sdk/ndk/28.2.13676358 \
#   android/build_apk.sh [--release] [--skip-cargo]
#
# Qt для Android ставится тем же скриптом, что и десктопный:
#   python3 .github/scripts/install_qt.py --host android --out ~/Qt/6.12.0/android_arm64_v8a
# (+ десктопный Qt той же версии для хостовых инструментов: moc, rcc, qmlimportscanner, androiddeployqt).
# --release — оптимизированный Rust. Подпись: ANDROID_KEYSTORE, ANDROID_KEY_ALIAS, ANDROID_KEYSTORE_PASS
# (иначе APK подписан отладочным ключом Android SDK — его можно поставить, но не обновить поверх другой подписи).
# Результат: target/android/Materialfy-<version>-arm64-v8a(-debug).apk
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
profile=debug
skip_cargo=0
for arg in "$@"; do
  case "$arg" in
    --release) profile=release ;;
    --skip-cargo) skip_cargo=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

: "${QT_ANDROID:?set QT_ANDROID to the Qt for Android prefix (…/android_arm64_v8a)}"
: "${QT_HOST:?set QT_HOST to the desktop Qt prefix of the same version (…/gcc_64)}"
: "${ANDROID_SDK_ROOT:?set ANDROID_SDK_ROOT}"
: "${ANDROID_NDK_ROOT:?set ANDROID_NDK_ROOT}"
API="${ANDROID_API:-28}"
PLATFORM="${ANDROID_PLATFORM:-android-36}"
BUILD_TOOLS="${ANDROID_BUILD_TOOLS:-$(ls "$ANDROID_SDK_ROOT/build-tools" | sort -V | tail -1)}"
TARGET=aarch64-linux-android
ABI=arm64-v8a
version="$(grep -m1 '^version = ' "$root/Cargo.toml" | sed -E 's/version = "(.*)"/\1/')"

host_tag=linux-x86_64
[[ "$(uname -s)" == Darwin ]] && host_tag=darwin-x86_64
toolchain="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/$host_tag"
bin="$toolchain/bin"

# Qt для Android ссылается на хостовый Qt из своего bin/qmake (сборка Qt — на машине CI Qt): поправить на QT_HOST
fix_qt_paths() {
  local conf="$QT_ANDROID/bin/target_qt.conf"
  if grep -q '^HostPrefix=../../$' "$conf" 2>/dev/null || ! grep -q "HostPrefix=$QT_HOST" "$conf"; then
    sed -i.bak -E "s#^HostPrefix=.*#HostPrefix=$QT_HOST#; s#^HostData=.*#HostData=$QT_ANDROID#; s#^HostSpec=.*#HostSpec=linux-g++#" "$conf"
  fi
  for script in qmake qmake6 qtpaths qtpaths6; do
    [[ -f "$QT_ANDROID/bin/$script" ]] && sed -i.bak -E "s#^(\S*/)bin/(qmake6|qtpaths6) #$QT_HOST/bin/\2 #" "$QT_ANDROID/bin/$script"
  done
}
fix_qt_paths

out="$root/target/android"
pkg="$out/package"          # исходники пакета: манифест, Java, ресурсы, ассеты
build="$out/build"          # рабочий каталог androiddeployqt и Gradle
mkdir -p "$out"

if [[ $skip_cargo == 0 ]]; then
  export QMAKE="$QT_ANDROID/bin/qmake6"
  export CC_aarch64_linux_android="$bin/${TARGET}${API}-clang"
  export CXX_aarch64_linux_android="$bin/${TARGET}${API}-clang++"
  export AR_aarch64_linux_android="$bin/llvm-ar"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/${TARGET}${API}-clang"
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$bin/llvm-ar"
  # libc++ общая (как у Qt), иначе два рантайма C++ в процессе
  export CXXSTDLIB_aarch64_linux_android=c++_shared
  # Библиотеки Qt для Android названы с ABI (libQt6Core_arm64-v8a.so), а cxx-qt линкует -lQt6Core: ссылки без
  # суффикса. В DT_NEEDED попадает SONAME (с суффиксом) — его и найдёт загрузчик в APK.
  qtlinks="$out/qt-link"
  mkdir -p "$qtlinks"
  for so in "$QT_ANDROID"/lib/libQt6*_"$ABI".so; do
    base="$(basename "$so" "_$ABI.so")"
    ln -sf "$so" "$qtlinks/$base.so"
  done
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-L native=$qtlinks -C link-arg=-Wl,-z,max-page-size=16384 ${CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS:-}"
  cargo_profile=()
  [[ $profile == release ]] && cargo_profile=(--release)
  (cd "$root" && cargo build --locked "${cargo_profile[@]}" --target "$TARGET" -p materialfy)
fi

lib="$root/target/$TARGET/$profile/materialfy"
[[ -f "$lib" ]] || { echo "no $lib" >&2; exit 1; }

# Каталог пакета: шаблон (манифест, Java, ресурсы) + ассеты (QML и шрифты распаковываются при первом запуске)
rm -rf "$build" "$pkg"
mkdir -p "$build/libs/$ABI" "$pkg/assets/materialfy"
cp -r "$root/android/AndroidManifest.xml" "$root/android/src" "$root/android/res" "$pkg/"
cp -r "$root/src/materialfy/qml" "$pkg/assets/materialfy/qml"
# метка сборки: приложение распаковывает QML заново, когда она сменилась, — и при переустановке той же версии
printf '%s' "$(date -u +%Y%m%d%H%M%S)-$(git -C "$root" rev-parse --short HEAD 2>/dev/null || echo local)" > "$pkg/assets/materialfy/build-id"
mkdir -p "$pkg/assets/materialfy/assets"
# fonts-src — исходники для сборки подмножеств, в APK не нужны
(cd "$root/src/materialfy/assets" && find . -path ./fonts-src -prune -o -type f -print | while read -r f; do
  mkdir -p "$pkg/assets/materialfy/assets/$(dirname "$f")"
  cp "$f" "$pkg/assets/materialfy/assets/$f"
done)
# Переводы интерфейса i18n/*.json встроены в бинарник (mf-i18n/build.rs); переводы Qt берёт androiddeployqt
cp "$lib" "$out/libmaterialfy_$ABI.so"

# OpenSSL 3 для Qt Network: TLS-плагин Qt (qopensslbackend) грузит libssl_3/libcrypto_3 сам, но в Qt для Android
# их нет — без них Qt не открывает ни одной https-ссылки (обложки, аватары). Сборки KDAB, которые советует Qt;
# коммит зафиксирован. OPENSSL_ANDROID — свой каталог с libssl_3.so и libcrypto_3.so для $ABI.
OPENSSL_REV=b71f1470962019bd89534a2919f5925f93bc5779
ssl_dir="${OPENSSL_ANDROID:-}"
if [[ -z "$ssl_dir" ]]; then
  ssl_src="$root/target/ext/android_openssl"
  if [[ "$(git -C "$ssl_src" rev-parse HEAD 2>/dev/null)" != "$OPENSSL_REV" ]]; then
    rm -rf "$ssl_src"
    mkdir -p "$ssl_src"
    git -C "$ssl_src" init -q
    git -C "$ssl_src" remote add origin https://github.com/KDAB/android_openssl
    git -C "$ssl_src" sparse-checkout set "ssl_3/$ABI"
    git -C "$ssl_src" fetch -q --depth 1 --filter=blob:none origin "$OPENSSL_REV"
    git -C "$ssl_src" checkout -q FETCH_HEAD
  fi
  ssl_dir="$ssl_src/ssl_3/$ABI"
fi
for so in libssl_3.so libcrypto_3.so; do
  [[ -f "$ssl_dir/$so" ]] || { echo "OpenSSL for Android not found: $ssl_dir/$so" >&2; exit 1; }
done

cat > "$out/deployment-settings.json" <<JSON
{
  "description": "Materialfy",
  "qt": { "$ABI": "$QT_ANDROID" },
  "sdk": "$ANDROID_SDK_ROOT",
  "sdkBuildToolsRevision": "$BUILD_TOOLS",
  "ndk": "$ANDROID_NDK_ROOT",
  "toolchain-prefix": "llvm",
  "tool-prefix": "llvm",
  "ndk-host": "$host_tag",
  "architectures": { "$ABI": "$TARGET" },
  "android-min-sdk-version": "$API",
  "android-target-sdk-version": "36",
  "android-package-source-directory": "$pkg",
  "android-version-code": "$(echo "$version" | awk -F. '{ printf "%d%02d%02d", $1, $2, $3 }')",
  "android-version-name": "$version",
  "android-app-name": "Materialfy",
  "android-package-name": "org.materialfy",
  "qml-importscanner-binary": "$QT_HOST/libexec/qmlimportscanner",
  "rcc-binary": "$QT_HOST/libexec/rcc",
  "qml-root-path": ["$root/src/materialfy/qml"],
  "qml-import-paths": ["$root/src/materialfy/qml"],
  "stdcpp-path": "$toolchain/sysroot/usr/lib/",
  "application-binary": "materialfy",
  "android-extra-libs": "$ssl_dir/libssl_3.so,$ssl_dir/libcrypto_3.so",
  "qrcFiles": "",
  "zstdCompression": false
}
JSON

deploy=(--input "$out/deployment-settings.json" --output "$build" --android-platform "$PLATFORM"
        --jdk "${JAVA_HOME:-$(dirname "$(dirname "$(readlink -f "$(command -v javac || command -v java)")")")}"
        --gradle --verbose)
cp "$out/libmaterialfy_$ABI.so" "$build/libs/$ABI/"
# Без ключа APK подписывается отладочным ключом (ставится на телефон как есть); с ключом — release-пакет
suffix="-debugsigned"
if [[ -n "${ANDROID_KEYSTORE:-}" ]]; then
  : "${ANDROID_KEY_ALIAS:?}" "${ANDROID_KEYSTORE_PASS:?}"
  deploy+=(--release)
  suffix=""
fi
[[ $profile == debug ]] && suffix="-debug"
"$QT_HOST/bin/androiddeployqt" "${deploy[@]}"

# androiddeployqt кладёт всё, до чего дотягивается импорт QtQuick.Dialogs: стили Controls, которых интерфейс не
# использует (он рисует свои компоненты Md3), отладчик QML, редкие форматы картинок, платформы без экрана.
# Убираем их из пакета и из списка предзагрузки (res/values/libs.xml) и пересобираем Gradle.
prune=(
  'libQt6QuickControls2FluentWinUI3*' 'libqml_QtQuick_Controls_FluentWinUI3*'
  'libQt6QuickControls2Imagine*' 'libqml_QtQuick_Controls_Imagine*'
  'libQt6QuickControls2Universal*' 'libqml_QtQuick_Controls_Universal*'
  'libQt6QuickControls2Fusion*' 'libqml_QtQuick_Controls_Fusion*'
  'libplugins_qmltooling_*'
  'libplugins_platforms_qminimal_*' 'libplugins_platforms_qoffscreen_*'
  'libplugins_imageformats_qtiff_*' 'libplugins_imageformats_qtga_*' 'libplugins_imageformats_qwbmp_*'
  'libplugins_imageformats_qicns_*' 'libplugins_imageformats_qpdf_*'
)
libdir="$build/libs/$ABI"
for pattern in "${prune[@]}"; do
  for f in "$libdir"/$pattern; do
    [[ -e "$f" ]] || continue
    name="$(basename "$f" .so)"
    rm -f "$f"
    sed -i "/<item>$ABI;${name#lib}<\/item>/d" "$build/res/values/libs.xml"
  done
done
# Библиотеки в APK — сжатыми: файл почти вдвое меньше (распаковываются при установке)
sed -i 's/^legacyPackaging=.*/legacyPackaging=true/' "$build/gradle.properties"
rm -rf "$build/build/outputs/apk"
if [[ -n "${ANDROID_KEYSTORE:-}" ]]; then
  (cd "$build" && ./gradlew --no-daemon assembleRelease)
  unsigned="$(find "$build/build/outputs/apk/release" -name '*.apk' | head -1)"
  tools="$ANDROID_SDK_ROOT/build-tools/$BUILD_TOOLS"
  "$tools/zipalign" -f -p 4 "$unsigned" "$out/aligned.apk"
  "$tools/apksigner" sign --ks "$ANDROID_KEYSTORE" --ks-key-alias "$ANDROID_KEY_ALIAS" \
    --ks-pass "pass:$ANDROID_KEYSTORE_PASS" --out "$out/Materialfy-$version-$ABI.apk" "$out/aligned.apk"
  rm -f "$out/aligned.apk"
else
  (cd "$build" && ./gradlew --no-daemon assembleDebug)
  cp "$(find "$build/build/outputs/apk/debug" -name '*.apk' | head -1)" "$out/Materialfy-$version-$ABI$suffix.apk"
fi
echo "$out/Materialfy-$version-$ABI$suffix.apk"
