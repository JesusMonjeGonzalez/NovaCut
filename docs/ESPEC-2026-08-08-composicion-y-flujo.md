# ESPEC: Segunda tanda Premiere — composición y flujo (2026-08-08 noche)

Fecha: 8 de agosto de 2026. Ámbito: modelo `src/ui/Timeline.swift`,
compositor `src/ui/Composicion.swift`, app `src/ui/App.swift`, exportación
`src/ui/Exportacion.swift`, timeline `src/ui/VistaMontaje.swift`. Diez piezas
apoyadas en la infraestructura existente; ninguna toca la arquitectura de
reproducción.

## 1. Modos de fusión

- `ModoDeFusion` (14 modos) con su filtro de Core Image. `Clip.modoDeFusion`.
- `InstruccionConColor.efectosPorCapa`: dict de `EfectosDeCapa` indexado por
  trackID (modo, máscara, viñeta, desenfoque). El compositor mezcla con el
  filtro del modo en vez de `composited(over:)`, con la opacidad en el alfa.
- El atajo de passthrough de una sola capa se desactiva cuando la capa tiene
  efectos, y el compositor custom se activa con efectos (antes solo con color
  o LUT).

## 2. Máscaras

- `MascaraDeClip`: forma (rectángulo/elipse), posición y tamaño en fracciones
  del lienzo (sobreviven a orientación y resolución), pluma e invertida.
- El rectángulo usa `CIRoundedRectangleGenerator` con desenfoque de pluma; la
  elipse, `CIRadialGradient` con radios de pluma. `CIBlendWithMask` mezcla por
  luminancia; la invertida compone la forma sobre negro y voltea el color
  (CIColorInvert a secas no toca el alfa y perdía todo el recorte, verificado
  y corregido).

## 3. Viñeta y desenfoque

- `ColorDeClip.vignette`/`radioDeVignette` (CIVignette) y `desenfoque`
  (CIGaussianBlur), por capa, antes de mezclar. `tieneAjustes` los incluye.

## 4. Curvas RGB

- `CurvasDeClip`: puntos de control por canal + luminancia; la tabla se
  interpola a los 256×3 floats de `CIColorCurves`. La luminancia es la curva
  maestra **después** de cada canal (multiplicar rompía la identidad: el test
  de la tabla lo cazó).
- Editor en hoja: lienzo con puntos arrastrables, doble clic añade, se aplica
  en vivo al monitor (el monitor de programa es la prueba de que el color
  coincide con la exportación).

## 5. Formas en títulos

- `TituloDeClip.forma` (texto/rectángulo/elipse/línea) con ancho/alto en
  fracciones; `CAShapeLayer` en la herramienta de quemado de títulos.

## 6. Presets ProRes y HEVC

- `PresetExportacion.hevc` (H.265) y `.prores` (ProRes 422 LPCM), con sus
  tipos de archivo; el diálogo de exportación ofrece los seis presets.

## 7. Audio scrubbing

- El `Slider` del programa notifica inicio/fin de arrastre; durante el arrastre
  la reproducción va a 0,5× para oír el material (Premiere hace lo mismo).

## 8. Zoom del monitor

- `zoomDeMonitor` publicado y botón en la barra (1× → 2×), escala animada.

## 9. Versiones de proyecto

- `guardarVersion()`: instantánea con fecha en `Versiones/` junto al proyecto,
  `⌘⌥S`, sin tocar el archivo de trabajo.

## 10. Automatización de volumen visible

- `CurvaDeGananciaView`: la curva de ganancia (y sus rampas de keyframes) se
  dibuja sobre los clips de audio. Solo informativa.

## Fuera de alcance (declarado)

- Buses de mezcla, sidechain selectivo, denoise, compresor multibanda y medidor
  en vivo: DSP pesado, pendiente.
- Múltiples secuencias, smart bins, EDL/XML/FCPXML, historial de deshacer
  visual, proxies por clip: pendientes.
- Editar dentro de un nido: pendiente (reRenderizarNido es el puente).

## Verificación

- `./probar.sh`: 16 grupos verdes. El grupo compositor-color añade siete
  pruebas sobre archivos reales (multiplicar → negro, máscara elíptica,
  invertida, viñeta, desenfoque, curva plana) y el grupo Timeline las pruebas
  de modelo (modos, máscaras, curvas, formas, viñeta/desenfoque).
- `./build-mac.sh`: compila la app completa (0 errores) e instalada.
- Humo manual pendiente: modos sobre dos clips reales, máscaras en el monitor,
  curvas arrastrando, formas en títulos, ProRes/HEVC, scrubbing, zoom,
  versiones y la curva de ganancia.
