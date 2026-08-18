# Editorcito: formatos y límites reales

Revisado el 3 de agosto de 2026 sobre macOS y AVFoundation.

## Regla de compatibilidad

`MP4`, `MOV` o `M4V` son contenedores. La posibilidad de editar depende del codec
de cada pista y de los decodificadores instalados en macOS. Editorcito valida el
archivo con AVFoundation antes de incorporarlo y no confía solo en la extensión.

Formatos habituales que macOS admite:

- Vídeo H.264/AVC y HEVC/H.265 en MP4, MOV o M4V.
- Apple ProRes en MOV.
- Audio AAC, ALAC y PCM en contenedores reconocidos.
- MP3, WAV, AIFF, M4A y otros audios que AVFoundation declare reproducibles.

Pueden fallar:

- MKV, WebM y AVI aunque contengan un codec conocido.
- AV1, VP9 u otros codecs sin decoder del sistema correspondiente.
- Vídeo con DRM, archivos truncados o cabeceras dañadas.
- Grabaciones con una duración o tabla de pistas inválida.
- Medios en nube que todavía no se hayan descargado completamente.

## Tamaño y duración

No existe un límite fijo impuesto por Editorcito. La importación lee metadatos y
no copia el vídeo completo a memoria. Los límites prácticos son:

- Espacio libre para exportación, caché y futuros proxies.
- Velocidad del disco y complejidad del codec.
- Memoria necesaria por los frames decodificados, no por el tamaño total del archivo.
- Duración y número de clips que debe componer AVFoundation.

Se verificó directamente el archivo local de prueba:

- Tamaño: 3.406.311.653 bytes, aproximadamente 3,2 GB.
- Duración: 5.931,051 segundos.
- Imagen: H.264, 1920 x 1080.
- Audio: AAC estéreo, 48 kHz.
- AVFoundation: reproducible y exportable, una pista de vídeo y una de audio.

El archivo está marcado como no optimizado para streaming. Esto no impide
importarlo, pero puede hacer más lentos algunos seeks porque sus índices no están
organizados para inicio rápido.

## Comportamiento de la aplicación

- El selector acepta contenido audiovisual, no una lista cerrada de extensiones.
- También se pueden arrastrar archivos a la ventana.
- Se muestra progreso mientras se analizan.
- Cada rechazo indica el archivo y la razón detectada.
- Los medios duplicados no se vuelven a añadir.
- Los proyectos se abren aunque falte un original; el clip queda offline y puede
  revincularse desde el inspector.

## Preview y proxies

Los proxies se generan bajo demanda desde el menú `Proxies`. Se guardan fuera del
proyecto en:

```text
~/Library/Caches/Editorcito/Proxies/<uuid>.mp4
```

Se usan únicamente para preview. La cola de exportación vuelve a los medios
originales para no degradar el resultado. La limpieza automática, cancelación de
generación y límite de espacio de la caché siguen pendientes.

## Exportación

La aplicación ofrece estos presets:

- MP4 H.264 de máxima calidad.
- MP4 HEVC de máxima calidad.
- MP4 vertical 1080x1920.
- MOV Apple ProRes 422.
- Audio M4A.
- Master MOV de máxima calidad.

También puede exportar el rango de trabajo y mantener varios trabajos en cola. Los
captions se queman en el vídeo y pueden exportarse además como SRT.

## VFR

Editorcito marca medios cuya pista no declara una duración mínima de frame estable
como `VFR` en la biblioteca. El timeline y `CMTime` trabajan con tiempos racionales,
pero todavía falta validar conformado y sincronización PTS con un corpus amplio de
grabaciones VFR de móviles, OBS y grabadoras externas.

## Próximo gate

Antes de afirmar compatibilidad profesional se necesita un corpus legal con MP4,
MOV, M4V, audio, H.264, HEVC, ProRes, CFR, VFR, vídeo vertical, múltiples canales,
archivos grandes, archivos truncados y codecs no soportados. Cada caso debe cubrir
importación, seek, preview, guardado, reapertura y exportación.

## Corpus (estado actual)

`generar-corpus.sh` genera el corpus golden con claqueta (flash + pitido
sincronizados en cinco puntos) y `probar-corpus.sh` lo mide:

- **Sincronía A/V**: el flash se muestrea a 4× la cadencia y el pitido en
  ventanas de 10 ms sobre un perfil de energía de toda la pista. Tolerancia
  1,5 frames. El golden pasa con menos de 1,2 frames en el peor caso.
- **Cadencia**: huecos reales sobre la cadencia mediana (los B-frames no son
  huecos; un paquete PCM largo del muxer tampoco). El golden pasa sin huecos.
- Las grabaciones reales con caídas de frames (p. ej. Screen Recording de
  macOS) fallan la cadencia con razón y se detectan como VFR al importar.

## Intercambio con otros editores

El montaje sale hacia la industria por dos formatos desde Archivo:

- **EDL (CMX 3600)**: cortes con timecodes de origen y montaje, canales
  V/A1/A2, velocidad constante como efecto de movimiento. Se anotan como
  comentarios `*` lo que el formato no lleva: fundidos, rampas, títulos,
  anidados y multicámara.
- **FCPXML 1.11**: secuencia de una espina, enlaces A/V unidos, velocidades
  constantes, capas apiladas recortadas a sus tramos libres y una `<note>`
  con las limitaciones. Títulos, anidados y multicámara se omiten con nota.

Ambos son funciones puras verificadas en `tests/intercambio`. La exportación
no degrada los medios: los archivos originales se referencian por ruta.
