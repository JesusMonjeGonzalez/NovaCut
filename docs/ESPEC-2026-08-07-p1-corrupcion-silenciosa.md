# ESPEC: P1 — Corrupción silenciosa (2026-08-07)

Fecha: 7 de agosto de 2026. Ámbito: app Swift `src/ui`. Continúa el trabajo de
`ESPEC-2026-08-07-arreglos-criticos.md` (ya implementada). No toca la GUI: los 5
arreglos son lógica pura verificable con el arnés.

## 1. Cambiar la base de tiempo no convierte los desfases multicámara

**Problema**: `cambiarTimebase` (App.swift:1132) convierte clips, keyframes,
marcadores, rango de trabajo y subtítulos, pero no `GrupoMulticam.desfases`
(frames de proyecto). Tras cambiar de 25 a 60 fps, cada ángulo se inserta con su
desfase en la escala antigua y la sincronía multicámara se descuadra en silencio.

**Solución**: en el mismo `performEdit`, convertir los desfases con la función
`convertir` local (por tiempo, como todo lo demás).

## 2. Los keyframes de ganancia intermedios no suenan

**Problema**: `aplicarVolumen` (Composicion.swift) solo lee `gananciaEn(frame: 0)`
y `gananciaEn(frame: duracion)` como extremos de la rampa central; un keyframe de
ganancia en mitad del clip no genera ninguna `setVolumeRamp` — la animación no
suena, solo los bordes.

**Solución**: el tramo central se construye por piezas con borde en cada keyframe
dentro del tramo: `piezasDeGanancia(desde:hasta:nivelDePista:clip:)` recorre los
bordes y crea una rampa por segmento con `gananciaEn` en cada extremo (la misma
interpolación lineal que usa el modelo, así la envolvente coincide). `piezasCentrales`
recibe esas piezas y dibuja el ducking encima, sin cambiar su lógica ni el nivel
de hundimiento (sigue siendo el del arranque del clip).

## 3. El LFE recibe peso de surround en 5.1

**Problema**: `MedidorDeSonoridad` sin `pesos` asume orden 5.0 por conteo
(Sonoridad.swift:219): en un WAV 5.1 (L R C LFE Ls Rs) el índice 3 es el LFE y se
le da +1,5 dB. La normalización decide con una medida falseada.

**Solución**: `SonoridadMedia` lee la disposición real del flujo con
`CMAudioFormatDescriptionGetChannelLayout` antes de crear el medidor:
- Con `mChannelDescriptions`: pesos por etiqueta (LFE → 0, surrounds → 1,41,
  el resto → 1), BS.1770-4.
- Con tags conocidos de 5.1/7.1 (`kAudioChannelLayoutTag_MPEG_5_1_A/B/C/D`,
  `AudioUnit_5_1`, `MPEG_7_1_A`): pesos explícitos por orden conocido.
- Sin layout: el fallback por conteo actual (mono/estéreo a uno, 5+ asume 5.0).

## 4. La transcripción puede quedarse «Transcribiendo…» para siempre

**Problema**: `reconocer` (Subtitulos.swift:74-89) reanuda el continuation solo
con resultado final o error; un audio silencioso o ruido continuo puede no emitir
final jamás.

**Solución**: tarea de timeout de 120 s que cancela el `SFSpeechRecognitionTask` y
resume con `SubtitulosError.tiempoAgotado`; y `withTaskCancellationHandler` para
que cancelar la tarea circundante cancele también el reconocimiento. El guard de
`terminado` evita dobles resúmenes.

## 5. El reencuadre vertical muestrea a 30 fps fijos

**Problema**: `DetectorDeSujeto.rastrear` (Reframe.swift:51) calcula el tiempo de
muestreo con `/ 30.0` sin mirar la cadencia real; con 25/50/60 o VFR los keyframes
salen en el frame equivocado.

**Solución**: `rastrear` recibe el `timebase` del proyecto y convierte los frames
de proyecto a segundos con `timebase.segundos(frame)` (la conversión racional que
el modelo ya usa en todas partes); `Muestra.frame` sigue siendo relativo al clip.
`reframearVertical` (App.swift:1654) pasa `self.timebase`.

## Verificación

- Nuevos asserts en `tests/avisos` no aplica; se amplían:
  - `tests/reframe`: la conversión de tiempo con timebase distinta de 30 (lógica
    pura si se expone el cálculo de tiempo; si el cálculo vive en `rastrear`,
    que necesita AVFoundation, se extrae como función pura testeable).
  - `tests/sonoridad`: pesos por layout con etiquetas conocidas (función pura
    `pesos(para:canales:)` si se extrae).
  - `tests/timeline`: desfases convertidos tras `cambiarTimebase` (si se expone la
    conversión) — `cambiarTimebase` vive en App.swift (no testeable sin la app);
    se extrae la conversión de un grupo a `Timeline.swift` como función pura.
- Gates: `./probar.sh` verde, `./build-mac.sh` compila.

## Fuera de alcance

- La cola de transcripción cancelable (roadmap).
- Keyframes de ganancia dentro de fundidos de entrada/salida (la envolvente de
  fundido manda y no se mezcla con keyframes).
- Peso de layout para disposiciones exóticas sin tag ni descripciones (fallback
  por conteo, declarado).
