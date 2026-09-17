import AppKit
import Foundation

// Prueba síncrona: no bombea el runloop ni ejecuta tareas de autosave/preview.
// Sale antes de que el actor principal pueda lanzar esas tareas diferidas.
MainActor.assumeIsolated {
    var fallos = 0
    func comprobar(_ condicion: Bool, _ mensaje: String) {
        if condicion { print("  ok  \(mensaje)") } else { print("FALLO  \(mensaje)"); fallos += 1 }
    }
    let editor = EditorState()
    editor.playhead = 2
    editor.agregarSubtitulo()
    let original = editor.montaje.subtitulos!.first!
    comprobar(original.inicio == editor.timebase.frames(segundos: 2), "crea subtítulo en el cabezal")
    comprobar(editor.canUndo, "crear registra deshacer")
    editor.editarSubtitulo(original.id, texto: "Texto corregido")
    comprobar(editor.montaje.subtitulos?.first?.texto == "Texto corregido", "edita el texto")
    editor.undo()
    comprobar(editor.montaje.subtitulos?.first?.texto == original.texto, "deshacer restaura el texto")
    editor.redo()
    comprobar(editor.montaje.subtitulos?.first?.texto == "Texto corregido", "rehacer recupera el texto")
    let antes = editor.montaje
    comprobar(editor.desplazarSubtitulos(frames: 25), "aplica sincronización")
    comprobar(editor.montaje.subtitulos?.first?.inicio == original.inicio + 25, "mueve el inicio")
    editor.undo()
    comprobar(editor.montaje == antes, "sincronización se deshace en un paso")
    comprobar(!editor.desplazarSubtitulos(frames: .min), "rechaza ajuste inválido")
    comprobar(editor.montaje == antes && editor.canRedo, "rechazo conserva documento e historial")
    editor.eliminarSubtitulo(original.id)
    comprobar(editor.montaje.subtitulos?.isEmpty == true, "elimina el cue elegido")
    editor.undo()
    comprobar(editor.montaje == antes, "deshacer recupera el subtítulo eliminado")
    exit(fallos == 0 ? 0 : 1)
}
