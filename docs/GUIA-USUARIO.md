# Editorcito: guía rápida de uso

## Flujo básico

1. Importa medios desde `Importar`, Finder o arrastrándolos a la ventana.
2. Selecciona un medio en la biblioteca. El monitor `Origen` permite reproducirlo y
   marcar `Entrada` y `Salida`.
3. Usa `Insertar` para abrir hueco o `Superponer` para escribir sobre el cabezal.
   Un vídeo con audio conserva su enlace A/V.
4. Selecciona `Programa` para revisar el montaje final.
5. Guarda el proyecto `.editorcito` y exporta desde el botón fijo de la barra superior.

## Timeline

- Arrastra el cuerpo de un clip para moverlo horizontalmente o entre pistas del mismo tipo.
- Arrastra un medio desde la biblioteca a la fila concreta del timeline donde quieras
  montarlo. Sin `⌥` abre hueco; con `⌥` superpone sobre el material existente.
- Arrastra los bordes para recortar. La herramienta activa determina normal, ripple,
  roll, slip o slide.
- `Shift` permite seleccionar varios clips en el timeline.
- El clic en una zona vacía de una pista coloca el cabezal en ese punto.
- El timeline sigue al cabezal durante la reproducción.
- Las transiciones aparecen con indicadores en los extremos del clip.

## Biblioteca y multicámara

- Usa el buscador y los bins para filtrar medios.
- Selecciona dos o más vídeos en la biblioteca para crear un grupo multicámara.
- La sincronización inicial usa el onset de la forma de onda cuando está disponible.
- El grupo crea ángulos en pistas separadas y un clip multicámara insertable
  desde el cabezal, con visor de ángulos en vivo en el monitor de programa:
  - **Cada cámara se ve en vivo**, sincronizada al instante de grupo del cabezal
    (el ángulo con desfase `d` enseña su material en `t − d`).
  - Pulsar una cámara **cambia el ángulo desde el cabezal**, también durante la
    reproducción. El ángulo que manda queda marcado y sus cortes se guardan en
    el clip.
  - **Sincronía manual**: `−` / `+` junto a cada cámara retrasa o adelanta su
    desfase un frame (10 con `⌥`), y «Sincronizar por audio» recalcula el onset
    automático. El desfase en curso se lee en timecode bajo la miniatura.
  - **Aplanar multicámara** convierte cada tramo de ángulo en un clip normal
    (el «flatten»): deja de ser multicámara y cada trozo se edita por separado.

## Rendimiento

- Abre el menú `Proxies` y elige `Generar proxies` para crear copias ligeras.
- Activa `Usar proxies en preview` para editar con esos archivos.
- Los proxies viven en `~/Library/Caches/Editorcito/Proxies` y no sustituyen los
  originales al exportar.

## Captions y transcripción

- `Subtítulos > Importar SRT` carga cues existentes.
- `Transcribir medio seleccionado` usa Speech on-device de macOS cuando el idioma y
  el dispositivo lo permiten.
- El cue bajo el cabezal se puede editar desde el inspector.
- El export de vídeo quema los captions y `Exportar SRT` conserva también el sidecar.

## Atajos

| Atajo | Acción |
|---|---|
| `J` / `K` / `L` | Reproducir atrás / pausa / reproducir delante |
| `Espacio` | Reproducir o pausar con el timeline enfocado |
| `I` / `O` | Entrada y salida de origen; en Programa marcan rango de trabajo |
| `V` / `C` | Selección / cuchilla |
| `B` / `N` | Ripple / roll |
| `Y` / `U` | Slip / slide |
| `H` | Mano |
| `S` / `M` | Imán / marcador |
| `⌘Z` / `⇧⌘Z` | Deshacer / rehacer |
| `⇧Z` | Ajustar montaje a la ventana |

Los atajos de una letra se activan cuando el timeline tiene el foco y no interceptan
la escritura en búsqueda, campos de texto o subtítulos.

## Títulos, retime, subclips y nidos

- **Títulos** (`⌘⌥T` o Montaje › Añadir título): un clip de título entra en la
  pista de trabajo en el cabezal; el inspector edita texto, tamaño, posición,
  color, contorno y fundido. Se quema sobre el vídeo al reproducir y al exportar.
  La forma del título puede ser **texto, rectángulo, elipse o línea**.
- **Retime**: además de la velocidad constante, el inspector tiene rampas
  (`Rampa en el cabezal` añade un keyframe de velocidad al 50 % desde el cabezal;
  `Congelar desde el cabezal` es el freeze frame). La velocidad interpola entre
  keyframes y el clip se parte en tramos al renderizar.
- **Subclips**: marca `Entrada` y `Salida` en el monitor de origen y pulsa
  `Subclip`: el recorte entra en la biblioteca como un medio con nombre, sin
  copiar el archivo.
- **Nidos** (`⌘G` con dos o más clips contiguos seleccionados): los clips se
  convierten en uno solo con su propia línea de tiempo interior, pre-renderizada
  a la caché. El clip anidado se mueve, recorta y retime como una pieza, y
  `Desanidar` (clic derecho) devuelve los clips originales.

## Composición, color y efectos

- **Modos de fusión**: en el inspector › Composición, 14 modos (multiplicar,
  pantalla, superponer…). Un clip en modo multiplicar oscurece lo que tiene
  debajo; en pantalla lo aclara.
- **Máscaras**: rectángulo o elipse con posición, tamaño, pluma (suavizado del
  borde) e invertida, en porcentaje del lienzo. Se ven en el monitor al instante.
- **Viñeta y desenfoque**: en el inspector › Efectos.
- **Curvas RGB**: «Curvas…» abre el editor con puntos de control arrastrables
  (doble clic añade punto). La curva de luminancia actúa como maestra sobre los
  tres canales y después cada canal con su propia curva.
- **Zoom del monitor**: botón con porcentaje en la barra del monitor
  (1× ajustado → 2×).
- **Audio scrubbing**: arrastra el cabezal y se oye el material a media
  velocidad por donde pasas.
- **Automatización visible**: la curva de ganancia se dibuja sobre los clips de
  audio del timeline (los keyframes se ven como rampas amarillas).

## Proyecto y entrega

- **Guardar versión** (`⌘⌥S`): instantánea con fecha y hora en la carpeta
  `Versiones/` junto al proyecto, sin tocar el archivo de trabajo.
- **Exportación**: además de MP4 H.264, vertical y audio, hay H.265/HEVC y
  ProRes 422 para intermedia.
- La exportación usa un archivo temporal: cancelar o fallar no destruye el destino
  anterior. Mientras exporta, `Cancelar` detiene solo el trabajo actual.
- Si se encuentra una recuperación automática, Editorcito pregunta si quieres cargarla
  o descartarla. Una recuperación aceptada queda marcada como pendiente de guardar.
- Los medios que no aparecen al abrir un proyecto quedan visibles como `offline`; usa
  `Revincular medio…` desde el menú contextual del clip.

## Limitaciones actuales

- Un medio VFR genera un aviso crítico antes de exportar; el conformado PTS aún requiere
  validación con corpus real.
- La caché de proxies se escribe de forma segura, pero todavía no tiene limpieza
  automática ni cancelación avanzada.
- El retime multicámara (velocidad distinta de 1 en un clip multicámara) está
  declarado no soportado: el constructor avisa antes de exportar.
- Marcha atrás (velocidad negativa) necesita renderizado previo y sigue
  declarado no soportado.
