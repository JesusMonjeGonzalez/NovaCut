# Estado de Editorcito — 8 de agosto de 2026 (tarde)

Siete piezas de nivel Premiere cerradas en una sola tanda: títulos (lower
thirds), retime avanzado con rampas y freeze frame, subclips, clips anidados,
parade RGB + histograma, transcripción al importar con cola cancelable y
preview incremental. El arnés pasa **16 grupos de `./probar.sh`** (el retime se
enganchó como grupo nuevo sobre archivos reales generados).

## Cerrado hoy (tarde)

### 1. Títulos y lower thirds
- `TituloDeClip` en el modelo (texto, posición en el lienzo, tamaño, fuente,
  color, contorno, fundido) y `Clip.esTitulo`: un clip que se quema sobre el
  vídeo en su tramo, como los subtítulos, con la misma herramienta de
  CoreAnimation — reproducir y exportar aplican el mismo código.
- `⌘⌥T` (Montaje › Añadir título) crea el clip en la pista de trabajo en el
  cabezal; el inspector edita todo, con ColorPicker.
- El constructor trata los títulos como las pistas de ajuste (sin medio) y
  añade una instrucción de fondo cuando solo hay títulos, porque AVFoundation
  exige al menos una instrucción.

### 2. Retime avanzado: rampas y freeze frame
- `RampaDeVelocidad` y `Clip.rampasDeVelocidad`: la velocidad interpola
  linealmente entre keyframes; `duracionEnOrigen` es la integral trapezoidal
  (los extremos del clip están anclados, que es donde el primer intento perdía
  la rampa final). Velocidad 0 en un tramo = freeze frame.
- El render parte el clip en piezas (`piezasDeVelocidad`, una por intervalo
  entre rampas, con su consumo de origen) e inserta cada una con su
  `scaleTimeRange`; un congelado inserta un solo frame y lo estira al tramo.
- UI: «Rampa en el cabezal» (50 %) y «Congelar desde el cabezal» (0 %), con la
  lista de rampas editable en el inspector.
- Verificado en el modelo y **sobre archivos reales**: `tests/retime` genera un
  patrón H.264 de grises por escalones (robusto a la cuantización del codec,
  que fue el primer falso negativo de la prueba) y comprueba 1×, 2×, rampa
  1→3× y congelado leyendo la luminancia decodificada.

### 3. Subclips
- `SubclipOrigen` en el modelo (medio base + rango) y `MediaItem.subclipDe`:
  el recorte entra en la biblioteca como un medio con nombre, sin copiar el
  archivo. `clipDe` desplaza `entradaEnOrigen` al rango del subclip.
- Botón «Subclip» en el monitor de origen cuando hay I/O marcadas. Persiste en
  el proyecto (MedioGuardado.subclip) y se convierte al cambiar la base de
  tiempo, como los desfases multicámara.

### 4. Clips anidados (nesting)
- `Clip.nido` guarda la línea de tiempo interior, como la secuencia dentro de
  la secuencia de Premiere. AVFoundation no puede anidar composiciones (la
  interior perdería sus instrucciones), así que el nido se **pre-renderiza a la
  caché** (ServicioDeNidos, calidad master) y el clip usa ese archivo como su
  medio —el mismo camino honesto que los proxies—.
- `⌘G` anida los clips seleccionados contiguos de una pista; el clip resultante
  se mueve, recorta y retime como una pieza. «Desanidar» devuelve los clips
  originales intactos.

### 5. Parade RGB e histograma
- `ParadeRGB` (tres formas de onda por canal) y `HistogramaDeLuminancia`
  (distribución global), con vImage/BGRA como los scopes existentes.
- Nuevos instrumentos del monitor («Parade RGB», «Histograma») en el menú Ver.
- Verificado en `tests/scope` con buffers sintéticos: un rojo puro quema solo
  su canal, el gris 50 % cae en su nivel y el histograma suma 1.

### 6. Transcripción al importar + cola cancelable
- Cola serial de transcripciones: `encolarTranscripcion`, un trabajo detrás de
  otro (el reconocimiento on-device es caro), con cancelación del trabajo en
  curso y de la cola entera.
- Preferencia «Transcribir al importar» en el menú Ver: los medios nuevos con
  vídeo entran solos en la cola. Un fallo no para la cola.

### 7. Preview incremental
- `LineaDeTiempo.firmaDeComposicion`: firma estructural (pistas, clips, dónde y
  cuánto, multicámara, rampas, timebase) **sin** los atributos (color,
  ganancia, keyframes, mezcla).
- `ConstructorDeMontaje.construir(…, reutilizando:)`: si la firma no cambió,
  reutiliza las pistas de la composición anterior y solo rehace las
  instrucciones de vídeo y la mezcla —mover el deslizador de ganancia o el
  color deja de reconstruir los tracks de doscientos clips—.
- Verificado en el modelo: ganancia/color no tocan la firma; duración, clip
  nuevo o rampas sí.

## Verificación

- `./probar.sh`: 16 grupos verdes (Timeline ampliado con rampas, títulos,
  subclips y firma; Scope ampliado con parade e histograma; Multicam; Retime
  nuevo sobre archivos reales).
- `./build-mac.sh`: compila la app completa (0 errores) e instalada en
  /Applications.

## Pendiente

- **Gate P0 (corpus VFR)**: sigue abierto — ~30 casos reales para
  `probar-corpus.sh`. Sin corpus no se dice «editor» en ningún sitio.
- **Humo manual**: títulos, rampas y congelados en pantalla, subclips, nidos
  (crear/desanidar), parade e histograma, la cola de transcripción y el preview
  incremental en un timeline grande.
- **Transcripción**: falta el indicador de progreso por medio en la biblioteca
  (la cola funciona, la UX de detalle es humo manual).
- **Nidos**: editar dentro del nido (abrir el interior, re-renderizar al
  cerrar) queda como puente —`reRenderizarNido` ya existe—; hoy el camino es
  desanidar, editar y re-anidar.
- **Pequeños**: exportación atómica (`.part`), pegar atributos en un solo
  commit de undo, force unwraps en `Pista` y `vistaClip`.
- **Core Rust**: congelado hasta el host Windows.

## Riesgos conocidos

- La gestión de color del compositor sigue sin conversión de espacio
  (declarado, no oculto).
- El retime con rampas usa interpolación lineal entre keyframes y tramos
  `scaleTimeRange`: es el mismo modelo que el resto del montaje, pero las
  rampas de velocidad con transiciones cruzadas no se han probado juntas.
- Los títulos se queman con CoreAnimation como los subtítulos: comparten sus
  límites (posicionamiento simple, sin animación de entrada además del fundido).
- El nido pre-renderizado es una instantánea: cambiar el interior requiere
  desanidar/re-anidar o el re-render manual.
