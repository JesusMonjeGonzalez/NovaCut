# NovaCut Windows frente a Premiere Pro (25 sep 2026)

Qué tiene el host Windows respecto a las funciones de Premiere que un montador
usa a diario, qué se añadió en esta ronda y qué falta todavía.

## Añadido en esta ronda

| Premiere | NovaCut Windows | Dónde |
|---|---|---|
| Lumetri › Básico (temperatura, tinte, intensidad, sombras, iluminaciones) | Deslizadores por clip y en capas de ajuste | Inspector › Lumetri básico |
| Efectos Enfocar, Reducción de ruido, Ruido | Enfocar (`unsharp`), Reducir ruido (`hqdn3d`), Grano (`noise`) | Inspector › Detalle y textura |
| Voltear, Recortar | Volteo H/V; recorte por borde con transparencia | Inspector › Transformar y recortar |
| Invertir velocidad | Reproducir hacia atrás, imagen y sonido | Inspector › Tiempo y estabilización |
| Estabilizador de deformación | Estabilizar de un paso (`deshake`) | Inspector › Tiempo y estabilización |
| Pasar a blanco, Deslizar, Empujar | 9 transiciones en total | Inspector › Transición de entrada |
| Sonido esencial (Diálogo/Música, reducir ruido, ducking automático) | Papel por clip, limpieza de voz, compresor y ducking por cadena lateral | Inspector › Sonido esencial |
| EQ paramétrico | EQ de tres bandas | Inspector › Ecualizador y dinámica |
| Keyframes de volumen (banda elástica) | Puntos en el cabezal, curva dibujada en la timeline | Inspector › Volumen animado |
| Gráficos esenciales (fondo, trazo, sombra), Mate de color | Caja, contorno, sombra, fondo completo; plantillas | Inspector › Apariencia, "+ Insertar" |
| Media Encoder: HEVC, ProRes, WebM, GIF | Los cuatro, con la misma exportación atómica | Selector de formato |

## Segunda ronda: texto, subtítulos y GPU

| Referencia | NovaCut Windows | Dónde |
|---|---|---|
| Premiere «Edición basada en texto» / Descript | Transcripción palabra a palabra; borrar texto corta todas las pistas | Pestaña Transcripción |
| Descript «Remove filler words» | Detección y borrado de muletillas en español e inglés | Pestaña Transcripción |
| CapCut / TikTok subtítulos automáticos | 4 estilos animados palabra a palabra, siguen las ediciones | Pestaña Transcripción › Subtítulos animados |
| Aceleración por hardware de Premiere/Media Encoder | NVENC, Quick Sync, AMF (y VideoToolbox en el host de desarrollo), con vuelta a CPU | Botón ⚡ GPU junto al formato |

Detalles de diseño:

- La maquetación de los subtítulos se calcula en NovaCut midiendo con la misma
  fuente TTF que usa `drawtext`, y cada palabra se dibuja con su propio
  `drawtext`. El estado normal y el resaltado ocupan exactamente el mismo sitio.
- Cortar por texto respeta velocidad, clips invertidos, keyframes y la banda de
  volumen; se niega sin tocar nada si el corte cruza una secuencia anidada o
  una rampa, o si hay pistas bloqueadas después del corte.
- La detección de GPU no se fía de la lista de codificadores de FFmpeg (los
  builds de Windows traen los tres aunque no haya GPU): codifica unos
  fotogramas de prueba con cada una.

Arreglado de paso: la exportación no escalaba el tamaño de los títulos a la
resolución de salida (el monitor sí), el monitor sin imagen ocultaba el
transporte y la timeline, y la fila de transporte tapaba el timecode con
monitores estrechos.

## Tercera ronda: usabilidad de la interfaz

Revisada ventana a ventana en 1280×720 (1080p al 150 %), 1366×768,
1536×864 (1080p al 125 %) y 1920×1080 emulado, con clics reales.

Lo que se cambió: timeline con alto garantizado (a 1280×720 no se veía ninguna
pista), paneles inferiores movidos a pestañas de la columna derecha, barra
superior en una fila con menú Archivo, herramientas como iconos, Inspector en
rejillas y secciones, tamaño de interfaz ajustable, tipografía mínima de 11 px
y más contraste.

Fallos graves que destapó la revisión y que ya existían:

- **Los títulos tapaban el vídeo con negro** en monitor y exportación: la
  fuente `color=black@0.0` de FFmpeg es opaca sin `format=rgba`.
- **El monitor se quedaba en negro** cuando el cabezal caía entre dos
  fotogramas del medio (cualquier proyecto con cadencia distinta a la del
  vídeo): faltaba `setpts=PTS-STARTPTS` tras la búsqueda.
- **Iconos como cuadrados vacíos** (Imán, flechas de los menús, herramientas,
  atajos con flechas): esos glifos solo estaban en la fuente monoespaciada.
  Una prueba recorre ahora todos los textos de la interfaz.
- El Inspector numeraba la pista como V0 mientras la timeline decía V1.

## Arreglado de paso

- **Audio exportado truncado.** Con FFmpeg reciente, el retardo que coloca cada
  clip en la timeline (`adelay`) dejaba timestamps que `amix` y el muxer
  malinterpretaban: un MP4 de 10 s salía con 0,006 s de AAC y un MP3 con 2,5 s.
  Ahora se renumeran tras el retardo. Lo cubre la prueba de render real.
- **Fundidos de audio desplazados.** Los `afade` iban después del retardo, así
  que el fundido de entrada de un clip que no empezaba en 0 se aplicaba al
  silencio previo. Ahora van antes, en tiempo local del clip.
- Tres pruebas del host fallaban por errores de las propias pruebas (una
  subcadena que siempre coincidía y dos clips por defecto demasiado cortos).

## Cómo se verifica

- `cargo test --features windows-host --bin novacut-windows` corre también en
  macOS y Linux (el host compila fuera de Windows para desarrollo).
- `render_real_tests` genera medios sintéticos, monta un proyecto con todos los
  efectos, títulos, capas de ajuste y transiciones nuevas, y exporta en los
  siete formatos comprobando la duración de cada archivo. Otra prueba mide que
  la música baja más de 6 dB bajo el diálogo con ducking. Si no hay FFmpeg, se
  saltan. `NOVACUT_KEEP_RENDERS=1` conserva las salidas y
  `NOVACUT_REQUIRE_REAL=1` convierte cualquier salto en fallo.
- La edición por texto se prueba de punta a punta: voz sintetizada, Whisper
  real, borrado de muletillas y una segunda transcripción que confirma que ya
  no se oyen. Necesita `NOVACUT_WHISPER_DIR` y `say` (macOS).
- La GPU se prueba codificando de verdad y comprobando en el archivo qué
  codificador lo escribió, y la vuelta a CPU con una GPU que el equipo no tiene.
- Los subtítulos animados se prueban leyendo píxeles: la palabra resaltada se
  desplaza de izquierda a derecha y en Karaoke se acumulan.
- Compila en cruzado para `x86_64-pc-windows-gnu` desde macOS.

## Cuarta ronda (28 sep 2026): los ocho bloques de la lista anterior

Clasificados de más a menos útiles para montar a diario, y hechos en ese
orden. Cada bloque destapó fallos que ya existían; van al final.

| Premiere | NovaCut Windows | Dónde |
|---|---|---|
| J/K/L con velocidades, marcha atrás, K+J/K+L | 1×→2×→4×→8× en ambos sentidos; atrás compone por tramos | Teclado |
| Rodar (N), Desplazar (Y), Deslizar (U) | Rodar N, Desplazar Y, Deslizar Mayús+Y | Herramientas |
| Levantar (`;`) / Extraer (`'`) | Sobre el rango I–O, respetando pistas bloqueadas | Teclado |
| Q/W recortar con ripple al cabezal | Igual, sobre el seleccionado o el de más arriba | Teclado |
| Cronómetro de keyframes en cada efecto | ◇/◀◆▶ en exposición, contraste, saturación, viñeta, desenfoque, temperatura, tinte, intensidad y rotación | Inspector |
| Media Encoder: preajustes, calidad/bitrate, cola | Panel Exportar (Ctrl+M), 10 preajustes, CRF o Mbps, audio kbps, cola en serie | Botón Exportar |
| Panel Proyecto: bins y varias secuencias | Biblioteca con bins anidados; secuencias nuevas, duplicar, renombrar, anidar | Medios › Proyecto |
| Lumetri: Blancos/Negros, HSL secundaria, Comparación de color | Los tres; la igualación mide la referencia ya graduada | Inspector |
| Transiciones de barrido e iris; audio de potencia constante | 4 barridos, iris y zoom; todo cruce de audio con curva `qsin` | Transición de entrada |
| Warp Stabilizer; interpolación por flujo óptico | vidstab de dos pasadas con análisis en caché; muestreo/mezcla/flujo óptico | Tiempo y estabilización |
| Fuentes en títulos | Selector por familia de las fuentes instaladas; títulos de varias líneas | Inspector del título |

Fallos que ya existían y que se arreglaron por el camino:

- **La exportación de keyframes no animaba**: cada tramo entre keyframes salía
  fijo en el valor de su punto medio. Ahora es animación por fotograma
  (expresiones y `sendcmd`), y la escala animada pasa por un lienzo fijo para
  que el giro no recorte.
- **Reproducir desde el minuto 45 decodificaba 45 minutos**: el monitor
  componía siempre desde 0. Ahora compone solo desde el cabezal.
- **En Windows, los montajes largos no se podían exportar**: con unos 200
  clips el grafo de filtros pasa de 32 767 caracteres, el límite de la línea
  de órdenes. El grafo viaja siempre por archivo (`-/filter_complex`).
- **«Exportar fotograma» nunca escribía el PNG** (faltaba el archivo de salida).
- **Partir clips invertidos** daba el material equivocado, y partir o recortar
  la cabeza desplazaba keyframes y banda de volumen. Había cuatro copias del
  código de partir; ahora hay una (`montaje.rs`).
- **Las disolvencias subían el volumen**: el saliente no bajaba mientras el
  entrante subía.
- **Los fundidos de los títulos** se aplicaban en el segundo 0 del montaje, no
  al empezar el título.
- **Un título con «%»** rompía el render entero.
- **`rotate` dejaba restos** de fotogramas anteriores en las esquinas al
  animar el giro.
- **Exportar un rango** que empezaba en mitad de un fundido, una transición o
  un anidado no coincidía con el montaje completo.

## Lo que sigue faltando frente a Premiere

1. **Enlace A/V y bloqueo de sincronía**: el ripple actúa por pista; con audio
   separado (J/L-cuts) puede desincronizar pistas que Premiere movería juntas.
2. **Previsualización en el monitor** de efectos temporales (estabilizar,
   grano animado, barridos a mitad) en el fotograma fijo; se ven al reproducir.
3. **Curvas HSL dibujables** (tono contra saturación); hoy la HSL secundaria es
   por familias de color.
4. **Plantillas de gráficos animadas** (.mogrt) y animación de texto por letra.
5. **Render en segundo plano de la timeline** (barra roja/amarilla/verde) para
   reproducir en tiempo real efectos pesados como el flujo óptico.
