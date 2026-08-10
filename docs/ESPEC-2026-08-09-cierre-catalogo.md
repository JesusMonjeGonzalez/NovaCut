# ESPEC: Tercera tanda Premiere — cierre del catálogo razonable (2026-08-09)

Fecha: 9 de agosto de 2026. Once piezas sobre la infraestructura existente.
Ninguna toca la arquitectura de reproducción ni de exportación.

## 1. Ruedas de color
- `RuedasDeColor`: sombras/medios/altas por canal (−1…1). `ColorDeClip.ruedas`.
- `RuedasDeColor.curvaDe` convierte las tres ruedas de un canal en una curva de
  tres puntos; `tablaDeRuedas` las interpola a la tabla RGB de `CIColorCurves`,
  aplicada tras las curvas propias en la cadena de color.
- UI: DisclosureGroup en Efectos con tres filas (una por rango) y sus canales.

## 2. Chroma key
- `ChromaKeyDeClip`: color de clave, tolerancia, suavizado, suprimir derrame.
  `Clip.croma`, aplicado por capa antes de mezclar.
- Señal de clave = dominancia del canal dominante de la pantalla sobre la
  media de los otros dos (`G − (R+B)/2`). CIColorDistance no existe en macOS.
  La rampa de tolerancia decide el alfa; el derrame se resta con una matriz y
  se enmascara con el alfa invertido.
- UI en Composición con ColorPicker para el color de clave.

## 3. Imágenes en títulos
- `FormaDeTitulo.imagen` + `TituloDeClip.rutaDeImagen`. La quemadura dibuja un
  `CALayer` con la imagen en su relación de aspecto. «Añadir imagen…» en Montaje.

## 4. Smart bins
- `MediaBin.filtro`: `contiene(_:)` decide (VFR/Audio por token, cualquier
  otra palabra en el nombre). `mediosVisibles` consulta el bin inteligente
  seleccionado.

## 5. Historia de deshacer
- `LineaDeTiempo.describirCambio(antes:después:)`: descripción por diff.
- `EditSnapshot.descripcion`; `commit` la genera. `historiaDeEdicion` se deriva
  de la pila (no puede desincronizarse). Panel en el inspector; `deshacerHasta`.

## 6. Combinar proyectos
- `importarOtroProyecto`: lee otro `.editorcito`, remapea medios duplicados,
  añade los nuevos y pega el montaje al final del actual por tipo de pista.

## 7. EDL
- `LineaDeTiempo.edl`: CMX3600. Eventos ordenados por inicio (vídeo antes que
  audio), reels a 8 caracteres, `* FROM CLIP NAME`, FCM según drop frame.
  `String(format:)` con `%s` + Strings de Swift = crash; se compone por piezas.

## 8. Pistas expandibles
- Chevron en cabeceras de audio: 58 ↔ 96 px.

## 9. Medidor en vivo
- `CadenaDeMezcla.claveDeMedidor` + `MedidorEnVivo` (registro con cerrojo,
  escrito por el tap). Barra en el inspector a 10 Hz.
- Un tap de identidad universal silencia la exportación (verificado): el
  medidor solo cubre pistas con tap ya existente (procesamiento o paneo).

## 10. Sidechain selectivo
- `Pista.fuenteDeDucking`; el compositor usa la pista elegida para los rangos
  de ducking (`rangosDe(_:)`), con la primera de audio como convención.

## 11. Pantalla completa
- Botón en la barra del monitor: `NSWindow.toggleFullScreen`.

## Fuera de alcance

- Buses de mezcla, denoise, multibanda, reverb/delay, time-stretch/pitch.
- Múltiples secuencias, FCPXML/XML, proxies por clip, smart bins por etiqueta.
- Medidor en vivo en pistas sin tap (compromiso con la exportación).

## Verificación

- `./probar.sh`: 16 grupos verdes (modelo + compositor-color ampliados).
- `./build-mac.sh`: 0 errores, instalada.
