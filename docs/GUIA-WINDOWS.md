# NovaCut Windows 0.1

Este es el primer host Windows de NovaCut. Permite:

- importar varios videos;
- ordenar y eliminar planos;
- ajustar puntos de entrada y salida;
- mover el cabezal en una timeline visual y partir clips con `Ctrl+K`;
- ver el fotograma del cabezal en el monitor integrado;
- previsualizar cada recorte;
- guardar y abrir proyectos `.ncrough`;
- abrir proyectos macOS `.editorcito`, conservando las pistas de video compatibles;
- colocar clips con huecos o solapados en hasta 16 pistas de video;
- importar audio independiente (WAV, MP3, M4A, FLAC) en hasta 16 pistas de audio;
- arrastrar clips de video o audio en tiempo y entre sus pistas;
- recortar in y out arrastrando los bordes de cada clip en la timeline;
- aplicar transiciones por negro con el clip anterior de la misma pista;
- componer capas y mezclar su audio respetando la posicion en timeline;
- usar 14 modos de fusion compatibles con proyectos Mac y máscaras rectangulares
  o elípticas con posición, tamaño, inversión y pluma;
- ajustar posicion, escala, rotacion y opacidad de cada capa;
- aplicar fundidos de entrada y salida de imagen y audio por clip;
- crear titulos de texto y convertir clips en titulos (posicion, tamano y color);
- ajustar exposicion, contraste y saturacion de cada clip de video;
- vineta, desenfoque gaussiano y ruedas de color sombras/medios/altas;
- curvas de color: maestra (luma) + R/G/B con puntos editables;
- cargar LUT 3D `.cube` por clip y mostrar waveform/vectorscope en el monitor;
- animar posición, escala y opacidad con keyframes ("Animar (keyframes)" en el
  inspector; el monitor evalúa el fotograma exacto y el export interpola por
  tramos entre keyframes);
- croma (pantalla verde/azul) con tolerancia, suavizado y supresion de derrame;
- multicamara basica: alinea un clip de cada angulo en pistas consecutivas
  (mismo inicio) y corta entre angulos con las teclas `1`-`4` durante la
  reproduccion o el repaso; `1` vuelve al angulo base;
- sincronizar los angulos por audio: selecciona el clip de referencia y pulsa
  "Sincronizar angulos por audio" (correlacion de envolventes, ±20 s);
- subtitulos: se crean en "Subtítulos", se exportan como archivo `.srt` y se
  pueden importar, estilizar y quemar en el video exportado y en el monitor;
- transcribir localmente con Whisper cuando `whisper-cli.exe` y cualquier
  modelo `ggml-*.bin` (tiny/base/small/medium/large, con o sin sufijos) están
  instalados; se usa el más pequeño disponible;
- mezclar ganancia por pista y master, medir LUFS y normalizar la exportación
  a -14 LUFS con precisión de dos pasos cuando la medición sigue vigente
  (si el proyecto cambió desde la medición, usa un paso aproximado);
- detectar y cortar silencios del clip seleccionado con ripple;
- detectar cortes de escena y partir el clip en planos automáticamente
  (revisión previa antes de aplicar);
- capas de ajuste ("+ Ajuste"): un clip sin medio propio que gradúa, difumina
  o enmascara todo lo que hay debajo en las pistas inferiores, limitado a su
  rango de tiempo (como en Premiere o DaVinci);
- balance estéreo por clip (L/R);
- ajuste magnético en la timeline: los arrastres se anclan a bordes de clips,
  al cabezal y a los marcadores (se desactiva con el interruptor);
- crear proxies H.264 540p para el monitor; la exportación siempre usa originales;
- aplicar reframe centrado automático a los presets vertical 1080x1920 y
  cuadrado 1080x1080 (Instagram), además de 720p/1080p/4K;
- crear rampas de velocidad interpoladas y usar el audio como reloj maestro;
- anidar/desanidar una pista e importar otro `.ncrough` como secuencia anidada;
- eliminar con ripple (`Shift+Supr`): cierra el hueco en la misma pista;
- previsualizar el montaje completo renderizado en modo rapido;
- reproducir el montaje entero dentro del monitor con `Espacio` o el boton
  "Reproducir": el cabezal avanza en la timeline, suena el audio de los clips
  y el medidor L/R muestra los niveles en dBFS;
- hacer zoom en la timeline (Ctrl+rueda o botones, con desplazamiento lateral);
- atajos: `S` corta en el cabezal, `Supr` elimina el clip, `Inicio`/`Fin` y
  flechas mueven el cabezal, `Ctrl+S` guarda, `Espacio` reproduce el recorte;
- detectar medios ausentes y revincularlos desde el inspector (barra inferior
  muestra "N medio(s) OFFLINE");
- ver miniaturas automaticas de cada clip de video en la timeline;
- clasificar clips con etiquetas de color (se guardan en el proyecto);
- exportar en 1080p, 4K o vertical 1080x1920 con barra de progreso y tiempo
  restante estimado durante el render;
- exportar solo audio en WAV o MP3;
- colocar marcadores con nombre (`M` o "+ Marcador"), saltar a ellos con un
  clic en la timeline y editarlos o borrarlos en la lista "Marcadores";
- regular el volumen del monitor mientras se reproduce;
- exportar el fotograma del cabezal como imagen PNG;
- cerrar el hueco delante del clip seleccionado con un clic;
- crear copias de seguridad automaticas con cada guardado (carpeta
  "NovaCut-Backups" junto al proyecto, se conservan las 10 ultimas);
- ajustar ganancia o silenciar el audio de cada clip;
- deshacer y rehacer con `Ctrl+Z` y `Ctrl+Y` (escribir texto o arrastrar un
  control agrupa el gesto completo en un solo paso, no uno por tecla);
- confirmar antes de perder cambios sin guardar al pulsar "Nuevo" o "Abrir";
- recuperar automaticamente la ultima sesion;
- exportar el montaje multipista a H.264 1080p con audio AAC.
- cancelar una exportacion sin destruir el archivo de destino anterior.

Tambien se pueden arrastrar videos desde el Explorador directamente a la
ventana. `Ctrl+S` guarda y la barra espaciadora previsualiza el recorte elegido.

## Instalacion

La forma recomendada es abrir `NovaCut-Windows-Setup.exe` y seguir el asistente.
El instalador crea el acceso del menu Inicio, puede crear otro en el escritorio,
asocia los proyectos `.ncrough` e instala FFmpeg automaticamente.

La version portable se abre con `novacut-windows.exe`. Si falta FFmpeg, NovaCut
muestra una pantalla con un boton para instalarlo sin usar la terminal.

Tambien se pueden colocar `ffmpeg.exe`, `ffprobe.exe` y `ffplay.exe` junto a
`novacut-windows.exe`.

Whisper es opcional y no se incluye por el tamaño de los modelos. Coloca el
ejecutable y el modelo en `whisper` junto a `novacut-windows.exe`, o en
`%LOCALAPPDATA%\NovaCut\Whisper`.

## Limites actuales

Los modos Color y Luminosidad importados de macOS se aproximan con composición
normal porque FFmpeg no ofrece equivalentes directos. Los proxies no se limpian
automáticamente. Whisper requiere una instalación local opcional y la calidad
depende del modelo. Conserva los medios originales y una copia de seguridad de
cualquier trabajo importante. Windows abre `.editorcito`, pero guardar de
vuelta al formato Mac sin perder elementos todavía no está habilitado.
