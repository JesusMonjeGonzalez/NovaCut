#!/bin/bash
# Verifica el camino completo de medición contra un archivo real: se monta el
# primer tramo del medio en una línea de tiempo, se lee la composición con su
# audioMix mediante AVAssetReader y se comprueba que la medida reproduce la del
# archivo original (y baja 6,02 dB cuando la mezcla baja 6,02).
#
#   ./probar-sonoridad-media.sh /ruta/al/video.mp4
#   ./probar-sonoridad-media.sh /ruta/a/senal.wav   (la envuelve en un .m4a)
set -euo pipefail
RAIZ="$(cd "$(dirname "$0")" && pwd)"
SALIDA="$RAIZ/build/pruebas"
mkdir -p "$SALIDA"

MEDIO="${1:-}"
if [ -z "$MEDIO" ]; then
    echo "Uso: $0 /ruta/al/medio (mp4, mov, m4a… o wav, que se envuelve en m4a)"
    exit 1
fi
if [[ "$MEDIO" == *.wav ]]; then
    MEDIO="$SALIDA/sonoridad-prueba.m4a"
    swiftc -O -target arm64-apple-macos14.0 \
        -framework AVFoundation -framework AudioToolbox \
        "$RAIZ/tests/sonoridad-media/crearMedio.swift" \
        -o "$SALIDA/crearMedio"
    "$SALIDA/crearMedio" "${1}" "$MEDIO"
fi

swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    -framework AVFoundation -framework AppKit \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Composicion.swift" \
    "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/SonoridadMedia.swift" \
    "$RAIZ/tests/sonoridad-media/main.swift" \
    -o "$SALIDA/pruebaSonoridadMedia"
"$SALIDA/pruebaSonoridadMedia" "$MEDIO"
