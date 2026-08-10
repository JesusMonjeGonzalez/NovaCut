#!/bin/bash
# Prueba la multicámara sobre archivos reales: genera tres «cámaras» de prueba
# (color sólido + tono con pads distintos) y comprueba que el clip multicámara
# cambia de ángulo en los cortes —en vídeo y en audio— y que los desfases del
# grupo se respetan.
#
#   ./probar-multicam.sh   (los archivos se generan en build/pruebas/camaras)
set -euo pipefail
RAIZ="$(cd "$(dirname "$0")" && pwd)"
SALIDA="$RAIZ/build/pruebas"
CAMARAS="$SALIDA/camaras"
mkdir -p "$CAMARAS"

swiftc -O -target arm64-apple-macos14.0 \
    -framework AVFoundation -framework AudioToolbox -framework CoreVideo \
    "$RAIZ/tests/multicam/crearCamaras.swift" \
    -o "$SALIDA/crearCamaras"
"$SALIDA/crearCamaras" "$CAMARAS"

swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    -framework AVFoundation -framework AppKit -framework CoreGraphics \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" \
    "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" \
    "$RAIZ/src/ui/SonoridadMedia.swift" \
    "$RAIZ/tests/multicam/main.swift" \
    -o "$SALIDA/pruebaMulticam"
"$SALIDA/pruebaMulticam" "$CAMARAS"
