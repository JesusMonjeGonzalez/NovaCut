# Editorcito: roadmap guiado por evidencia

La fuente estratégica es [DAFO-COMPETITIVO.md](DAFO-COMPETITIVO.md).

## Hecho el 4 de agosto de 2026

- **Modelo de montaje profesional** (`src/ui/Timeline.swift`): base de tiempo
  racional en frames enteros (23,976 es 24000/1001, no un decimal), timecode SMPTE
  con drop frame, multipista V/A, marcadores, etiquetas de color y enlace A/V.
- **Operaciones de edición completas**: insertar con arrastre, sobrescribir, partir,
  levantar, borrar con arrastre, cerrar huecos, mover entre pistas, y recorte
  normal / ripple / roll, más slip y slide. Imán a cortes, marcadores y cabezal.
- **Composición multipista** (`src/ui/Composicion.swift`): capas por pista con
  z-orden, opacidad, fundidos de entrada y salida, encuadre, recorte, retime,
  ganancia en dB, silencio, solo, ducking y crossfades. La orientación de cámara se aplica antes de
  encajar en el lienzo, que es lo que arregla mezclar vertical y horizontal.
- **Producción de medios**: monitor de origen independiente con entrada/salida,
  insertar y superponer; miniaturas, waveforms, búsqueda, bins, VFR detectado,
  proxies de preview y enlace A/V al mover o retirar segmentos.
- **Animación y captions**: keyframes de transformación/opacidad/ganancia,
  transiciones visibles en el timeline, importación/exportación SRT,
  transcripción on-device y captions quemados en el export.
- **Entrega**: presets MP4, vertical, M4A y master, rango de trabajo y cola de
  exportaciones serializada.
- `./probar.sh`: 78 comprobaciones del modelo sin abrir la aplicación, más una
  prueba del pipeline de composición contra un archivo real.

- **Interfaz enganchada al modelo nuevo**: vista multipista con cabeceras fijas,
  regla en timecode, siete herramientas con sus atajos de siempre (V, C, B, N, Y,
  U, H), imán conmutable, marcadores, recorte por arrastre en los dos bordes,
  menús contextuales e inspector completo del clip (opacidad, escala, giro,
  posición, ganancia, fundidos, velocidad y etiqueta).
- **Proyecto v2 portable**: guarda el montaje entero, resuelve los medios por ruta
  relativa a la carpeta del proyecto, y si un archivo no aparece el clip queda
  offline y revinculable en vez de invalidar el proyecto. Los `.editorcito` de la
  versión 1 se migran al abrirlos.
- **Asistente reescrito**: habla en timecode y conoce once órdenes de montaje
  (recortar, mover, quitar, quitar cerrando, cortar, silenciar, ganancia, fundido,
  velocidad, marcador y etiquetar) en lugar de tres.
- **Tamaño de la interfaz ajustable** con ⌘+, ⌘− y ⌘0, altura de pistas
  configurable, y divisores arrastrables entre monitor y montaje.
- **Calidad de vida**: selección múltiple de medios para multicámara, auto-scroll
  del playhead, click-to-seek en las pistas, atajos que respetan campos de texto,
  indicador de cambios sin guardar, confirmación al cerrar y etiquetas de
  accesibilidad en controles principales.

### Revisión de ajustables e interfaz

Auditada con pruebas, no de vista. Cinco defectos encontrados y corregidos:

1. **Los atajos no llegaban.** `keyboardShortcut` dentro de un `Menu` de la barra de
   herramientas se anuncia pero no se registra. Ahora hay barra de menús de verdad
   (`MenusDeEditorcito`) con Archivo, Ver y Montaje, y los atajos funcionan.
2. **La aplicación no tenía menús propios**: Archivo/Edición eran los vacíos que pone
   macOS. El estado vive ahora en la escena para que los menús puedan invocarlo.
3. **Ningún ajuste indicaba cuál estaba activo.** Los tamaños y la altura de pistas
   son selectores con marca; el botón muestra el porcentaje en curso.
4. **Seis tooltips en toda la aplicación.** Toda la barra superior y el transporte
   iban sin ayuda pese a ser botones de icono. Cubiertos, con su atajo en el texto.
5. **Zoom del montaje lineal de 3 a 200**, con todo el rango útil en el primer palmo
   del recorrido. Ahora es logarítmico, que es como se percibe el zoom.

Separado además lo que estaba mezclado: el tamaño de la interfaz es preferencia de
la aplicación y vive en Ver; la altura de las pistas es propiedad del montaje y
viaja con el proyecto. Añadido `⇧Z` —ajustar el montaje a la ventana, el atajo más
usado de cualquier NLE—, más `⌘]`/`⌘[` y `⌥↑`/`⌥↓`.

Limitación conocida: funciona `⌘=` pero no `⇧⌘=`. El menú anuncia `⌘=`, que es lo
que de verdad hace.

### Dos fallos que no se veían a simple vista

1. El lienzo se dibujaba **fuera del marco de la ventana** en pantalla completa,
   porque se le forzaba un tamaño mínimo mayor que el disponible y nada lo
   recortaba. El mínimo lo fija ahora la ventana y crece con el zoom.
2. La aplicación **abortaba** (`SIGABRT` en `_postWindowNeedsUpdateConstraints`) al
   escribir en el modelo desde dentro de un `GeometryReader`: eso ocurre en plena
   pasada de layout, y AppKit aborta el proceso si se le pide recalcular
   restricciones mientras las está calculando. El aviso se aplaza un ciclo.

Este bloque queda cerrado. Las features anteriores están implementadas en el host
macOS y verificadas con el pipeline real; no deben seguir apareciendo como trabajo
pendiente en documentación competitiva.

## Hecho el 11 de agosto de 2026

- **Corpus golden y arnés arreglado** (`generar-corpus.sh`, `probar-corpus.sh`):
  el generador se colgaba y dejaba un patrón de 0 bytes; el arnés contaba la
  reordenación de B-frames y los bloques PCM del muxer como huecos. Ahora siete
  patrones de claqueta (CFR, VFR real, vertical, HEVC, ProRes, H.264+AAC,
  audio solo) pasan sincronía ≤1,5 frames y cadencia sin huecos.
- **Detección VFR real** (`MedioResuelto.esCadenciaVFR`): scan de PTS en vez
  de la heurística de `minFrameDuration`, que no veía las grabaciones con
  caídas de frames. Las Screen Recording de tests/corpus ahora se detectan.
- **Intercambio profesional** (`src/ui/Intercambio.swift`): EDL CMX 3600
  (canales V/A1/A2, velocidades como M2, anotaciones `*`, columnas de 80) y
  FCPXML 1.11 (recursos, enlaces A/V, velocidades, aplastado con recorte de
  capas, `<note>` con limitaciones), con «Exportar EDL…» y «Exportar FCPXML…»
  en Archivo. 28 comprobaciones nuevas; `probar.sh` pasa 502.

## Hecho el 4 de septiembre de 2026

- **Conformado VFR macOS** (`src/ui/ConformadoVFR.swift`): writer H.264 con PTS
  CFR explícitos, audio original conservado, validación de caché y bloqueo cerrado
  de exportación/nidos si el intermediario no es íntegro. Verificado con el corpus
  golden y grabaciones reales locales.
- **Proyecto portable** (`src/ui/Proyecto.swift`): el arnés cubre migración v1,
  round-trip v2, rutas relativas, fallback por nombre y medios offline; `probar.sh`
  lo ejecuta y CI incluye el conformado VFR con audio.
- **Proxies cancelables** (`src/ui/Proxies.swift` y `src/ui/App.swift`): la
  generación se cancela de forma cooperativa y el menú limpia proxies huérfanos
  sin tocar los del proyecto actual.

## Hecho el 7 de septiembre de 2026

- **Banco de subtítulos en los dos hosts** (`src/ui/PanelDeSubtitulos.swift`,
  `src/core/subtitles.rs`): búsqueda literal sobre el texto de los cues y
  sincronización global por milisegundos o alineando el primero al cabezal. El
  desplazamiento se valida entero antes de aplicarse, así que un ajuste que dejaría
  cues antes del inicio se rechaza sin dejar el documento a medio resincronizar.
  macOS añade alta, edición, borrado y salto al cue, cada uno un solo paso de deshacer.
- **Presupuesto de la caché de proxies** (`src/ui/Proxies.swift`,
  `src/core/proxy_cache.rs`): límite de disco configurable con desalojo del menos
  usado recientemente, que se detiene en cuanto la caché cabe. Los proxies del
  proyecto abierto nunca se desalojan y el exceso se avisa. En Windows el barrido se
  limita al sufijo `-proxy.mp4`, para no tocar material del usuario en esa carpeta.
  Cierra el pendiente de límite automático de espacio.
- **Identidad del proxy atada al medio, no al clip**: en Windows el nombre era
  `{medio}-{índice}-proxy.mp4`, así que reordenar o borrar clips podía enlazar un
  proxy con otro vídeo. Ahora el nombre lleva la huella del medio (ruta, tamaño y
  fecha), dos clips del mismo archivo comparten proxy sin recodificar, y un medio
  sustituido en disco se marca `PROXY DESACTUALIZADO` en vez de mostrar contenido
  caducado. macOS cacheaba por UUID persistido en el proyecto y tenía el mismo
  defecto: el nombre lleva ahora la huella y la versión anterior se retira al
  regenerar. Los nombres antiguos se siguen leyendo, así que ningún proyecto
  guardado pierde su proxy.
- **Revinculación en lote** (`src/ui/Revinculacion.swift`, `src/core/relink.rs`): una
  carpeta y una sola pasada resuelven todos los medios offline. El desempate es
  determinista —tamaño, luego profundidad, luego orden alfabético— para que dos
  búsquedas sobre la misma carpeta no elijan archivos distintos. El recorrido tiene
  tope y lo avisa en vez de colgarse recorriendo un disco entero, y no sigue enlaces
  simbólicos. Avanza el pendiente de «media offline/relink» de la sección Ahora.
- **Importación de EDL** (`src/ui/Intercambio.swift`): el intercambio deja de ser de
  ida. Se leen cortes, timecodes de origen y montaje, canales V/A/B/AA, nombres y
  velocidad constante por `M2` o comentario. La verificación es la ida y vuelta contra
  el exportador, que es lo único que no depende de que yo haya entendido bien el
  formato. Los eventos solapados se reparten en pistas y los medios entran offline
  para resolverlos con la revinculación en lote. Falta el mismo trabajo en Windows.
- **Drop frame imposible corregido en el modelo**: `Timebase` aceptaba
  `dropFrame: true` a 25 fps y producía timecodes con `;` que no describen ningún
  reloj. Ahora solo se admite donde está definido, en las cadencias NTSC.
- **EDL en Windows, sobre el núcleo compartido** (`src/core/edl.rs`): el host de
  Windows exporta e importa CMX 3600. El formato vive en el core en frames, sin saber
  nada del modelo de cada host, así que la ida y vuelta se ejecuta en `cargo test --lib`
  en cualquier sistema; el mapeo proyecto↔EDL se prueba aparte en el host. Los cortes
  importados se reparten en pistas para no solaparse y lo que el formato no lleva se
  enumera al exportar.
- **CI que ejecutaba menos de lo que parecía**: el flujo solo compilaba en macOS. Ni las
  pruebas del núcleo en Rust ni las del host de Windows se ejecutaban en ningún sitio.
  Añadidos `cargo test --lib` al trabajo de macOS y un trabajo de Windows que compila y
  pasa las pruebas del host.
- **Drop frame imposible, también en el núcleo**: `Timebase::new` aceptaba drop frame
  fuera de NTSC, igual que el modelo de macOS. Corregido en los dos.
- `./probar.sh` pasa 653 comprobaciones; `cargo test --lib`, 52; el host de Windows suma
  tres pruebas propias que ahora sí se ejecutan en CI.

## Ahora

- Validar VFR, PTS y sincronización A/V con material legal y heterogéneo.
- Completar la semántica de proyecto portable, migraciones y media offline/relink.
- Medir seek, playback, proxies y exportación en proyectos largos.
- Poblar el corpus VFR (gate P0) y el humo manual tras cada ronda.
- Editar dentro de los clips anidados (abrir el interior y re-renderizar al cerrar).
- Buses de mezcla y envíos (otra arquitectura de tap), time-stretch/pitch,
  denoise espectral real.

## Hecho el 9 de agosto de 2026 (noche)

- DSP de audio pesado en la cadena de mezcla, en orden declarado y fijo
  (puerta → EQ → multibanda → compresor → limiter → reverb → retardo → paneo):
- Puerta de ruido con decisión por envolvente de pico (sin «chatter»).
- Compresor multibanda con cruces Linkwitz-Riley fijos (250 Hz / 4 kHz) y
  reconstrucción exacta por construcción.
- Retardo con realimentación y mezcla (anillo del tamaño exacto del tiempo).
- Reverb de Schroeder (peines + paso-todo, un anillo por peine).
- Mandos completos en el inspector del mezclador y presets en el menú
  contextual; todo Codable con el proyecto.

## Después del gate alpha

- Estabilización y seguimiento GPU más allá del reencuadre facial actual.
- Gestión completa del ciclo de vida y espacio de la caché de proxies.
- Corpus de codecs y benchmark reproducible antes de prometer paridad profesional.
- Pruebas UI de recuperación, revinculado y cancelación de exportaciones.

## Hecho el 8 de agosto de 2026 (tarde)

- Títulos y lower thirds (⌘⌥T), quemados con la misma herramienta que los subtítulos.
- Retime avanzado: rampas de velocidad interpoladas y freeze frame, verificados
  sobre archivos reales (tests/retime).
- Subclips desde el monitor de origen (I/O → «Subclip»), con rango persistente.
- Clips anidados (⌘G): secuencia interior pre-renderizada a la caché, con desanidar.
- Parade RGB e histograma de luminancia en el monitor de programa.
- Transcripción al importar con cola serial cancelable.
- Preview incremental: firma estructural del montaje, solo se rehacen las
  instrucciones y la mezcla cuando los atributos cambian.

## Hecho el 8 de agosto de 2026 (noche)

- 14 modos de fusión entre capas (multiplicar, pantalla, superponer, dodge,
  burn, diferencia, color…), aplicados por el compositor custom por capa.
- Máscaras de rectángulo y elipse con pluma e invertidas, en fracciones del
  lienzo (sobreviven a orientación y resolución).
- Viñeta y desenfoque gaussiano por clip.
- Curvas RGB con editor visual de puntos (luminancia maestra + canal por
  canal), aplicadas en vivo al monitor.
- Formas en los títulos: rectángulo, elipse y línea (relleno o contorno).
- Presets de exportación ProRes 422 y H.265/HEVC.
- Audio scrubbing al arrastrar el cabezal (0,5× con la cadena de mezcla).
- Zoom del monitor (1× ajustado → 2×).
- Versiones de proyecto (⌘⌥S → instantánea con fecha en Versiones/).
- Curva de ganancia dibujada sobre los clips de audio (automatización visible).

## Después del gate macOS

- Host Windows con Media Foundation y Direct3D 12.
- Paridad del formato de proyecto y operaciones esenciales.
- Paridad de captions, transcript y rough cut local entre hosts.

## No iniciar todavía

- HDR, plugins, colaboración, VFX 3D o generación de vídeo.
- Cualquier afirmación 4K/8K sin benchmark reproducible.
- “Alternativa a Premiere/Resolve” sin proyectos reales y gates superados.
