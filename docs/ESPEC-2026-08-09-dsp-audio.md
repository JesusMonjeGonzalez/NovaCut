# ESPEC: Cuarta tanda — DSP de audio pesado (2026-08-09, noche)

Fecha: 9 de agosto de 2026. Cuatro piezas sobre la `CadenaDeMezcla` existente,
lo que la tanda anterior dejó fuera de alcance a propósito. Ninguna toca la
arquitectura de reproducción ni de exportación: el tap de mezcla ya viaja en el
`mezclaDeAudio`, así que reproducir = exportar = medir sigue siendo una sola
cadena.

## Orden de la cadena (declarado y fijo)

**Puerta de ruido → EQ → multibanda → compresor → limiter → reverb → retardo →
paneo.** Los efectos van detrás del limiter a propósito: su mezcla no se vuelve
a comprimir. El paneo cierra la cadena como siempre.

## 1. Puerta de ruido

- `PuertaDeRuidoDePista`: umbral dBFS, ataque, soltura y profundidad (cuánto se
  atenúa por debajo del umbral; 0,01 = −60 dB).
- La decisión se toma sobre la **envolvente de pico de la entrada** (pico
  instantáneo con caída de 20 ms), no sobre la muestra: una senoide cruza el
  umbral dos veces por periodo y decidir por muestra abriría y cerraría la
  puerta a cada cruce —el «chatter», cazado en el arnés con un tono que perdía
  el 40 % de nivel—. La ganancia se suaviza con ataque/soltura exponenciales
  como el compresor.

## 2. Compresor multibanda

- `CompresorMultibandaDePista`: tres bandas (graves/medios/agudos) con su
  umbral, ratio y activación; `BandaDeMultibanda`. Cruces **fijos y
  declarados**: 250 Hz y 4 kHz (escuela Fairlight/multibanda).
- La separación es Linkwitz-Riley de 24 dB/octava (dos secciones de Butterworth
  encadenadas por cruce), pero la banda media es la **diferencia** de los dos
  pasos bajos y la aguda lo que sobra de la entrada: la reconstrucción es
  exacta por construcción (verificado muestra a muestra con ratio 1).
- Cada banda tiene su detector enlazado (un detector por banda para todos los
  canales) y la misma ley de ganancia que el compresor de pista.

## 3. Retardo

- `RetardoDePista`: tiempo (0,01…2 s, recortado), realimentación y mezcla.
  Anillo por canal con el tamaño exacto del tiempo: la lectura es el propio
  índice de escritura, donde vive la muestra de hace exactamente `tiempo`.
- El paréntesis de `(retardo?.tiempoEnSegundos ?? 0.3) * fs` es obligatorio:
  `??` liga más flojo que `*` y sin él el anillo salía de 16 muestras (el eco
  llegaba en un suspiro, cazado en el arnés).

## 4. Reverb

- `ReverbDePista`: tamaño (0,05…1, sala a cabina) y mezcla. `tiempoDeCaida` =
  0,4 + tamaño·2 s.
- Schroeder clásico: cuatro peines en paralelo (retardos de la literatura
  escalados a la frecuencia real) y dos paso-todo en serie. La realimentación
  de cada peine sale del tiempo de caída: g = 10^(−3D/(t60·fs)).
- **Cada peine tiene su propia historia**: compartir un anillo mezclaba los
  estados y el eco sonaba antes de su tiempo (cazado en el arnés). Los
  paso-todo usan un anillo doble del tamaño exacto de su retardo (historia de
  entrada y de salida, el paso-todo de dos memorias, exacto).

## Fuera de alcance

- Buses de mezcla y envíos (necesitan otra arquitectura de tap).
- Time-stretch/pitch (cambia la línea de tiempo; el tap post-decode no puede).
- Denoise espectral real, de-esser por análisis.
- Múltiples secuencias, FCPXML/XML, proxies por clip.

## Verificación

- `./probar.sh`: grupo Mixer ampliado a 20 comprobaciones nuevas (puerta,
  multibanda con reconstrucción exacta e independencia de bandas, retardo con
  la serie de ecos, reverb con cola y tiempo, cadena completa a la vez, JSON
  con los parámetros nuevos).
- `./build-mac.sh`: 0 errores, instalada en /Applications.
