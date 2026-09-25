#!/bin/bash
# Empaqueta la app de macOS para una release: binario universal (Apple
# Silicon + Intel), nombre de producto NovaCut y zip listo para subir.
#
# La firma es ad hoc: sin cuenta de desarrollador de Apple no hay
# notarización, así que la primera apertura necesita «Abrir igualmente»
# (ver docs/INSTALAR.md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="$ROOT/build/release"

UNIVERSAL=1 "$ROOT/build-mac.sh"

rm -rf "$OUT"
mkdir -p "$OUT"
APP="$OUT/NovaCut.app"
cp -R "$ROOT/build/Editorcito.app" "$APP"
PLIST="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleName NovaCut" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName NovaCut" "$PLIST"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

lipo -archs "$APP/Contents/MacOS/Editorcito" | grep -q "x86_64 arm64" \
    || { echo "El binario no es universal" >&2; exit 1; }

ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT/NovaCut-macOS.zip"
echo "Listo: $OUT/NovaCut-macOS.zip ($(lipo -archs "$APP/Contents/MacOS/Editorcito"))"
