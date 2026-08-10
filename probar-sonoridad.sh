#!/bin/bash
# Verifica el medidor de sonoridad contra las señales de EBU Tech 3341 sin
# abrir la aplicación ni arrastrar un decodificador: Sonoridad.swift es
# aritmética sobre muestras y Foundation basta para comprobarla.
set -euo pipefail
RAIZ="$(cd "$(dirname "$0")" && pwd)"
SALIDA="$RAIZ/build/pruebas"
mkdir -p "$SALIDA"
# El primer argumento, si llega, es el directorio con el set de la EBU: sin él
# corren solo las señales sintéticas.
swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/tests/sonoridad/main.swift" \
    -o "$SALIDA/pruebaSonoridad"
"$SALIDA/pruebaSonoridad" "$@"
