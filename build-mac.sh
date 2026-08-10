#!/bin/bash
set -euo pipefail

# Se guarda el argumento antes de nada: el bucle que genera los iconos usa
# `set -- ancho archivo`, y eso machaca los parámetros posicionales del script.
ACCION="${1:-}"

ROOT="$(cd "$(dirname "$0")" && pwd)"
APP="$ROOT/build/Editorcito.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

ICONSET="$ROOT/build/Editorcito.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for spec in "16 icon_16x16.png" "32 icon_16x16@2x.png" "32 icon_32x32.png" "64 icon_32x32@2x.png" "128 icon_128x128.png" "256 icon_128x128@2x.png" "256 icon_256x256.png" "512 icon_256x256@2x.png" "512 icon_512x512.png" "1024 icon_512x512@2x.png"; do
    set -- $spec
    sips -s format png -z "$1" "$1" "$ROOT/assets/Editorcito.svg" --out "$ICONSET/$2" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Editorcito.icns"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleName</key><string>Editorcito</string>
    <key>CFBundleDisplayName</key><string>Editorcito</string>
    <key>CFBundleExecutable</key><string>Editorcito</string>
    <key>CFBundleIdentifier</key><string>studio.editorcito.mac</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleIconFile</key><string>Editorcito.icns</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSpeechRecognitionUsageDescription</key><string>Editorcito usa el reconocimiento local para crear subtítulos del medio seleccionado.</string>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
    <key>CFBundleDocumentTypes</key><array><dict>
        <key>CFBundleTypeName</key><string>Audiovisual media</string>
        <key>CFBundleTypeRole</key><string>Editor</string>
        <key>LSItemContentTypes</key><array><string>public.audiovisual-content</string></array>
    </dict></array>
</dict></plist>
PLIST

swiftc -O \
    -target arm64-apple-macos14.0 \
    -parse-as-library \
    -framework AppKit \
    -framework AVFoundation \
    -framework AVKit \
    -framework Speech \
    -framework SwiftUI \
    -o "$APP/Contents/MacOS/Editorcito" \
    "$ROOT"/src/ui/*.swift

codesign --force --sign - "$APP" 2>/dev/null || true
echo "Editorcito listo: $APP"

# Instalación opcional en /Applications.
#
# Existe porque, si no, lo que abre Spotlight es la copia de la última vez que uno
# se acordó de copiarla a mano, y acabas probando una versión de hace días sin
# enterarte. `lsregister` refresca el registro para que aparezca al momento.
if [ "$ACCION" = "instalar" ]; then
    DESTINO="/Applications/Editorcito.app"
    pkill -x Editorcito 2>/dev/null || true
    rm -rf "$DESTINO"
    cp -R "$APP" "$DESTINO"
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f "$DESTINO" 2>/dev/null || true
    echo "Instalada en $DESTINO"
fi
