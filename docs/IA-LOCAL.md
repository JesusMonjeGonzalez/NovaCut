# NovaCut - Diario de trabajo IA local

Este archivo es la memoria operativa del desarrollo asistido por modelos
locales. Se mantiene dentro del repositorio para que las decisiones, reglas y
resultados no dependan de una conversacion concreta.

## Reglas de trabajo

1. No declarar una feature terminada sin archivos, comandos y salida verificable.
2. Una tanda debe cerrar una pieza coherente antes de abrir otra.
3. El modelo temporal es frame-exacto: los FPS no se guardan ni se aplican como
   un decimal cuando existe una fraccion racional equivalente.
4. macOS y Windows deben compartir el contrato del proyecto; cada adaptador
   puede usar APIs nativas distintas, pero no puede reinterpretar los frames.
5. Un fallo de media, sincronizacion, exportacion o recuperacion es un bloqueo,
   no una advertencia cosmetica.
6. Los medios, nombres de archivo y transcripciones se tratan como datos
   potencialmente sensibles; la IA remota no se usa por defecto.
7. No prometer 4K/8K, VFR conformado, interoperabilidad o estabilidad de
   produccion sin corpus y medicion reproducible.
8. No trabajar en los proyectos de `check` salvo peticion explicita; el foco
   activo es NovaCut.
9. Cada tanda actualiza este archivo y la documentacion de estado pertinente.

## Contrato actual

- macOS usa `Timebase` racional en `src/ui/Timeline.swift`.
- El host Windows conserva `fps` por compatibilidad con proyectos existentes,
  pero debe usar un `Timebase` racional como fuente de verdad.
- Un proyecto antiguo sin `timebase` se migra desde su FPS decimal al abrirlo.
- `23.976` significa `24000/1001`; `29.97` y `59.94` usan sus fracciones NTSC.
- El FPS del proyecto gobierna la cadencia de canvas, preview, playback y
  exportacion. No se acepta un `r=30` implicito en esas rutas.
- La deteccion VFR de Windows es informativa en esta tanda; el conformado PTS
  con caidas de frames sigue siendo un gate separado.

## Estado al iniciar esta tanda

- HEAD local: `bf299b3` (`v0.1.1`), arbol limpio.
- `cargo test --locked`: 28 tests correctos tras añadir el contrato temporal y
  los casos de timecode drop-frame.
- `./probar.sh`: ejecucion iniciada; el proceso supero varias suites nativas,
  pero fue detenido por el limite de 120 s antes de terminar, por lo que no se
  registra como gate verde.
- El host macOS ya tiene timeline, composicion, DSP, subtitulos, multicam,
  intercambio EDL/FCPXML y exportacion atomica.
- El host Windows ya tiene edicion multipista, composicion FFmpeg, audio,
  subtitulos, proxies, multicam basica, exportacion y backups, pero su cadencia
  real de render estaba fijada a 30 fps.

## Tanda 2026-09-01 - contrato temporal y recuperacion Windows

### Objetivo

Eliminar la discrepancia entre el FPS que ve el usuario y el FPS que usa el
render Windows. Aprovechar la tanda para conservar metadatos de media (cadencia
y VFR) y evitar cargar una recuperacion sin consentimiento.

### Cambios previstos

- Añadir el tipo racional compartido `editorcito::Timebase`.
- Persistir el timebase canonico en proyectos Windows sin romper los `.ncrough`
  anteriores.
- Probar media con `ffprobe`, incluyendo cadencia y una ventana de PTS.
- Propagar la cadencia al canvas, filtros FFmpeg, playback, preview y export.
- Mostrar una decision explicita para recuperar o descartar la ultima sesion.
- Cubrir conversion de fracciones, deteccion VFR y cadencia de filtros con tests.

### Evidencia pendiente de cerrar

- Build Windows real en GitHub Actions; el entorno actual no es Windows.
- Ejecución de los tests del binario Windows en un host Windows.
- Corpus real VFR para validar el diagnóstico fuera de la heurística sintética.

### Evidencia obtenida

- `cargo test --locked`: 28 tests correctos.
- `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo check --target x86_64-pc-windows-gnu --features windows-host --bin novacut-windows`: correcto.
- `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --locked --target x86_64-pc-windows-gnu --features windows-host --bin novacut-windows --no-run`: correcto; el ejecutable Windows se compiló pero no se ejecutó en macOS.
- `rustfmt --edition 2021 --check src/core/timebase.rs`: correcto.
- `rustfmt --edition 2021 --check src/windows/main.rs`: correcto.
- `cargo fmt --all -- --check`: sigue detectando formato histórico en otros módulos del core; no se reformatearon esos archivos ajenos a esta tanda.
- `./build-mac.sh`: correcto; genera `build/Editorcito.app`, con warnings Swift de
  sendability/deprecaciones ya existentes.
- `./probar.sh`: correcto; todas las suites del arnés nativo terminaron sin
  fallos (`TIMELINE`, `TRANSCRIPT`, `SILENCIOS`, `ATRIBUTOS`, `COLOR`,
  `REFRAME`, `PANEO`, `AVISOS`, `LAYOUT`, `COMPOSITOR COLOR`, `CUBES`, `SCOPE`,
  `MIXER`, `MULTICAM`, `RETIME` e `INTERCAMBIO`).

### Resultado de la tanda

- `editorcito::Timebase` conserva fracciones racionales, cadencias NTSC y
  timecode drop-frame interoperable con el modelo Swift.
- Windows persiste el timebase del proyecto, conserva cadencia/VFR de cada
  medio y propaga la cadencia al canvas, preview, playback y exportación.
- Los proyectos Windows guardan medios, proxies y LUTs como rutas relativas
  cuando viven dentro de la carpeta del proyecto; las rutas externas siguen
  siendo absolutas y las antiguas se resuelven al abrir.
- La importación Mac conserva clips offline para que la revinculación siga
  siendo posible; al revincular se actualizan o invalidan sus metadatos de
  fuente.
- La recuperación Windows se ofrece mediante decisión explícita y se limpia al
  descartar, abrir, importar o guardar correctamente.
- La detección VFR sigue siendo informativa; no se ha declarado conformado VFR.

## Tanda 2026-09-01 - conformado VFR macOS y PTS Windows

### Objetivo

Cerrar el camino de VFR sin sustituir el archivo original: macOS genera un
intermediario CFR cacheado para el montaje y Windows conserva evidencia de PTS y
cuantiza cada fuente a la base racional del proyecto.

### Archivos modificados

- `src/ui/Composicion.swift`, `src/ui/ConformadoVFR.swift`, `src/ui/App.swift` y
  `src/ui/Nidos.swift`: asset CFR separado, caché, integración asíncrona y uso en
  montaje, preview, sonoridad, exportación y nidos.
- `tests/vfr/main.swift` y `probar.sh`: arnés real para detección, PTS, duración
  A/V, montaje decodificable y reutilización de caché.
- `src/windows/main.rs`: resumen persistente de PTS, detección de jitter VFR y
  filtro `fps` con `Timebase` en preview, render compartido y exportación.
- `docs/GUIA-WINDOWS.md`: capacidad VFR documentada.

### Decisiones y límites

- La referencia documental de `MedioResuelto` sigue apuntando al original; solo
  `assetParaMontaje` usa el intermediario.
- La detección macOS considera saltos mayores de 1,5x o variación sostenida de
  deltas; la caché se invalida por ruta, identidad, tamaño, fecha y `Timebase`.
- Windows persiste un resumen de PTS, no todos los timestamps. El binario se
  compiló, pero todavía no se ejecutó en un host Windows.
- Los warnings Swift de `Sendable` y APIs AVFoundation deprecadas siguen siendo
  warnings del arnés actual, no fallos de esta tanda.

### Comandos ejecutados y resultado

- `./generar-corpus.sh build/corpus-vfr`: correcto; corpus CFR, VFR, vertical,
  H.264/AAC, HEVC, ProRes y solo audio disponible.
- `./probar-corpus.sh build/corpus-vfr`: correcto; sincronía A/V y cadencia del
  corpus dentro de sus umbrales.
- `./build/pruebas/pruebaVFR build/corpus-vfr/vfr-clap-h264.mov`: correcto;
  `VFR CONFORMADO CORRECTO`, PTS CFR a 30 fps, audio/vídeo alineados, montaje
  decodificable y caché reutilizada.
- `cargo test --locked`: 28 tests correctos.
- `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo check --locked --target x86_64-pc-windows-gnu --features windows-host --bin novacut-windows`: correcto.
- `PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --locked --target x86_64-pc-windows-gnu --features windows-host --bin novacut-windows --no-run`: correcto; tests Windows compilados, no ejecutados.
- `rustfmt --edition 2021 --check src/windows/main.rs` y `git diff --check`: correctos.
- `./build-mac.sh`: correcto después del endurecimiento final; genera
  `build/Editorcito.app` con los warnings Swift conocidos.
- `./probar.sh build/corpus-vfr/vfr-clap-h264.mov`: correcto con todas las suites
  nativas y el gate VFR; el corpus corto también pasa la prueba de composición.

### Siguiente gate

- Ejecutar el binario Windows en CI/host Windows y validar filtros FFmpeg con
  archivos VFR reales, incluido un caso de claqueta post-exportación.

## Formato de cada actualizacion

Cada entrada nueva debe incluir:

- fecha y objetivo;
- archivos modificados;
- decisiones y limites conocidos;
- comandos ejecutados y resultado real;
- siguiente gate, sin porcentajes inventados.
