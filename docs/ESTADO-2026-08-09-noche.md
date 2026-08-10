# Estado de Editorcito — 9 de agosto de 2026 (noche)

Cuarta tanda: **DSP de audio pesado**, lo que la tercera tanda dejó fuera de
alcance a propósito. Cuatro efectos de inserción sobre la `CadenaDeMezcla`
existente, que ya corría igual en reproducción y exportación (la regla de la
casa: reproducir = exportar = medir).

## Cerrado hoy

### 1. Puerta de ruido
- `PuertaDeRuidoDePista`: umbral, ataque, soltura y profundidad. La decisión se
  toma sobre la envolvente de pico de la entrada (caída de 20 ms): decidir por
  muestra abría y cerraba la puerta a cada cruce de la senoide —el «chatter»,
  cazado en el arnés con un tono que perdía el 40 % de nivel—.
- Verificado: ruido a −60 dB cae 60 dB, el tono a −20 pasa entero.

### 2. Compresor multibanda
- Tres bandas (graves/medios/agudos) con cruces fijos 250 Hz / 4 kHz, cada una
  con umbral, ratio y activación. Separación Linkwitz-Riley de 24 dB/octava con
  la banda media como diferencia de pasos bajos y la aguda como resto de la
  entrada: **la reconstrucción es exacta por construcción** (verificado muestra
  a muestra con ratio 1).
- Verificado: los agudos comprimen un tono de 8 kHz y los graves no lo tocan;
  los graves comprimen un 100 Hz (−10,5 dB) sin mover medios ni agudos.

### 3. Retardo
- `RetardoDePista`: tiempo (0,01…2 s), realimentación y mezcla. Anillo por
  canal del tamaño exacto del tiempo. Un error de precedencia del `??` con `*`
  dejaba el anillo en 16 muestras —cazado en el arnés—.
- Verificado: la serie de ecos 0,5 → 0,25 → 0,125 con realimentación 0,5.

### 4. Reverb (Schroeder)
- `ReverbDePista`: tamaño y mezcla; cuatro peines en paralelo y dos paso-todo
  en serie, con los retardos clásicos escalados a la frecuencia real. Cada
  peine con su propia historia: compartir anillo mezclaba los estados y el eco
  sonaba antes de su tiempo (cazado en el arnés).
- Verificado: la cola decae, nada suena antes del peine más corto, y el tamaño
  1 conserva ~4000× más energía final que el 0,3.

### 5. UI y modelo
- Presets en el menú contextual «Procesamiento» de las pistas de audio y
  mandos completos en el inspector (Mezclador): umbral/profundidad de la
  puerta, umbral/ratio por banda del multibanda, tamaño/mezcla de la reverb y
  tiempo/realimentación/mezcla del retardo. Todo Codable con el proyecto y
  opcional por compatibilidad (`tieneProcesamientoDeAudio` unifica el criterio
  del tap).

## Verificación

- `./probar.sh`: suite completa de comprobaciones del modelo, composición, color,
  DSP, multicámara y retime, sin fallos reportados.
- `./build-mac.sh`: 0 errores; bundle `build/Editorcito.app` generado para arm64.

## Pendiente

- **Gate P0 (corpus VFR)**: sigue abierto — ~30 casos reales en
  `tests/corpus`. Sin corpus no se dice «editor».
- Humo manual de esta tanda: puerta limpiando una grabación real, multibanda
  sobre música, reverb/retardo a la escucha en reproducción y exportación.
- Buses de mezcla y envíos (necesitan otra arquitectura de tap), time-stretch/
  pitch, denoise espectral real.
- Infraestructura: múltiples secuencias, FCPXML/XML, proxies por clip, editar
  dentro de clips anidados.

## Riesgos conocidos

- Los cruces del multibanda son fijos (250 Hz / 4 kHz): un tono cerca del cruce
  se reparte entre dos bandas y la compresión de una sola se nota menos —es el
  comportamiento esperado de un Linkwitz-Riley, no un defecto.
- La reverb es monaural por canal (sin cross-feed entre canales): la imagen
  estéreo del efecto es la del material.
- El retardo se recorta a 2 s: más allá es retardo de ping-pong, que necesita
  otra pista.

## Cierre de fiabilidad y UX

### Seguridad de entrega

- La exportación de películas, proxies y nidos escribe primero en un archivo temporal
  oculto y sustituye el destino solo después de un estado `completed`.
- La barra inferior ofrece cancelación de la exportación actual. Una cancelación o un
  fallo limpia el temporal y conserva el archivo anterior.
- Las versiones usan segundos y sufijo incremental para no sobrescribirse si se guardan
  varias en el mismo instante.

### Proyecto y medios

- La recuperación automática ya no se aplica durante la inicialización: aparece una
  confirmación con nombre y fecha aproximada.
- Abrir o importar un proyecto conserva los medios no localizados como elementos offline;
  sus clips no desaparecen y se pueden revincular desde el timeline.
- Revincular conserva subclips y fuerza una reconstrucción del preview para no reutilizar
  una composición hecha con el archivo anterior.
- Cambiar entre original y proxy invalida también el preview incremental.

### Edición y audio

- La cuchilla sobre un clip enlazado corta vídeo y audio en el mismo frame.
- Trim, slip, slide y cambio de velocidad comparten el delta dentro del grupo A/V.
- Keyframes y rampas se rebajan al partir o vaciar un rango, incluyendo el estado
  efectivo en el nuevo inicio.
- Las transiciones se aplican a ambos lados del corte cuando los clips son contiguos.
- El tap DSP usa el número real de canales y adapta buffers intercalados y no intercalados.

### Privacidad y usabilidad

- La IA local puede usar el frame del cabezal; OpenCode Go recibe solo texto y metadatos
  del montaje, nunca frames ni audio.
- El drag and drop de biblioteca a timeline respeta la fila bajo el puntero.
- Los clips exponen nombre, rango, estado offline y acción de selección a accesibilidad.
- Un medio VFR genera un aviso crítico antes de exportar porque todavía no existe
  conformado PTS completo.

## Estado crítico al cierre

- **Mejoras aplicadas:** exportación segura, cancelación, recuperación confirmada,
  offline/relink, invalidación de preview, edición A/V, rebasing de animación, DSP por
  canales, privacidad de IA, drop por pista y accesibilidad básica.
- **Pendientes P0:** corpus real VFR/PTS, pruebas E2E de recovery/relink/export cancelado,
  pruebas con audio mono/planar/5.1 y humo manual de los flujos nuevos.
- **Pendientes P1:** re-render automático de nidos al abrir, migraciones antiguas más
  amplias, proxies cancelables y limpieza de caché, captions profesionales y UI tests.
