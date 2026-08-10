# ESPEC: P2 — Compositor completo, pistas de ajuste, LUT y vectorscopio (2026-08-07)

Fecha: 7 de agosto de 2026. Ámbito: app Swift `src/ui`.

## Fase A — Compositor completo (bug crítico + fundamento)

**Problema verificado por experimento** (`tests/experimento`): `CompositorDeColor`
toma solo `sourceTrackIDs.first`, aplica el color y devuelve ese frame. Con un
compositor custom activo, AVFoundation **no** compone las demás capas: opacidades,
transformaciones, recortes y capas inferiores desaparecen. Como
`customVideoCompositorClass` es global, **basta un clip con color en el montaje**
para que todos los segmentos pierdan sus capas. Verificado: dos vídeos superpuestos
al 50 % → sin compositor custom: magenta; con CompositorDeColor: azul puro.

**Solución**: `CompositorDeColor` pasa a componer de verdad, replicando lo que
hace el compositor nativo de AVFoundation:
- Para cada `layerInstruction` de la instrucción, de abajo arriba: `sourceFrame`,
  `transform(at:)`, `opacity(at:)` y `cropRectangle(at:)` en
  `request.compositionTime` (API existente de `AVVideoCompositionLayerInstruction`).
- Se aplican con Core Image (transformación, recorte, alfa) y se componen con
  `sourceOver`; luego la cadena de color (y el LUT de la fase C) sobre el
  compuesto; `finish(withComposedVideoFrame:)` en todos los caminos.
- Sin color, se compone igual (el compositor es global): la corrección cuesta lo
  que cuesta, y la regla de que reproducción y exportación coinciden manda.

**Verificación** (test permanente, patrón `tests/multicam`): `tests/compositor-color`
genera dos vídeos de color sólido y compara píxel a píxel el render **nativo** de
AVFoundation contra el del compositor completo, en cuatro casos: opacidad al 50 %,
transformación desplazada, recorte y cadena de color. Ejecutable
`probar-compositor-color.sh` + bloque en `probar.sh` si no requiere archivo previo
(lo genera solo).

## Fase B — Pistas de ajuste (adjustment layers)

**Problema**: Premiere/FCP/Resolve tienen pistas de ajuste para aplicar un look
global por tramo; Editorcito no.

**Solución**: `Clip.esAjuste: Bool = false` (Codable, opcional → proyectos viejos
siguen abriendo). Un clip de ajuste no tiene medio:
- El constructor lo salta en la inserción (sin pista, **sin** aviso crítico).
- En `construirInstrucciones`, si el clip visible superior del tramo es de ajuste,
  su color es el `colorDeTramo` — como ya funciona con el color del clip superior,
  y ahora el compositor lo aplica sobre el compuesto entero (fase A).
- UI: menú/atajo «Añadir pista de ajuste» (crea pista V y un clip de ajuste en el
  cabezal con el color actual), y el inspector/color del clip de ajuste funciona
  como el de cualquier clip. El clip de ajuste se dibuja en el timeline con
  etiqueta.

## Fase C — LUT `.cube` por clip y vectorscopio

**LUT**: `Clip.lutDeColor: String?` (ruta al `.cube`). El compositor aplica
`CIColorCube` después de la cadena primaria (el orden es parte del resultado y se
declara). `ParseadorDeCubes` (lógica pura): `LUT_3D_SIZE N`, dominio opcional,
tolerante a comentarios; se cachea por ruta (estática) y si el archivo falta se
avisa en el render (aviso de ajuste, no crítico). Cubierto por `tests/cubes`.

**Vectorscopio**: `Scope.swift` recibe un modo (forma de onda / vectorscopio) sobre
el mismo frame capturado. Cálculo puro: mapear RGB → (U,V) de la señal (R−Y, B−Y)
y volcar un gráfico de croma; `calcularVectorscopio` es función pura testeable en
`tests/scope`. El menú Ver alterna entre los dos instrumentos.

## Fase D — Mezclador: EQ, compresor y limiter

**Decisión de arquitectura (a confirmar)**: la vía de AudioUnits de Apple
(`NBandEQ`, `DynamicsProcessor`, `PeakLimiter`) no encaja con
`MTAudioProcessingTap` (procesa buffers en sitio, no con un contexto de render de
unidades). La alternativa que respeta la arquitectura y la cultura de la casa es
DSP propio en el tap, como el medidor EBU:
- `EcualizadorDePista`: EQ paramétrico de 4 bandas con secciones `Biquad` —la
  clase ya existe y está verificada en `Sonoridad.swift`—.
- `CompresorDePista`: envolvente + reducción de ganancia (umbral, ratio, ataque,
  soltura, rodilla).
- `LimitadorDePista`: protección de pico (techo, ataque/soltura).
- `Pista` gana `ecualizacion: [BandaDeEQ]?`, `compresor: CompresorDePista?`,
  `limiter: LimitadorDePista?` (opcionales, compatibilidad).
- El tap procesa en el orden EQ → compresor → limiter, el mismo código en
  reproducción y exportación (viaja en `mezclaDeAudio`), y el medidor mide el
  resultado (la regla reproducir = exportar = medir).
- Verificación con señales (`tests/mixer`): barrido senoidal por banda (el EQ
  responde donde debe), tono sobre umbral (el compresor reduce el ratio pedido),
  pico por encima del techo (el limiter lo corta). DSP puro, testable sin GUI.

## Fuera de alcance

- Smooth slow motion óptico, títulos Essential Graphics, traducción de subtítulos.
- Mezclador por buses y automatización (Fairlight): después del gate alpha.
- El orden de fase: A → C(LUT) → B (dependen del compositor) → D (independiente).

## Estado (7 de agosto de 2026, noche)

- **Fase A hecha**: compositor completo, verificado píxel a píxel contra el
  compositor nativo (mezcla, transformación, recorte, color sobre dos capas) en
  `tests/compositor-color` (12 comprobaciones). En el camino se arregló también
  el color del tramo, que cogía la pista **inferior** contradiciendo su propio
  comentario.
- **Fase B hecha**: `Clip.esAjuste`, el constructor salta los ajustes y su
  color/LUT manda sobre el compuesto; «Añadir pista de ajuste» en el menú de
  pistas; dibujo morado en el timeline. Verificado en `tests/compositor-color`.
- **Fase C (LUT) hecha**: `ParseadorDeCubes` (lógica pura, `tests/cubes`, 8
  comprobaciones) + `Clip.lutDeColor` + `CIColorCube` en el compositor tras la
  cadena primaria + aviso de ajuste si la LUT no existe. Verificado end-to-end
  (clip con LUT «todo a verde» sobre material real).
- **Fase C (vectorscopio) pendiente** y **Fase D (mixer DSP) pendiente**:
  siguientes bloques. El vectorscopio reutiliza el frame capturado de
  `CapturadorDeFrames`; el mixer (EQ 4 bandas con `Biquad`, compresor y limiter
  en el tap, con la regla reproducir = exportar = medir) es el bloque grande y
  se diseña con su propio spec cuando toque.
