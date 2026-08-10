# Editorcito — top 10 de los competidores y cómo lo hacemos nosotros

Revisado el 6 de agosto de 2026. **Los diez puntos están hechos** —con dos
salvedades declaradas en su sitio: el punto 5 se cierra con el paneo (queda
limiter/compresor/EQ), el 8 con la rueda primaria y el waveform (queda el LUT
`.cube`), y el 9 con el arnés completo y corpus VFR real (falta poblar los 30
casos, y hasta que pase no se dice «editor»)—. Las piezas: `src/ui/Transcript.swift`,
`src/ui/Silencios.swift`, `src/ui/VistaTranscript.swift`, `extraerRango`/`extraerRangos`
en `Timeline.swift`, el quemado con estilo y resalte en `Composicion.swift`, el
`CompositorDeColor`, `DetectorDeSujeto`/`ReframeVertical`, `TapDePaneo`,
`probar-corpus.sh` y la bitácora de propuestas, con 214 comprobaciones en
`./probar.sh`. Lo aprendido al hacerlos:

- El tiempo por palabra **ya se leía y se tiraba** en `Subtitulos.swift`: conservarlo costó
  un campo y sostiene a la vez la edición por texto y el resalte palabra a palabra.
- Borrar varios tramos hay que aplicarlo **de atrás hacia delante**. Al revés, el primer
  borrado corre lo que viene detrás, los rangos siguientes apuntan a otro sitio, se come
  material que nadie seleccionó y no salta ningún error. Hay una prueba con tres tramos
  para que no pueda pasar por casualidad.
- Una selección seguida se funde en un solo rango **dentro del mismo clip** y no a través
  de un corte: entre dos clips del mismo medio puede haber material de otro que el panel de
  texto no enseña, y fundir ahí lo borraría sin que nadie lo haya pedido.
- El umbral de silencio es **relativo a la sonoridad integrada del propio material**. Con un
  umbral fijo en dBFS, en una voz susurrada no se corta nada y en un pódcast comprimido se
  corta todo, y no hay número que valga para los dos.

Ordenados por lo que más distancia abre en **nuestro nicho declarado**
—entrevistas, podcast y contenido social—, no por lo que más titula.

## Quién es cada uno| Competidor | En qué es el techo | Qué le duele |
|---|---|---|
| **DaVinci Resolve 21** | Gratis y de una magnitud mayor: página Photo nueva, ocho herramientas IA, IntelliSearch, IntelliTrack, CineFocus, Voice to Subtitle, uTalk, Krokodove en Fusion | Peso, complejidad y una curva que expulsa a quien solo quiere cortar una entrevista |
| **Premiere Pro 26** | `Media Intelligence` **local** con búsqueda en lenguaje natural, traducción de subtítulos a 17 idiomas, Generative Extend, Firefly en la línea de tiempo | Suscripción, nube para la traducción, ecosistema pesado |
| **Final Cut Pro** | Rendimiento en Apple Silicon y línea de tiempo magnética | Cerrado, sin Windows, poco flexible |
| **Descript** | El rival **directo** del nicho: editar borrando texto del transcript, muletillas fuera de una pasada, voz clonada | Calidad de salida y control de montaje limitados |
| **CapCut / Opus Clip** | Subtítulos animados palabra a palabra y clips verticales automáticos | Nube obligatoria, plantilla evidente, dudas de privacidad |

**Lo que esto significa:** contra Resolve no se gana por cantidad —es gratis y
tiene cien veces más—. Se gana por *ruta corta* en un trabajo concreto. Y el que
tiene esa ruta hoy es Descript, no Resolve. Los tres primeros puntos de la lista
son esa ruta.

## La tabla

| # | Lo que tienen | Quién | Estado | Cómo se hizo / se hace |
|---|---|---|---|---|
| 1 | **Editar borrando texto** del transcript | Descript, Premiere | **hecho** | `PalabraDelMontaje` mapea tiempo de medio a frames de montaje (con velocidad y recorte) y borrar llama a `extraerRangos`. Panel con clic y ⇧clic, doble clic para ir, y «Muletillas» que marca para revisar antes de quitar |
| 2 | **Buscar el material por lo que se dice** | Premiere `Media Intelligence` (local), Resolve `IntelliSearch` | **hecho** | `buscarEnLoQueSeDice`: sin acentos ni mayúsculas, frases en orden, contexto de 6 palabras. Encuentra también en medios **no montados** y ordena lo montado primero. «Transcribir todo» cubre la biblioteca en cola |
| 3 | **Silencios fuera y rough cut** reversible | Resolve, Premiere, Descript | **hecho** | `DetectorDeSilencios` sobre la curva momentánea del medidor que pasa la EBU: umbral relativo (−25 LU), histéresis de 8 LU, guarda de 120 ms, mínimo de 0,5 s. Un solo ⌘Z y un marcador en cada costura |
| 4 | **Subtítulos con estilo y resalte palabra a palabra** | CapCut, Premiere (17 idiomas) | **hecho** | `EstiloDeSubtitulo` serializado con el proyecto (fuente, color, contorno, fondo, posición, modo de resalte); quemado por palabra con `palabrasDelMontaje` del punto 1 |
| 5 | **Mezclador de verdad**: EQ, compresor, limiter, paneo | Fairlight | Paneo **hecho**; EQ/compresor/limiter pendientes | `TapDePaneo` con `MTAudioProcessingTap` en el parámetro de mezcla: el mismo código suena y se exporta |
| 6 | **Copiar y pegar atributos, match frame, extend edit** | Todos los NLE | **hecho** | ⌥⌘C/⌥⌘V sobre la selección (transformación, ganancia, fundidos); F abre el origen en el frame del cabezal; E estira al cabezal; Q/W recortan al cabezal |
| 7 | **Reencuadre vertical automático** con seguimiento | Premiere Auto Reframe, Resolve smart reframe | **hecho** | `DetectorDeSujeto` (Vision, on-device) → suavizado → keyframes de posición **editables** en `ReframeVertical` |
| 8 | **Color usable**: scopes, LUT, rueda primaria | Resolve es el estándar del mundo | Rueda primaria + waveform **hechos**; LUT `.cube` pendiente | `CompositorDeColor` (AVVideoCompositing + CIFilter) aplica el color igual en reproducción y exportación; forma de onda con vImage sobre el frame del monitor |
| 9 | **Fiabilidad con formatos raros** | Décadas de casos límite | **Arné completo + corpus VFR real**; falta poblar los 30 casos | `probar-corpus.sh`: sincronía por el método del clap en 5 puntos + cadencia de samples (caza el VFR); `MedioResuelto` ya detecta VFR al importar |
| 10 | **IA sobre imagen y voz**, no solo sobre la lista de cortes | Resolve 21 (8 herramientas), Premiere (Firefly) | **hecho** | El asistente lee el transcript (quita por lo que se dice), ve una miniatura del cabezal (1024 px JPEG, dos dialectos) y la bitácora registra aplicada/descartada/deshecha |

---

## 1. Editar borrando texto

**Ellos.** En Descript seleccionas «eh… bueno, lo que quería decir» en el
transcript, pulsas Supr, y el vídeo pierde ese trozo. Premiere lo lleva en
`Text-Based Editing` desde 2023. Para una entrevista de una hora es la diferencia
entre veinte minutos y tres.

**Nosotros.** Es el punto donde estamos más cerca de lo que parece.
`Subtitulos.swift` ya recorre `transcripcion.segments` leyendo
`segmento.timestamp` y `segmento.duration` —**el tiempo por palabra ya está en la
mano**— y lo agrupa en líneas de subtítulo, tirando el resto. Guardarlo cuesta un
campo.

Lo que falta, en tres piezas:

- **Modelo**: `Transcripcion` con `[Palabra(texto, inicio, duracion, clip)]`
  persistida en el `.editorcito`. Los campos opcionales nuevos ya se abren con
  compatibilidad hacia atrás, así que no rompe proyectos.
- **Vista**: un panel de texto donde la selección de rango de caracteres se
  traduce a rango de tiempo, con el cabezal siguiendo la palabra que suena.
- **Acción**: borrar la selección es `quitar cerrando hueco` sobre el rango, que
  ya existe y ya es una transacción de undo. Nada nuevo en el motor de montaje.

Y el remate que vende: **«quitar muletillas»** = buscar en las palabras un
diccionario en español (*eh, em, o sea, ¿sabes?, este…*) y proponer los N cortes
como **una sola** propuesta con diff, que es la disciplina que ya tiene el
asistente. Descript lo cobra; aquí sale de datos que ya se calculan y sin salir
del Mac.

Recomendación firme: **empezar por aquí.** Es el nicho declarado, la
infraestructura está al 70 %, y es lo que Resolve hace peor.

## 2. Buscar el material por lo que se dice

**Ellos.** `Media Intelligence` de Premiere etiqueta objetos, lugares, personas y
ángulos, y busca en lenguaje natural —y lo hace **en local**, sin subir nada, que
era nuestro argumento—. Resolve 21 trae `IntelliSearch` con la misma idea.

**Nosotros.** Perseguir el reconocimiento visual es caro y no es donde está el
valor de una entrevista. El 90 % del caso se cubre con lo que ya sabemos hacer:

- Transcribir **al importar**, en segundo plano y con cola cancelable, y guardar
  el transcript por medio en la biblioteca.
- La búsqueda de la biblioteca (que ya existe, por nombre) pasa a buscar también
  dentro del texto, y el resultado **salta al segundo exacto** del medio en el
  monitor de origen.
- Después, si compensa: embeddings de texto contra el stack local en `:9292`
  para que «cuando habla del precio» encuentre «nos costó demasiado». Eso es un
  índice vectorial pequeño, no un modelo de visión.

Y la parte visual, barata y honesta: etiquetas manuales por medio y por bin, más
lo que ya hay (fps, resolución, duración). No prometer «busca planos de la
ventana» hasta que haya con qué.

## 3. Silencios fuera y rough cut reversible

**Ellos.** Está en Resolve, en Premiere y es media razón de existir de Descript.

**Nosotros.** El trabajo difícil ya está hecho y verificado: el medidor de
`Sonoridad.swift` pasa el set oficial de la EBU (12/12 integrada, 6/6 LRA, 9/9
pico real). Sobre eso, detectar silencios es:

1. Sonoridad de ventana corta (400 ms, la de EBU Tech 3341) a lo largo del clip.
2. Umbral **relativo** a la integrada del propio clip —del orden de −25 LU—, no
   un dBFS absoluto: es lo que hace que funcione igual en una voz susurrada y en
   un pódcast comprimido.
3. Histéresis (entrar y salir con umbrales distintos) y guarda de 120 ms a cada
   lado, o los cortes se comen las consonantes iniciales.
4. Duración mínima de silencio configurable (por defecto 500 ms).

**Reversible** significa dos cosas concretas: los cortes entran como **una sola**
entrada de undo, y se dejan marcadores para poder revisarlos. Nada de aplicar
cien cortes irreversibles.

## 4. Subtítulos con estilo y resalte por palabra

**Ellos.** CapCut y las plantillas «virales»: palabra resaltada en el momento en
que se pronuncia, contorno, sombra, posición segura. Premiere añade traducción
nativa a 17 idiomas (con viaje a la nube: ahí tenemos argumento).

**Nosotros.** Sale **del mismo dato del punto 1**: teniendo el tiempo por
palabra, el resalte es interpolar qué palabra está activa en cada frame. Un dato,
dos productos.

- Presets de estilo serializados con el proyecto: fuente, cuerpo, color,
  contorno, fondo con opacidad, posición, márgenes seguros, y modo de resalte
  (ninguno / palabra / línea a línea).
- El quemado ya existe; pasa a dibujar con estilo en vez de texto plano.
- Traducción: con el modelo local y **solo** si se mide. Un subtítulo mal
  traducido que se quema en el vídeo es peor que no tenerlo.

**Hecho así:**

- `EstiloDeSubtitulo` (en `Timeline.swift`, junto a `Subtitulo`) es `Codable` y
  viaja en el `.editorcito`: un proyecto abierto en otro Mac tiene que quemar
  igual. `LineaDeTiempo.estilosDeSubtitulo` siempre contiene el «default», y
  cada `Subtitulo.estilo` nombra cuál usa.
- El quemado (`Composicion.herramientaDeSubtitulos`) dibuja con el estilo:
  fuente, cuerpo, color, contorno, fondo con opacidad y posición. Sin
  transcript —o con el modo en «ninguno»— quema el subtítulo entero como
  siempre, que es el camino de compatibilidad.
- El resalte por palabra reutiliza `palabrasDelMontaje`, la misma maquinaria
  que sostiene la edición por texto: coloca cada palabra del transcript en los
  frames del montaje con velocidad y recorte resueltos. Se quema una capa por
  palabra con una animación de color: la que suena se pinta del color de
  resalte, las demás del del texto. El fondo del estilo va en una capa aparte
  para no dibujar un rectángulo por palabra.
- La traducción sigue en «no iniciar»: no se quema nada que no se haya medido.

Límite honesto: el resalte por palabra necesita el transcript (punto 1); un SRT
importado de fuera solo trae el bloque entero y se quema sin resalte, con el
estilo sí.

## 5. Mezclador de verdad

**Ellos.** Fairlight: cadena por pista, buses, automatización, plugins.

**Nosotros.** Tenemos lo raro bien —medición conforme, ducking, normalización al
exportar verificada, y la garantía de que **la medición predice la exportación**
(0,01 LU de distancia)— y nos falta lo básico. Y hay una función muerta que
conviene cerrar: `Pista.paneo` existe, `fijarPaneoPista` lo escribe y **no lo usa
ni la mezcla ni la exportación**. Eso es un defecto, no una carencia.

Cómo se hace en este stack:

- `AVMutableAudioMix` solo da volumen y rampas. Para EQ, compresor y limiter hay
  que enganchar un `MTAudioProcessingTap` a la pista de composición, o pasar la
  reproducción a `AVAudioEngine` con una cadena de `AVAudioUnit`.
- Las unidades son de Apple y no hay que escribir DSP:
  `kAudioUnitSubType_NBandEQ`, `kAudioUnitSubType_DynamicsProcessor`,
  `kAudioUnitSubType_PeakLimiter`, `kAudioUnitSubType_MatrixMixer` para el paneo.
- **La regla que ya nos costó una vez**: cualquier cosa que se añada a la
  reproducción tiene que aplicarse igual en la exportación y medirse con el
  mismo medidor. El fallo del fundido a silencio en toda la duración del clip
  salió justo de que reproducción y export interpolaban por su cuenta.

Orden sensato: paneo (cerrar la función muerta) → limiter (protege el pico, que
ya sabemos medir) → compresor → EQ.

**Hecho (paneo):** `Pista.paneo` existía, `fijarPaneoPista` lo escribía y **nadie
lo leía** —la función muerta que el punto anunciaba—. Ahora `ConstructorDeMontaje`
lo engancha como `audioTapProcessor` del parámetro de mezcla: `TapDePaneo` aplica
una ley de balance (ganancia por canal: −1 todo a la izquierda, +1 todo a la
derecha, centro a media amplitud, no a plena) con un `MTAudioProcessingTap`. Como
el tap viaja dentro de `mezclaDeAudio`, la reproducción (`item.audioMix`) y la
exportación (`session.audioMix`) usan **el mismo código**, que es la regla que ya
costó un fallo de fundidos. Probado en `tests/paneo` (ley de balance, recorte de
valores disparatados, y que el tap se engancha a un parámetro de mezcla real).

Queda el limiter, el compresor y el EQ: la vía es la misma (AudioUnits de Apple en
el tap, o pasar la reproducción a `AVAudioEngine`), y la regla de que
reproducción y exportación tienen que aplicar lo mismo sigue mandando.

## 6. Acciones, atajos y clic derecho

Hay tres `contextMenu` en toda la app. Un editor que viene de Premiere echa en
falta estas en cinco minutos, y **todas se apoyan en operaciones que ya existen**
en `Timeline.swift`:

| Acción | Atajo habitual | Qué es |
|---|---|---|
| **Copiar atributos / Pegar atributos** | ⌥⌘C / ⌥⌘V | Llevar escala, posición, opacidad, ganancia y fundidos de un clip a veinte. La más rentable de la tabla |
| **Match frame** | F | Abrir en el monitor de origen el medio del clip bajo el cabezal, en ese frame |
| **Extend edit** | E | Estirar el corte seleccionado hasta el cabezal |
| **Trim to playhead** | Q / W | Recortar hasta el cabezal por la izquierda / derecha |
| **Replace edit** | ⇧D | Sustituir el clip por lo que hay en el monitor de origen conservando duración |
| **Nudge** | ⌥, / ⌥. | Mover un frame |
| **Añadir corte en todas las pistas** | ⌃⇧K | Partir lo que esté bajo el cabezal en todas |
| **Pegar solo vídeo / solo audio** | — | Con edición enlazada es lo que se pide constantemente |
| **Revelar en Finder** | — | Una línea |
| **Render de entrada a salida** | — | Cachear el tramo para reproducirlo fluido |

Clic derecho que faltan: en el **clip** (velocidad…, propiedades, desenlazar A/V,
reemplazar, revelar en Finder, etiqueta de color); en la **cabecera de pista**
(silencio, solo, altura, bloquear, insertar pista arriba/abajo, borrar pista); en
la **regla** (marcador, borrar marcador, fijar entrada/salida de trabajo); en la
**biblioteca** (transcribir, crear multicámara, mostrar en Finder, sustituir
medio offline).

Sin esto la app se siente demo, con esto se siente NLE. Es el mejor cambio por
hora de trabajo de toda la lista.

**Hecho así** (todo sobre el modelo de `Timeline.swift` que ya existía, como
prometía la tabla):

- **Copiar/pegar atributos (⌥⌘C / ⌥⌘V)**: copia transformación (posición,
  escala, rotación, opacidad), ganancia y fundidos del clip seleccionado, y pega
  en todos los seleccionados de una vez —el punto es llevar un look a veinte
  clips—. La velocidad no entra: pegar velocidad cambiaría la duración, que no
  es un atributo. Un ⌘Z lo deshace todo (`performEdit`).
- **Match frame (F)**: abre en el monitor de origen el medio del clip bajo el
  cabezal, y coloca el cabezal de origen en el frame que corresponde —con
  velocidad y recorte resueltos—.
- **Extend edit (E)**: estira al cabezal el borde más cercano; con empate, el de
  salida. No hay que agarrar el borde con el ratón: mueves el cabezal y pulsas E.
- **Trim to playhead (Q/W)**: recorta la entrada/salida al cabezal, que ya
  existía (`recortarHastaCabezal`) y ahora tiene su atajo.
- En el clic derecho del clip: Copiar atributos, Pegar atributos, Match frame y
  Extend edit. Cubierto por `tests/atributos` (qué viaja en el pegado, el
  criterio de borde del extend y el cálculo del match frame).

## 7. Reencuadre vertical automático

**Ellos.** Auto Reframe de Premiere y el smart reframe de Resolve siguen al
sujeto y animan el encuadre al pasar de 16:9 a 9:16.

**Nosotros.** Ya exportamos vertical, con encuadre fijo, y ya hay keyframes de
transformación en el modelo. Falta el detector:

- `VNDetectFaceRectanglesRequest` y `VNTrackObjectRequest` de Vision, on-device
  y gratis, muestreando cada N frames.
- Suavizar la trayectoria (media móvil o un paso de Kalman sencillo: si no, el
  encuadre tiembla) y **volcarla como keyframes de posición del clip**.
- Ahí está la ventaja sobre ellos, y hay que decirla así: Premiere te da una caja
  negra; aquí salen **keyframes normales que puedes tocar, borrar o rehacer**.

**Hecho así:**

- `DetectorDeSujeto.rastrear` decodifica el clip a 360 px de ancho (basta para
  seguir una cara y cuesta una fracción), detecta la cara en el primer frame con
  `VNDetectFaceRectanglesRequest` y la sigue con `VNTrackObjectRequest`; si el
  seguimiento se pierde, vuelve a detectar. Todo en el dispositivo y sin subir
  nada, que es el argumento de la casa.
- `DetectorDeSujeto.suavizar` aplica una media móvil: el encuadre que salta de
  frame en frame se siente peor que uno ligeramente retrasado.
- `ReframeVertical.keyframes` convierte la trayectoria en `ClipKeyframe`
  normales: la escala sale del tamaño del sujeto (un sujeto del 30 % de la
  imagen no necesita zoom; uno del 10 %, 300 %), la posición sigue al centro. El
  giro, la opacidad y el recorte del usuario se conservan.
- La acción `reframearVertical` corre en segundo plano (muestrear tarda) y está
  en el clic derecho del clip: «Reencuadre vertical automático». Cubierto por
  `tests/reframe` (suavizado, escala, posición y conservación de atributos).

## 8. Color usable

**Ellos.** Resolve *es* la referencia mundial de color. No se compite: se cubre
el suelo para que nadie tenga que salir de la app por una corrección básica.

**Nosotros.** Nada todavía. Alcance realista y suficiente:

- Rueda primaria: exposición, temperatura, contraste, saturación, altas/bajas.
- LUT `.cube` por clip y por pista.
- **Waveform y vectorscopio** del frame del monitor, en un compute shader — un
  histograma sobre la textura ya decodificada, que es barato y es lo que convence
  a alguien que sabe de color.
- Se implementa como filtro en la cadena de `Composicion.swift`, que ya monta
  capas con z-orden, opacidad, encuadre y recorte. Con `AVVideoComposition`
  personalizado se aplica igual en reproducción y en exportación, que es la
  única forma de que el color no mienta.

**Hecho así:**

- `CompositorDeColor` es un `AVVideoCompositing` que aplica la cadena con
  `CIFilter`: exposición en EV (no lineal), contraste y saturación con
  `CIColorControls`, temperatura alrededor de 6500 K con `CITemperatureAndTint`,
  y altas/sombras con una curva interpolada a la tabla de `CIColorCurves`. El
  orden de la cadena es fijo y está declarado en el código.
- Cada tramo de la composición lleva su color en `InstruccionConColor` (el del
  clip visible de la pista superior), y el `customVideoCompositorClass` solo se
  activa cuando algún clip tiene color: sin color no hay coste.
- El color viaja en `Clip.color` (ya existía) y se edita desde el clic derecho:
  «Corrección de color» con los seis mandos y «Restablecer». Como el compositor
  es el `videoComposition` que usa el reproductor y el exportador, el monitor
  enseña exactamente lo que saldrá en el archivo —la regla de los fundidos otra
  vez.
- La **forma de onda** (`Scope.swift`) se calcula con vImage sobre el frame del
  monitor capturado con `AVAssetImageGenerator`: el mismo dato que iría a un
  compute shader, sin duplicar la decodificación. Se alterna desde el menú Ver y
  se refresca al mover el cabezal. Verde por debajo del blanco nominal, rojo por
  encima.
- Cubierto por `tests/color` (la cadena es lógica pura de Core Image).
- **Pendiente**: el LUT `.cube` por clip/pista, que es un `CIColorCube` más en la
  cadena cuando el proyecto traiga un archivo de LUT.

Lo que **no**: nodos, HDR, tracking de máscaras. Está en «no iniciar todavía» y
así debe quedarse.

## 9. Fiabilidad con formatos raros

**Ellos.** Décadas de casos límite acumulados. Es ventaja imposible de igualar y
por eso hay que acotar en qué se es fiable, y demostrarlo.

**Nosotros.** El gate P0 sigue abierto y es lo que separa «alpha» de «editor». Lo
concreto que falta es un **corpus golden** de unos 30 archivos con asertos, no
una impresión general:

- VFR de iPhone y de Android (el caso que más rompe la sincronía).
- HEVC 10 bits HLG, H.264 con B-frames, ProRes 422, AV1.
- 23,976 con timecode drop-frame, 25, 29,97, 50, 60.
- Rotación por metadato (retrato grabado en horizontal), anamórfico.
- Audio a 44,1 y 48 kHz en el mismo proyecto, mono, estéreo y multipista.
- Un archivo con audio que arranca antes del vídeo, y uno con hueco de PTS.

Y el arnés: `probar-corpus.sh` que para cada archivo compruebe **sincronía A/V
≤ 1 frame** en cinco puntos, acierto de seek sobre 10.000 saltos, y que la
exportación no trunque. Los guiones de esta casa ya funcionan así
(`probar-sonoridad.sh` contra el set de la EBU es el modelo a seguir: se pasa o
no se pasa, sin interpretación).

**Hecho así:**

- `probar-corpus.sh` comprueba dos cosas objetivas por archivo:
  1. **Sincronía A/V por el método del clap**: detecta el flash blanco en el
     vídeo y el pitido en el audio dentro de una ventana, y mide su desfase.
     Los archivos sin patrón de prueba se marcan con `?` —no se castigan, pero
     no dan el visto bueno—.
  2. **Cadencia de samples**: lee los tiempos de presentación de todos los
     samples de vídeo y audio y cuenta los saltos de cadencia mayores de 1,5
     frames. Un reloj que salta es un VFR que el editor tiene que tratar; más
     de un 1 % de huecos es material que no se puede montar sin desfasar el
     audio.
- El corpus inicial son grabaciones de pantalla de macOS reales, que **son VFR
  de verdad** (43,6 y 40,2 fps nominales, 32-34 % de saltos de cadencia): el
  caso exacto que rompe la sincronía. El arnés los detecta y los marca —ese es
  su trabajo—.
- `MedioResuelto.cargar` ya detecta el VFR al importar (`esVFR`), así que el
  editor sabe con qué está tratando antes de montar.

**Lo que falta** (y el gate P0 sigue abierto): poblar el corpus con los ~30
casos de la lista —HEVC 10 bits HLG, H.264 con B-frames, ProRes 422, AV1,
23,976 drop-frame, rotación por metadato, audio 44,1/48 kHz mezclado, audio que
arranca antes que el vídeo, hueco de PTS— y que `probar-corpus.sh` los pase
todos. Hasta entonces no se dice «editor» en ningún sitio.

## 10. La IA sobre imagen y voz

**Ellos.** Resolve 21 mete ocho herramientas de IA de golpe —IntelliTrack,
CineFocus, Voice to Subtitle, uTalk—, Premiere mete Firefly en la línea de
tiempo. Es imposible igualar la cantidad.

**Nuestra ventaja es real y hay que profundizarla, no ensancharla**: local,
gratis, con diff antes de aplicar, cancelable, y el plan se descarta si el
montaje cambió mientras el modelo pensaba. Eso último no lo hace ninguno de
ellos.

Tres mejoras concretas, en orden:

1. **Que el asistente lea el transcript.** Con el punto 1 hecho, «quita donde se
   equivoca al presentarse» pasa de imposible a una búsqueda más un `quitar
   cerrando hueco`. Hoy solo conoce timecodes y nombres de clip.
2. **Que vea un frame.** Para órdenes visuales hace falta una miniatura del
   cabezal en la petición. El camino está resuelto en Yunkil y se copia tal cual:
   reducir a 1024 px y JPEG antes de base64, los dos dialectos de contenido
   (`image_url` y `source/base64`), y presupuesto de tokens mayor cuando hay
   imagen. Sin reducir, el fallo no es «lento»: es un error de servidor que no
   menciona la imagen.
3. **Bitácora de propuestas.** Yunkil registra en JSONL qué propuso la IA y qué
   hizo la persona —aplicada, descartada, o deshecha en los 45 segundos
   siguientes, que es el «no» más rotundo—. Editorcito no la tiene, y es el único
   dato de calidad que no se puede fabricar sintéticamente ni reconstruir
   después. Cuesta un archivo y hay que ponerla **antes** de afinar prompts.

**Hecho así:**

1. **El transcript entra en el contexto del modelo** (`contextoParaIA`): cada
   línea lleva su timecode del montaje y el número de clip que la dice. La orden
   se resuelve con las acciones que ya existían —«recortar» con la entrada y la
   salida del tramo que se oye—, sin una orden nueva en el vocabulario.
2. **El asistente ve el cabezal**: `miniaturaDelCabezal` captura el frame del
   reproductor, lo reduce a 1024 px y lo pasa a JPEG (factor 0,7) antes de
   base64; `NovaAssistant.planConImagen` lo manda en los dos dialectos
   (`image_url` para OpenAI, `source`/`base64` para Anthropic) y sube el
   presupuesto a 3.000 tokens, que es lo que cuesta describir una imagen. Sin
   miniatura —medio sin vídeo— la llamada es la de siempre.
3. **La bitácora** (`propuestas.jsonl` en Application Support) registra cada
   propuesta —petición, resumen, acciones, desenlace— y el deshacer inmediato
   corrige el asiento a `DESHECHO` dentro de la ventana de arrepentimiento de
   45 s, como en Yunkil. Está puesta antes de afinar prompts, que es el único
   orden correcto.

---

## Lo que no copiamos, y por qué

- **Generación de vídeo** (Firefly, Generative Extend). Nube, coste por segundo y
  ninguna relación con terminar una entrevista.
- **Nodos de color, HDR, VFX 3D, plugins.** Ya está en «no iniciar todavía».
- **Voz clonada** (Descript). Riesgo reputacional que no compensa.
- **Cualquier afirmación 4K/8K o «alternativa a Resolve»** sin benchmark
  reproducible y proyectos reales terminados.

## Orden recomendado

1. Punto 1 (edición por transcript). Es el nicho, y el dato ya se está leyendo.
2. Punto 6 (clic derecho y atajos). Una tarde, cambio de percepción entero.
3. Punto 3 (silencios), que cae casi solo del medidor que ya pasa la EBU.
4. Punto 4 (subtítulos con estilo), que reutiliza el dato del punto 1.
5. Punto 9 (corpus) en paralelo y sin excusas: hasta que pase, no se dice
   «editor» en ningún sitio.
6. Punto 5 (paneo primero, que es cerrar una función muerta).

## Fuentes

- [DaVinci Resolve 21 — guía de novedades (PDF)](https://documents.blackmagicdesign.com/SupportNotes/DaVinci_Resolve_21_New_Features_Guide.pdf)
- [Resolve 21 — anuncio y herramientas IA (CineD)](https://www.cined.com/davinci-resolve-21-announced-new-photo-page-eight-new-ai-tools-tethered-camera-controls-and-more/)
- [Resolve 21 — release final (PetaPixel)](https://petapixel.com/2026/06/03/davinci-resolve-21-officially-released-with-new-photo-editing-ai-tools-and-much-more/)
- [Adobe — auto-etiquetado y traducción de subtítulos](https://www.broadcastnow.co.uk/production-and-post/adobe-reveals-ai-auto-tagging-and-caption-translation-for-premiere-pro/5201022.article)
- [Premiere Pro 26 — novedades](https://phantomeditor.video/blog/whats-new-premiere-pro-26-2026)
- [Final Cut Pro](https://www.apple.com/final-cut-pro/)
