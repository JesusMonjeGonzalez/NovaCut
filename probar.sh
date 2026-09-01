#!/bin/bash
# Prueba el modelo de montaje sin abrir la aplicación.
#
# Se compila como un ejecutable suelto en vez de montar un target de pruebas:
# el modelo no depende de la interfaz, así que verificarlo no debería costar más
# que un swiftc. `-enforce-exclusivity=checked` está a propósito, porque las
# operaciones de edición tocan `pistas[i]` desde métodos que mutan el montaje y
# ahí es donde un acceso mal planteado revienta en tiempo de ejecución.
#
# Mezclador.swift y Sonoridad.swift viajan en casi todos los grupos: el modelo
# (Timeline) referencia los parámetros de la cadena de mezcla, y la cadena usa
# el `Biquad` del medidor.
set -euo pipefail
RAIZ="$(cd "$(dirname "$0")" && pwd)"
SALIDA="$RAIZ/build/pruebas"
mkdir -p "$SALIDA"
swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/tests/main.swift" \
    -o "$SALIDA/pruebaTimeline"
"$SALIDA/pruebaTimeline"

# Edición por transcript: el mapa entre lo que se dice (tiempo del medio) y lo que
# se oye (frames de montaje), y el borrado de varios tramos, que hay que aplicar de
# atrás hacia delante o se come material que nadie seleccionó.
swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/tests/transcript/main.swift" \
    -o "$SALIDA/pruebaTranscript"
"$SALIDA/pruebaTranscript"

# Detección de silencios: umbral relativo, histéresis y guarda a los lados. Son las
# tres cosas que separan un rough cut usable de uno que se come las consonantes.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Silencios.swift" "$RAIZ/tests/silencios/main.swift" \
    -o "$SALIDA/pruebaSilencios"
"$SALIDA/pruebaSilencios"

# Atributos de clip: qué viaja en un copiar/pegar de atributos (transformación,
# ganancia, fundidos) y el criterio de borde del extend edit y el match frame.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/tests/atributos/main.swift" \
    -o "$SALIDA/pruebaAtributos"
"$SALIDA/pruebaAtributos"

# Corrección de color: la cadena de filtros del compositor es lógica pura de
# Core Image y se prueba sin abrir la aplicación.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/tests/color/main.swift" \
    -o "$SALIDA/pruebaColor"
"$SALIDA/pruebaColor"

# Reencuadre vertical: suavizado de la trayectoria y conversión a keyframes
# editables. La detección de Vision no se prueba aquí (necesita un vídeo real),
# pero el volcado de muestras a keyframes sí es lógica pura.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/src/ui/Reframe.swift" "$RAIZ/tests/reframe/main.swift" \
    -o "$SALIDA/pruebaReframe"
"$SALIDA/pruebaReframe"

# Paneo de pista: la ley de balance y que el tap viaja en el parámetro de mezcla,
# que es lo que hace que reproducción y exportación apliquen el mismo código.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/tests/paneo/main.swift" \
    -o "$SALIDA/pruebaPaneo"
"$SALIDA/pruebaPaneo"

# Informe de avisos del constructor: qué es crítico (medio offline, retime o
# ángulo multicámara sin soporte) y qué llega a la exportación para preguntar.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/tests/avisos/main.swift" \
    -o "$SALIDA/pruebaAvisos"
"$SALIDA/pruebaAvisos"

# Pesos BS.1770 por disposición real de canales: el LFE no contribuye y el
# surround va a +1,5 dB, pero en qué índice está cada uno solo lo sabe el layout.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/src/ui/SonoridadMedia.swift" "$RAIZ/tests/layout/main.swift" \
    -o "$SALIDA/pruebaLayout"
"$SALIDA/pruebaLayout"

# Compositor de color: genera dos vídeos de color sólido y compara píxel a
# píxel el compositor custom contra el nativo (mezcla, transformación, recorte
# y color sobre dos capas). Existe porque el compositor viejo perdía las capas
# inferiores con cualquier clip de color en el montaje.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/tests/compositor-color/main.swift" \
    -o "$SALIDA/pruebaCompositorColor"
"$SALIDA/pruebaCompositorColor"

# LUTs `.cube`: el parseador es lógica pura — tamaño, dominio, comentarios y la
# expansión de LUTs 1D al cubo que espera Core Image.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/tests/cubes/main.swift" \
    -o "$SALIDA/pruebaCubes"
"$SALIDA/pruebaCubes"

# Vectorscopio: buffers BGRA sintéticos de color sólido deben caer en los bins
# que predicen las ecuaciones de croma, y el gris en el centro.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Scope.swift" "$RAIZ/tests/scope/main.swift" \
    -o "$SALIDA/pruebaScope"
"$SALIDA/pruebaScope"

# Mezclador: EQ, compresor y limiter son DSP puro — el seno en la banda sube,
# el tono sobre el umbral se reduce al ratio, nada pasa del techo, y el paneo
# cierra la cadena.
swiftc -O -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" "$RAIZ/tests/mixer/main.swift" \
    -o "$SALIDA/pruebaMixer"
"$SALIDA/pruebaMixer"

# Multicámara sobre archivos reales: genera tres «cámaras» de prueba y comprueba
# que el clip multicámara cambia de ángulo en los cortes —en vídeo y en audio—
# y que los desfases del grupo se respetan. Era la inconsistencia preexistente
# que dejaba este grupo fuera del arnés.
if [ ! -x "$SALIDA/crearCamaras" ]; then
    swiftc -O -target arm64-apple-macos14.0 \
        -framework AVFoundation -framework AudioToolbox -framework CoreVideo \
        "$RAIZ/tests/multicam/crearCamaras.swift" \
        -o "$SALIDA/crearCamaras"
fi
mkdir -p "$SALIDA/camaras"
"$SALIDA/crearCamaras" "$SALIDA/camaras"
swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    -framework AVFoundation -framework AppKit -framework CoreGraphics \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" \
    "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" \
    "$RAIZ/src/ui/SonoridadMedia.swift" \
    "$RAIZ/tests/multicam/main.swift" \
    -o "$SALIDA/pruebaMulticam"
"$SALIDA/pruebaMulticam" "$SALIDA/camaras"

# Retime avanzado sobre un archivo real: un patrón de grises por escalones y
# tres comprobaciones —velocidad constante a 2×, rampa 1→3× y congelado— con
# la luminancia decodificada como prueba objetiva de cuánto material se ve.
swiftc -O -target arm64-apple-macos14.0 \
    -framework AVFoundation -framework CoreVideo \
    "$RAIZ/tests/retime/crearPatron.swift" \
    -o "$SALIDA/crearPatron"
mkdir -p "$SALIDA/retime"
"$SALIDA/crearPatron" "$SALIDA/retime"
swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    -framework AVFoundation -framework AppKit -framework CoreGraphics \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" \
    "$RAIZ/src/ui/Transcript.swift" "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/LUTs.swift" \
    "$RAIZ/tests/retime/main.swift" \
    -o "$SALIDA/pruebaRetime"
"$SALIDA/pruebaRetime" "$SALIDA/retime"

# Intercambio con otros editores: el EDL CMX 3600 y el FCPXML 1.11 son lógica
# pura sobre el modelo (timecodes exactos, enlaces A/V, velocidades y
# aplastado de capas), y se verifican sin abrir la aplicación.
swiftc -O -enforce-exclusivity=checked -target arm64-apple-macos14.0 \
    "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" \
    "$RAIZ/src/ui/Intercambio.swift" "$RAIZ/tests/intercambio/main.swift" \
    -o "$SALIDA/pruebaIntercambio"
"$SALIDA/pruebaIntercambio"

# Prueba de composición contra un archivo real, si se pasa uno:
#   ./probar.sh /ruta/al/video.mp4
# Verifica lo que ninguna prueba de modelo puede: que el montaje se convierte en
# algo que AVFoundation decodifica de verdad, con sus capas y su cadencia exacta.
if [ $# -ge 1 ]; then
    swiftc -O -target arm64-apple-macos14.0 \
        -framework AVFoundation -framework AppKit -framework CoreGraphics -framework CoreImage \
        "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" \
        "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/ConformadoVFR.swift" "$RAIZ/src/ui/LUTs.swift" \
        "$RAIZ/tests/composicion/main.swift" \
        -o "$SALIDA/pruebaComposicion"
    "$SALIDA/pruebaComposicion" "$1"

    # Conformado VFR: PTS constantes, duración A/V alineada y reutilización de la
    # caché. Se ejecuta solo con un archivo explícito para no convertir el arnés
    # general en una operación que necesite medios grandes.
    swiftc -O -target arm64-apple-macos14.0 \
        -framework AVFoundation -framework AppKit -framework CoreGraphics -framework CoreImage \
        "$RAIZ/src/ui/Timeline.swift" "$RAIZ/src/ui/Mezclador.swift" "$RAIZ/src/ui/Sonoridad.swift" "$RAIZ/src/ui/Transcript.swift" \
        "$RAIZ/src/ui/Composicion.swift" "$RAIZ/src/ui/ConformadoVFR.swift" "$RAIZ/src/ui/LUTs.swift" \
        "$RAIZ/tests/vfr/main.swift" \
        -o "$SALIDA/pruebaVFR"
    "$SALIDA/pruebaVFR" "$1"
fi
