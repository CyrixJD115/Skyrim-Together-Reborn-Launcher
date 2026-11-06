#!/usr/bin/env bash
# Packages the Linux build into a portable AppImage.
#
# Usage: scripts/make-appimage.sh [output-dir]
#   Builds the release binary, assembles an AppDir (binary, .desktop,
#   hicolor icons) and runs linuxdeploy + its appimage plugin to produce
#   dist/STR-Launcher.AppImage. The tools are downloaded once into
#   target/appimage-tools/ (set APPIMAGETOOLS_DIR to override).
set -euo pipefail
cd "$(dirname "$0")/.."

OUT_DIR="${1:-dist}"
TOOLS_DIR="${APPIMAGETOOLS_DIR:-target/appimage-tools}"
ARCH="${APPIMAGE_ARCH:-x86_64}"
EXE="$ARCH" # linuxdeploy appends "-$ARCH" to the output name

mkdir -p "$TOOLS_DIR"
LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-$ARCH.AppImage"
APPIMAGETOOL="$TOOLS_DIR/appimagetool-$ARCH.AppImage"
fetch() { # fetch <url> <dest>
    [ -x "$2" ] && return 0
    echo "downloading $(basename "$2")…"
    curl -fL --retry 3 -o "$2" "$1"
    chmod +x "$2"
}
fetch "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage" "$LINUXDEPLOY"
fetch "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage" "$APPIMAGETOOL"

echo "building release binary…"
cargo build --release

APPDIR="$PWD/target/appdir/STR-Launcher.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
         "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
         "$APPDIR/usr/share/metainfo"

cp target/release/str-launcher "$APPDIR/usr/bin/"
cp assets/io.github.cyrixjd.str-launcher.desktop "$APPDIR/usr/share/applications/"
cp assets/icon-512.png "$APPDIR/usr/share/icons/hicolor/512x512/apps/str-launcher.png"
cp assets/icon.png     "$APPDIR/usr/share/icons/hicolor/256x256/apps/str-launcher.png"
cat > "$APPDIR/usr/share/metainfo/io.github.cyrixjd.str-launcher.appdata.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>io.github.cyrixjd.str-launcher</id>
  <name>STR Launcher</name>
  <summary>One-click installer and launcher for Skyrim Together Reborn</summary>
  <metadata_license>MIT</metadata_license>
  <project_license>MIT</project_license>
  <url type="homepage">https://github.com/CyrixJD115/Skyrim-Together-Reborn-Launcher</url>
  <launchable type="desktop-id">io.github.cyrixjd.str-launcher.desktop</launchable>
  <description><p>Finds your Skyrim Special Edition install, downloads and installs Skyrim Together Reborn with all prerequisites, and launches you into a multiplayer session — on Windows and Linux.</p></description>
  <releases><release version="0.1.0" date="2026-11-06"/></releases>
</component>
EOF

# Bundles non-base shared libs (wayland/xkbcommon/…) next to the binary and
# sets RPATH so the AppImage runs on any distro; GL stays on the system
# (it is driver-specific and must not be bundled).
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    -e "$APPDIR/usr/bin/str-launcher" \
    -d "$APPDIR/usr/share/applications/io.github.cyrixjd.str-launcher.desktop" \
    -i "$APPDIR/usr/share/icons/hicolor/512x512/apps/str-launcher.png" \
    --exclude-library "libGL.so*" \
    --exclude-library "libEGL.so*" \
    --exclude-library "libgallium*"

# Build the AppImage ourselves with an explicit output path (the linuxdeploy
# appimage plugin runs appimagetool from its own temp CWD and the output can
# be lost there). Needs FUSE (libfuse2) — e.g. `apt-get install libfuse2` on CI.
mkdir -p "$OUT_DIR"
"$APPIMAGETOOL" "$APPDIR" "$PWD/$OUT_DIR/STR-Launcher.AppImage" 1>&2
echo "done: $OUT_DIR/STR-Launcher.AppImage"
