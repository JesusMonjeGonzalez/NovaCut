# ESPEC: Arreglos críticos de la app (2026-08-07)

Fecha: 7 de agosto de 2026.
Ámbito: app Swift `src/ui`. El core Rust no se toca (gated para el host Windows).

Origen: auditoría de 22 hallazgos; esta espec cubre los 8 críticos + infraestructura
mínima reutilizable (enfoque B). No se abordan los hallazgos MEDIA/BAJA.

## 1. Trabajo pesado fuera del hilo principal

**Problema**: `quitarSilencios` (App.swift:707-725) y la medición de `exportMovie`
(2134-2153) corren el bucle síncrono de `AVAssetReader` de `SonoridadMedia`
dentro de un `Task` que hereda el actor `@MainActor`. Congela la UI durante toda la
pasada de audio.

**Solución**:
- En `SonoridadMedia.swift`, el núcleo de la medición pasa a funciones que reciben
  `AVMutableComposition` + `AVMutableAudioMix?` + `CMTimeRange?` (objetos que cruzan
  hilos sin problema). Las funciones públicas actuales delegan en ellas.
- En `EditorState`, helper único:

  ```swift
  private func enSegundoPlano<Resultado: Sendable>(
      _ operacion: @escaping @Sendable () throws -> Resultado
  ) async throws -> Resultado {
      try await Task.detached(priority: .userInitiated) { try operacion() }.value
  }
  ```

- `quitarSilencios` y la medición previa a exportar construyen el render en el actor
  (barato) y solo el bucle de lectura se envía a `enSegundoPlano`; el resultado
  vuelve con `await MainActor.run` (patrón ya usado por `refrescarFormaDeOnda`).
- La closure `@Sendable` captura `render.composicion` y `render.mezclaDeAudio` por
  separado, no el struct `MontajeRenderizable`, para no depender de la no-estrictez
  de Swift 5.

## 2. Frame real del monitor: forma de onda y miniatura de la IA

**Problema**: `refrescarFormaDeOnda` (1414-1418) y `miniaturaDelCabezal`
(2391-2393) exigen `player.currentItem.asset as? AVURLAsset`; el reproductor siempre
instala `AVPlayerItem(asset: render.composicion)` — un `AVMutableComposition` — así
que el cast falla siempre y ambas features están muertas.

**Solución**:
- `EditorState` guarda el último render (`private var ultimoRender: MontajeRenderizable?`),
  fijado en `rebuildPreview`.
- Helper compartido en `Composicion.swift` (`enum CapturadorDeFrames`):

  ```swift
  static func capturarFrame(
      de composicion: AVComposition,
      videoComposition: AVMutableVideoComposition?,
      en segundo: Double
  ) async -> CGImage?
  ```

  con `AVAssetImageGenerator` + `generator.videoComposition` — produce el frame tal y
  como se ve en el monitor (el compositor custom se aplica igual que en el reproductor).
  Fija `requestedTimeToleranceBefore/After = .zero`: la tolerancia por defecto (~1 s)
  haría que la forma de onda mirara a un fotograma distinto del que se ve.
- `refrescarFormaDeOnda` y `miniaturaDelCabezal` usan `ultimoRender`; con timeline
  vacío devuelven `nil` sin cambio de comportamiento.
- Nota honesta: la miniatura no lleva subtítulos quemados (el image generator no
  renderiza `animationTool`); el color, encuadre y capas sí van.

## 3. Registro de taps de paneo sin limpieza

**Problema**: `TapDePaneo.registro` (Composicion.swift:329-381) crece sin límite: el
callback `finalize` del tap es `nil`, así que nada se elimina jamás.

**Solución**: implementar `finalize` para que borre la entrada del registro bajo el
cerrojo (`registro[tap] = nil`). AVFoundation lo invoca al liberarse el `audioMix`
(cada `rebuildPreview` reemplaza el item), así el registro se auto-limpia.

## 4. Contrato `finish` del compositor garantizado

**Problema**: `CompositorDeColor.startRequest` (198-217) tiene dos `return` sin
`finish(...)`; un solo frame devuelto así cuelga la exportación para siempre.

**Solución**: todos los caminos terminan en `finish`:
- Instrucción no `InstruccionConColor` → passthrough del primer frame de origen, o
  `finish(withError:)` si no hay.
- Sin frame de origen → `finish(withError:)`.
- `newPixelBuffer` nil → `finish(withError:)`.
- `enum CompositorError: Error` (`sinOrigen`, `sinFrame`, `sinBuffer`) con descripción
  localizada. Color neutro → passthrough, como hoy.

## 5. Herramienta Mano (H)

**Problema**: `.mano` cae en el `default` de `gestoDelCuerpo` (VistaMontaje.swift:
602-621) y mueve clips como la Selección.

**Solución**: la herramienta Mano panea el timeline con el mismo mecanismo de
anclas que usa el playhead: una vista ancla invisible (`mano-anchor`) cuya x cambia
con el arrastre y `lector.scrollTo` la centra, **sin animación** (un scroll animado
haría el paneo elástico). Para que funcione tanto sobre clips como en espacio vacío:
- El caso `.mano` dentro del gesto del clip (que ya recibe el `ScrollViewProxy` como
  parámetro, vía `filaDePista`) hace el paneo y **no** entra en `beginTrim`/`endTrim`
  ni mueve el clip; el clic sin arrastre no selecciona ni hace seek.
- El lienzo lleva un gesto condicional (`DragGesture(minimumDistance: 1)`) que solo
  existe con la Mano activa para el espacio vacío; nunca compite con los clips
  porque el gesto de la vista hija gana sobre el del contenedor.
- Cursor `openHand` al hover y `closedHand` durante el arrastre.
- El resto de herramientas no cambia.

## 6. Informe de avisos hasta la exportación

**Problema**: el constructor recoge `avisos` pero la cola de exportación los ignora:
un medio offline desaparece del render con solo un aviso y el archivo sale con huecos
negros sin avisar.

**Solución**:
- `avisos` de `MontajeRenderizable` pasa a `[AvisoDeMontaje]`:
  `struct AvisoDeMontaje: Equatable { let mensaje: String; let critico: Bool }`.
  - Crítico (cambia el resultado respecto al montaje): medio sin archivo, error de
    montaje, retime multicámara no soportado, ángulo sin material.
  - De ajuste (informativo): metraje recortado por fin de archivo.
- En `procesarSiguienteExportacion`: si hay críticos → `NSAlert` listándolos con
  "Exportar de todas formas" / "Cancelar este trabajo" (cancelar retira el trabajo
  de la cola). Los de ajuste se avisan en el status de finalización.
- `rebuildPreview` sigue mostrando el primer aviso en el status (adaptado al tipo).
- Consumidores a adaptar (ya mapeados): `App.swift:2072` (`avisos.first`),
  `tests/multicam/main.swift:112-113,171` (`.joined` y `.contains` sobre mensaje),
  `tests/composicion/main.swift:55` (`.joined`), `tests/sonoridad-media/main.swift:
  224,250,279` (`avisos: []` — sigue compilando por inferencia de tipo). Nota: los
  grupos `multicam` y `sonoridad-media` no están enganchados en `probar.sh`
  (inconsistencia preexistente, fuera de alcance).

## 7. Cancelación real de la IA

**Problema**: `cancelAI()` cancela la `Task` pero el request de `URLSession.data(for:)`
sigue volando hasta el timeout; la UI se queda "Planificando…" pegada y el catch de
la petición vieja puede pisar el estado de una nueva.

**Solución**:
- En `AITransport.complete`: sesión `URLSession` efímera por petición envuelta en
  `withTaskCancellationHandler`; al cancelar, `session.invalidateAndCancel()` aborta
  el request en vuelo y `URLError.cancelled` se traduce explícitamente a
  `CancellationError` (para que el catch de `editWithAI` lo trate como cancelación).
- `cancelAI()` despega la UI al momento (`aiWorking = false`, estado "Cancelado").
- Contador `aiGeneracion`: cada `editWithAI` lo incrementa; el catch solo toca
  `aiResult`/`aiWorking` si la generación coincide — la petición vieja no pisa a la
  nueva.

## 8. Descartar cambios respeta la decisión

**Problema**: "No guardar" al cerrar no borra el autosave; el montaje rechazado
reaparece al arrancar (solo `saveProject` lo elimina).

**Solución**: en `windowShouldClose`, el caso "No guardar" elimina `autosaveURL`
antes de devolver `true`. Es la única vía de salida verificada: la app no registra
delegate de terminación propio (sin `applicationShouldTerminate`/`willTerminate`),
y Cmd+Q también pasa por `windowShouldClose`.

## Verificación

- Nuevo grupo `tests/avisos/main.swift` (patrón de `tests/color`): línea de tiempo con
  medios ausentes → `construir` produce avisos críticos y la clasificación es correcta;
  se engancha en `probar.sh`.
- Gates: `./probar.sh` entero verde (214 + nuevas), `./build-mac.sh` compila, y humo
  manual: herramienta Mano, forma de onda del monitor, miniatura en una petición de IA,
  exportación con un medio offline (debe preguntar), cancelar una petición de IA.

## Fuera de alcance

- Hallazgos MEDIA y BAJA (preview incremental, modelo de comandos, keyframes de
  ganancia en mezcla, LFE 5.1, reframe 30 fps, etc.).
- Barra de progreso de la medición de sonoridad.
- Core Rust.
