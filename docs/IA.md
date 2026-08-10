# IA en Editorcito

## Selección

Proveedor y modelo se eligen en `Montaje con IA`.

- Local rápido: `Qwen3.5 9B`.
- Go económico: `DeepSeek V4 Flash` o `MiMo V2.5`.
- Go equilibrado: `DeepSeek V4 Pro`.
- Go razonamiento: `GLM-5.2`.
- Go máxima capacidad: `Kimi K3` o `Qwen3.8 Max`.

GPT 5.6 Luna no está disponible aún porque requiere la API Responses.

## Datos enviados

El modo local no usa red externa. OpenCode Go recibe la orden, nombres de clips,
entrada, salida y duración. No recibe frames, audio ni el archivo multimedia.

La credencial procede de la conexión existente de OpenCode y no se muestra ni se
guarda dentro del proyecto Editorcito. OpenCode Go puede consumir cuota.

## Edición con IA

- Vocabulario cerrado de 11 órdenes: recortar, mover, quitar, quitar cerrando,
  cortar, silenciar, ganancia, fundido, velocidad, marcador y etiquetar.
- Validación de índices y máximo de 30 acciones.
- Cancelación antes de aplicar.
- Descarte si el timeline cambió durante la inferencia.
- Confirmación humana con resumen y cantidad de cambios.
- Una transacción de undo para toda la propuesta aceptada.

## Transcripción y captions

El menú `Subtítulos` permite importar y exportar SRT. También puede preparar el
audio del medio seleccionado y usar `Speech` de macOS para transcripción on-device.
La aplicación exige el permiso `NSSpeechRecognitionUsageDescription` y solo usa el
modo local cuando el recognizer declara soporte en el dispositivo. Si no está
disponible, no envía el audio a un servicio remoto.

Los segmentos se convierten en cues de subtítulo, se pueden editar con el cabezal
en el inspector y se queman en el vídeo exportado. La eliminación automática de
silencios, la búsqueda por palabra y el rough cut basado en transcript siguen
pendientes.
