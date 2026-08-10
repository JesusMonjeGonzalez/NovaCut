# Estado de Editorcito — 8 de agosto de 2026 (noche)

Segunda tanda de nivel Premiere: **10 piezas más**, todas apoyadas en la
infraestructura existente. El arnés sigue con **16 grupos verdes** —el grupo
compositor-color ahora también prueba modos de fusión, máscaras (normal e
invertida), viñeta, desenfoque y curvas RGB sobre archivos reales.

## Cerrado hoy (noche)

### 1. Modos de fusión entre capas
- `ModoDeFusion` (14 modos: multiplicar, pantalla, superponer, aclarar,
  oscurecer, dodge, burn, luz fuerte/suave, diferencia, exclusión, color,
  luminosidad), cada uno con su filtro de Core Image.
- Los efectos de capa viajan en la instrucción indexados por trackID
  (`EfectosDeCapa`): el compositor mezcla con el filtro del modo en vez del
  `composited(over:)` de siempre, con la opacidad en el alfa.
- Verificado sobre archivos reales: rojo sobre azul en multiplicar da negro.

### 2. Máscaras (rectángulo y elipse, con pluma)
- `MascaraDeClip`: forma, posición y tamaño en fracciones del lienzo (sobrevive
  a cambios de orientación y resolución), pluma e invertida.
- Se dibujan como gradientes (`CIRoundedRectangleGenerator` + desenfoque para
  el rectángulo, `CIRadialGradient` para la elipse) y se aplican con
  `CIBlendWithMask`, que mezcla por luminancia. La invertida se construye
  componiendo la forma sobre negro y volteando el color —CIColorInvert a secas
  no toca el alfa y se pierde todo, verificado—.
- Verificado: centro rojo y esquina negra con la elipse; y al revés invertida.

### 3. Viñeta y desenfoque por clip
- `ColorDeClip.vignette`/`radioDeVignette` (CIVignette) y `desenfoque`
  (CIGaussianBlur), aplicados por capa antes de mezclar. Verificado: la viñeta
  oscurece el borde dejando el centro.

### 4. Curvas RGB
- `CurvasDeClip`: curva de luminancia + curva por canal, editables como puntos
  de control; el render las interpola a la tabla de `CIColorCurves`. La
  luminancia es la curva maestra **después** de cada canal —multiplicar por la
  luminancia rompía la identidad (x·x ≠ x), que fue el fallo del primer
  intento, cazado por el test de la tabla—.
- Editor visual en una hoja: lienzo con puntos arrastrables, doble clic añade,
  y la curva se aplica en vivo al monitor. Verificado: curva plana a 0,5 deja
  el rojo a media luz.

### 5. Formas básicas en los títulos
- `TituloDeClip.forma`: texto, rectángulo, elipse o línea (con relleno o
  contorno), con ancho/alto en fracciones del lienzo. Se dibujan con
  `CAShapeLayer` en la misma herramienta de quemado.

### 6. Presets de exportación ProRes y HEVC
- `MP4 · H.265/HEVC 1080p` (tamaño de archivo) y `ProRes 422` (master de
  calidad para intermedia), además de los H.264/vertical/audio/master.

### 7. Audio scrubbing
- Arrastrar el cabezal reproduce a 0,5× para oír el material por donde se
  pasa, como Premiere. Estado `scrubbing` publicado.

### 8. Zoom del monitor
- Botón de zoom en la barra del monitor (1× ajustado → 2× tamaño real), con
  escala animada.

### 9. Versiones de proyecto
- `⌘⌥S` «Guardar versión»: instantánea con fecha en `Versiones/` junto al
  proyecto, sin tocar el archivo de trabajo.

### 10. Automatización de volumen visible
- `CurvaDeGananciaView`: la curva de ganancia (y sus rampas de keyframes) se
  dibuja sobre los clips de audio en el timeline, como la línea de volumen de
  Premiere. Solo informativa: la edición sigue en el inspector.

## Verificación

- `./probar.sh`: 16 grupos verdes. El grupo compositor-color añade 7 pruebas
  nuevas sobre archivos reales (modo de fusión, dos máscaras, viñeta,
  desenfoque, curvas) y el grupo Timeline las pruebas de modelo (modos,
  máscaras, curvas, formas, viñeta).
- `./build-mac.sh`: compila la app completa (0 errores) e instalada en
  /Applications.

## Pendiente

- **Gate P0 (corpus VFR)**: sigue abierto — ~30 casos reales para
  `probar-corpus.sh`. Sin corpus no se dice «editor».
- **Humo manual**: todo lo nuevo — modos de fusión sobre dos clips, máscaras
  en el monitor, curvas arrastrando puntos, formas en títulos, ProRes/HEVC,
  audio scrubbing, zoom, versiones y la curva de ganancia en el timeline.
- **Audio**: buses de mezcla, sidechain selectivo, denoise, compresor
  multibanda, medidor en vivo — DSP pesado, fuera de esta tanda a propósito.
- **Infraestructura**: múltiples secuencias, smart bins, EDL/XML/FCPXML,
  historial de deshacer visual, proxies por clip — pendientes.
- **Core Rust**: congelado hasta el host Windows.

## Riesgos conocidos

- Las máscaras se evalúan en el render (Core Image por frame): con muchas
  máscaras y plumas grandes el preview puede pedir más CPU. Humo manual para
  medir.
- Los modos de fusión dependen de los filtros de mezcla de Core Image: la
  matemática exacta puede diferir un punto de la de Premiere (mismo espacio,
  mismo orden, implementación distinta).
- El audio scrubbing reproduce a 0,5× con la cadena de mezcla completa: en
  montajes con EQ/compresor puede notarse el coste.
- El zoom del monitor escala el frame ya compuesto: a 2× se ve el
  escalado, no un re-render a tamaño real.
