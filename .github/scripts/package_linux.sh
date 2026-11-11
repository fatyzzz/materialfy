#!/usr/bin/env bash
# Linux packaging: AppDir -> AppImage (linuxdeploy + linuxdeploy-plugin-qt) plus a tar.gz of the same AppDir.
#
#   QMAKE=~/Qt/6.12.0/gcc_64/bin/qmake6 .github/scripts/package_linux.sh 0.1.0
#
# Layout: usr/bin/materialfy, usr/bin/qml (application QML - the shell looks for <exe>/qml), usr/bin/assets,
# Qt and its QML modules in usr/lib, usr/plugins, usr/qml (qt.conf is written by the plugin). libasound and
# libpulse (librespot/cpal audio) are picked up by linuxdeploy as regular dependencies of the executable.
# Output goes to target/dist.
set -euo pipefail

VERSION="${1:?usage: package_linux.sh VERSION}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EXE="${MATERIALFY_EXE:-$ROOT/target/release/materialfy}"
OUT="$ROOT/target/dist"
APPDIR="$OUT/AppDir"
TOOLS="${LINUXDEPLOY_DIR:-$OUT/tools}"
ARCH="$(uname -m)"

[ -x "$EXE" ] || { echo "$EXE not found - run cargo build --release -p materialfy first" >&2; exit 1; }
: "${QMAKE:?QMAKE must point to qmake6 from Qt 6.12}"
QTDIR="$("$QMAKE" -query QT_INSTALL_PREFIX)"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps" "$APPDIR/usr/share/icons/hicolor/512x512/apps" "$TOOLS"

install -m755 "$EXE" "$APPDIR/usr/bin/materialfy"
cp -r "$ROOT/src/materialfy/qml" "$APPDIR/usr/bin/qml"
find "$APPDIR/usr/bin/qml" -name __pycache__ -prune -exec rm -rf {} +
mkdir -p "$APPDIR/usr/bin/assets"
cp -r "$ROOT/src/materialfy/assets/fonts" "$APPDIR/usr/bin/assets/fonts"
find "$ROOT/src/materialfy/assets" -maxdepth 1 -type f \( -name '*.svg' -o -name '*.png' \) \
    -exec cp {} "$APPDIR/usr/bin/assets/" \;
cp "$ROOT/src/materialfy/assets/materialfy.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/materialfy.svg"
cp "$ROOT/materialfy.png" "$APPDIR/usr/share/icons/hicolor/512x512/apps/materialfy.png"

cat > "$APPDIR/usr/share/applications/materialfy.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=Materialfy
GenericName=Music Player
GenericName[ru]=Музыкальный плеер
GenericName[pt_BR]=Reprodutor de música
Comment=Spotify client in Material 3 Expressive style
Comment[ru]=Клиент Spotify в стиле Material 3 Expressive
Comment[pt_BR]=Cliente Spotify em Material 3 Expressive
Exec=materialfy %U
Icon=materialfy
Terminal=false
Categories=AudioVideo;Audio;Player;
StartupWMClass=materialfy
DESKTOP

fetch_tool() {
    local name="$1" url="$2"
    if [ ! -x "$TOOLS/$name" ]; then
        # GitHub release downloads sometimes answer 500 for minutes: retry for about three minutes
        curl -fsSL --retry 12 --retry-delay 15 --retry-all-errors -o "$TOOLS/$name" "$url"
        chmod +x "$TOOLS/$name"
    fi
}
fetch_tool "linuxdeploy-$ARCH.AppImage" \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"
fetch_tool "linuxdeploy-plugin-qt-$ARCH.AppImage" \
    "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-$ARCH.AppImage"
export PATH="$TOOLS:$PATH"
# CI runners have no FUSE
export APPIMAGE_EXTRACT_AND_RUN=1

export QMAKE
export QML_SOURCES_PATHS="$ROOT/src/materialfy/qml"
export LD_LIBRARY_PATH="$QTDIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# Wayland next to xcb if this Qt build has it; offscreen for --screenshot (CI smoke test)
wayland=()
for p in "$QTDIR"/plugins/platforms/libqwayland*.so "$QTDIR"/plugins/platforms/libqoffscreen.so; do
    [ -e "$p" ] && wayland+=("$(basename "$p")")
done
if [ ${#wayland[@]} -gt 0 ]; then
    EXTRA_PLATFORM_PLUGINS="$(IFS=';'; echo "${wayland[*]}")"
    export EXTRA_PLATFORM_PLUGINS
fi

# plugin-qt does not deploy these: without graphics-integration GL cannot be created on Wayland (crash),
# without iconengines the SVG window and tray icons do not load; linuxdeploy collects their dependencies
mkdir -p "$APPDIR/usr/plugins"
for d in iconengines wayland-graphics-integration-client wayland-decoration-client wayland-shell-integration; do
    [ -d "$QTDIR/plugins/$d" ] && cp -r "$QTDIR/plugins/$d" "$APPDIR/usr/plugins/"
done

"linuxdeploy-$ARCH.AppImage" --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/materialfy" \
    --desktop-file "$APPDIR/usr/share/applications/materialfy.desktop" \
    --icon-file "$APPDIR/usr/share/icons/hicolor/scalable/apps/materialfy.svg" \
    --plugin qt

# appimagetool (since late Oct 2026) only takes a PNG for .DirIcon, not the SVG linuxdeploy links
cp "$ROOT/materialfy.png" "$APPDIR/materialfy.png"
ln -sfn materialfy.png "$APPDIR/.DirIcon"

TARBALL="$OUT/materialfy-$VERSION-linux-$ARCH.tar.gz"
tar -C "$OUT" --transform "s|^AppDir|materialfy-$VERSION|" -czf "$TARBALL" AppDir
echo "tar.gz: $TARBALL"

APPIMAGE="$OUT/Materialfy-$VERSION-$ARCH.AppImage"
rm -f "$APPIMAGE"
(cd "$OUT" && LDAI_OUTPUT="$APPIMAGE" OUTPUT="$APPIMAGE" \
    "linuxdeploy-$ARCH.AppImage" --appdir "$APPDIR" --output appimage)
[ -f "$APPIMAGE" ] || { echo "AppImage was not produced" >&2; exit 1; }
echo "AppImage: $APPIMAGE"
