# ESPEC: Siete piezas de nivel Premiere (2026-08-08 tarde)

Fecha: 8 de agosto de 2026. Ámbito: modelo `src/ui/Timeline.swift`,
compositor `src/ui/Composicion.swift`, scopes `src/ui/Scope.swift`, app
`src/ui/App.swift`, `src/ui/Nidos.swift` (nuevo), proyecto `src/ui/Proyecto.swift`
y el arnés. No toca la arquitectura de reproducción ni la de exportación.

## 1. Títulos y lower thirds

- `TituloDeClip` (texto, posición X/Y en fracción del lienzo, tamaño, fuente,
  color, contorno, fundido) y `Clip.esTitulo`. El título no usa su medio.
- El constructor los trata como las pistas de ajuste (sin track) y los quema
  con la herramienta de CoreAnimation de los subtítulos (`quemaTitulo`), así
  que reproducir y exportar aplican el mismo código. Con solo títulos se añade
  una instrucción de fondo: AVFoundation exige al menos una.
- UI: `⌘⌥T` crea el clip en el cabezal; el inspector edita el texto y la forma.

## 2. Retime avanzado

- `RampaDeVelocidad` y `Clip.rampasDeVelocidad`: keyframes de velocidad con
  interpolación lineal. `duracionEnOrigen` es la integral trapezoidal con los
  extremos anclados. Velocidad 0 = freeze frame.
- `piezasDeVelocidad()` parte el clip en tramos con su consumo de origen; el
  render inserta cada tramo con su `scaleTimeRange`, y un congelado inserta un
  frame y lo estira. La marcha atrás sigue declarada no soportada.
- Verificación: modelo en `tests/main.swift` y render real en `tests/retime`
  (patrón H.264 de grises por escalones; se mide la luminancia decodificada).

## 3. Subclips

- `SubclipOrigen` (medio base + rango) en el modelo, `MediaItem.subclipDe` en
  la app y `MedioGuardado.subclip` en el proyecto. `clipDe` desplaza
  `entradaEnOrigen` al rango; `cambiarTimebase` convierte los rangos.
- UI: botón «Subclip» en el monitor de origen con I/O marcadas.

## 4. Clips anidados

- `Clip.nido` guarda la línea de tiempo interior. AVFoundation no anida
  composiciones (la interior perdería sus instrucciones), así que el nido se
  pre-renderiza a la caché (`ServicioDeNidos`, calidad master) y el clip usa
  ese archivo como medio. `⌘G` anida clips contiguos de una pista; desanidar
  devuelve los originales. Editar dentro del nido queda para la siguiente
  ronda (`reRenderizarNido` ya es el puente).

## 5. Parade RGB e histograma

- `ParadeRGB.calcular` (tres distribuciones por canal) y
  `HistogramaDeLuminancia.calcular`, con el mismo camino BGRA/vImage que los
  scopes existentes. Dos instrumentos nuevos en el menú Ver.

## 6. Transcripción al importar + cola cancelable

- Cola serial (`colaDeTranscripcion`, `procesarSiguienteTranscripcion`), con
  cancelación del trabajo en curso y de la cola entera. Un fallo no la para.
- Preferencia «Transcribir al importar»: los medios con vídeo entran solos.

## 7. Preview incremental

- `LineaDeTiempo.firmaDeComposicion`: estructura (pistas, clips, dónde/cuánto,
  multicámara, rampas, timebase), sin atributos.
- `ConstructorDeMontaje.construir(…, reutilizando:)`: con la misma firma,
  reutiliza las pistas de la composición anterior y solo rehace las
  instrucciones y la mezcla. `MontajeRenderizable` guarda firma, pistas
  compuestas y tracks de audio para ese reuso.

## Fuera de alcance

- Marcha atrás (velocidad negativa) y retime multicámara: declarados no
  soportados con aviso crítico.
- Editar dentro de un nido (la ronda siguiente lo cierra con re-render).
- Indicador de progreso por medio en la cola de transcripción (UX de detalle).

## Verificación

- `./probar.sh`: 16 grupos verdes (Timeline ampliado, Scope ampliado, Multicam,
  Retime nuevo sobre archivos reales).
- `./build-mac.sh`: compila la app completa (0 errores).
- Humo manual pendiente (títulos, rampas, subclips, nidos, scopes nuevos, cola
  de transcripción y preview incremental en un timeline grande).
