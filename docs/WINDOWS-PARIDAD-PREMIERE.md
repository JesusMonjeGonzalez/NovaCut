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

## Lo que sigue faltando frente a Premiere

Por orden de lo que más se nota al montar (lo de texto, subtítulos y GPU ya
está hecho):

1. **Keyframes de cualquier parámetro.** Solo se animan posición, escala,
   opacidad y volumen; Lumetri y efectos son constantes por clip.
2. **Lumetri avanzado**: curvas HSL, secundarias con selección de color, blancos
   y negros separados, coincidencia de color entre planos.
3. **Warp Stabilizer de dos pasadas** (`vidstab`) y remapeo de tiempo con flujo
   óptico (`minterpolate`).
4. **Transiciones de barrido y zoom**, y transiciones de audio propias
   (potencia constante) separadas de las de imagen.
5. **Varias secuencias por proyecto** y bins; hoy un proyecto es una secuencia,
   y las secuencias anidadas se importan desde otro `.ncrough`.
6. **Cola de exportación** y ajustes de bitrate/CRF editables por el usuario.
7. **Previsualización en el monitor** de efectos temporales: estabilizar y el
   ruido animado solo se ven al reproducir o exportar, no en el fotograma fijo.
8. Plantillas de gráficos animadas (.mogrt) y selección de fuente en títulos.
