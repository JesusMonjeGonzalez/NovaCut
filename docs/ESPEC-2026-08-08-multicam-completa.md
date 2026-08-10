# ESPEC: Multicámara completa — visor, corte destructivo y sincronía manual (2026-08-08)

Fecha: 8 de agosto de 2026. Ámbito: modelo `src/ui/Timeline.swift`, app
`src/ui/App.swift` y el arnés. Cierra el tramo del roadmap «Ahora» que quedaba:
*vista multiángulo, corte destructivo de ángulo y sincronía manual*. No toca el
compositor: el render multicámara ya está verificado (tests/multicam).

## 1. Vista multiángulo en vivo

**Problema**: el visor actual (`multicamStrip`) enseña una tira de miniaturas
estáticas —el frame del medio, no lo que la cámara está grabando ahora—. El
cambio de ángulo funciona durante la reproducción, pero el operador no ve el
material de cada cámara en vivo: es un selector, no un visor.

**Solución**: el visor pasa a ser una cuadrícula de reproductores vivos, uno
por ángulo, sincronizados con el cabezal de programa:

- Cada ángulo es un `AVPlayer` con su `AVURLAsset`, mudo (el audio lo pone la
  composición del programa) y con `resizeAspect`, como el monitor principal.
- La posición de cada ángulo se deriva del cabezal: en el instante de grupo
  `t` (segundos de programa), el ángulo con desfase `d` enseña su material en
  `t − d`. Es exactamente la misma matemática que usa el constructor
  (`entradaOrigen = grupoT − desfase`), así que lo que se ve en el visor es lo
  que se oirá al exportar.
- En pausa, un `seek` del cabezal mueve todos los ángulos a su material
  correspondiente. En reproducción, todos arrancan a la vez y un temporizador
  de re-sincronización (cada 0,5 s) corrige la deriva de reloj de cada
  `AVPlayer` (los relojes independientes de AVFoundation no garantizan
  sincronía de muestra): si un ángulo se separa más de 2 frames del cabezal,
  se le hace `seek` a su posición correcta. Este re-sincronizado solo corre
  mientras el visor está abierto.
- El ángulo activo en el cabezal queda marcado; pulsar otro cambia el ángulo
  desde el cabezal, como hoy. El visor se construye bajo demanda (los
  `AVPlayer` solo existen mientras hay un clip multicámara seleccionado) y se
  libera al dejar de verlo o al reconstruir el preview.

**Verificación**: no es lógica pura (AVPlayer + timing). Se cubre con:
- `tests/multicam`: el cálculo de la posición de cada ángulo a partir del
  cabezal y los desfases se extrae como función pura `posicionDeAngulo(...)`
  y se comprueba contra los casos del constructor.
- Humo manual: el visor muestra las cámaras en vivo, marcadas y a tiempo.

## 2. Corte destructivo de ángulo («aplanar»)

**Problema**: hoy el único destino de un clip multicámara es el visor; no hay
forma de convertirlo en clips normales —el «flatten» de Premiere, el «commit»
de Resolve— para luego recortar, retimar o redistribuir cada tramo por
separado.

**Solución**: `LineaDeTiempo.aplanarMulticam(clipID:)` (modelo puro):

- Calcula `segmentos(duracion:)` del clip multicámara y, para cada tramo,
  crea un clip normal con:
  - `mediaID` = el ángulo del tramo,
  - `inicio` = `clip.inicio + tramo.desde`,
  - `duracion` = `tramo.hasta − tramo.desde`,
  - `entradaEnOrigen` = `clip.inicio + tramo.desde − desfase[ángulo]` —la
    misma cuenta del constructor, por eso el material aplanado suena y se ve
    exactamente en su sitio—,
  - el `enlace` del tramo: el vídeo y el audio del mismo tramo comparten
    enlace nuevo (los tramos distintos son independientes entre sí),
  - `multicam = nil` y los atributos del clip original (transformación,
    color, ganancia, keyframes **rebasados** al tramo, fundidos recortados a
    la duración del tramo).
- El clip original y su audio enlazado desaparecen; los tramos se insertan en
  la misma pista del clip original (y el audio enlazado en su pista).
- Un tramo sin material (desfase que lo deja fuera del archivo) se omite con
  aviso: el constructor ya lo habría marcado como crítico.

UI: botón «Aplanar multicámara» en el visor. El proyecto queda con clips
normales: deja de ser multicámara, a propósito («destructivo»).

**Verificación**: tests de modelo en `tests/main.swift` (lógica pura):
- los tramos salen con su ángulo, inicio, duración y `entradaEnOrigen`
  correctos (con y sin desfases);
- vídeo y audio del mismo tramo comparten enlace nuevo;
- los keyframes quedan rebasados al tramo;
- el clip original y su enlazado ya no están.

## 3. Sincronía manual

**Problema**: la sincronización inicial usa el onset de la forma de onda
(`inicioDeAudio`), que falla con material sin audio, música en vez de voz, o
grabaciones cuyo onset no es el mismo evento. Hasta hoy no había manera de
corregir el desfase de un ángulo a mano.

**Solución**: `LineaDeTiempo.ajustarDesfase(grupoID:medioID:delta:)` (modelo
puro, con tope en cero para no pedir material antes del arranque) y, en el
visor, controles de sincronía por ángulo:

- Botones «−1 / +1 frame» (con ⌥, ±10 frames) y el desfase actual en timecode.
- Un botón «Sincronizar por audio» que recalcula el onset como al crear el
  grupo (el arreglo automático sigue disponible).
- La corrección viaja en el grupo (fuente de verdad) y se aplica al render al
  momento: el visor en vivo se re-sincroniza con los desfases nuevos.

**Verificación**: tests de modelo en `tests/main.swift` (lógica pura):
- el delta se aplica al desfase del ángulo,
- el desfase nunca baja de cero,
- los desfases nulos se tratan como cero.

## Fuera de alcance

- Retime multicámara (velocidad distinta de 1 en un clip multicámara: sigue
  declarado no soportado con aviso crítico).
- Sincronía por markers/clap manual en la biblioteca (se hace desde el visor
  con los nudges).
- Aplanar solo un tramo (se aplana el clip completo; recortar después es
  edición normal).

## Verificación general

- `./probar.sh` verde (grupos Timeline + multicam enganchado).
- `./probar-multicam.sh` verde (render sobre archivos reales).
- `./build-mac.sh` compila la app completa.
