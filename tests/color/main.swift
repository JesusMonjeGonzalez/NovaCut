import CoreImage
import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

print("— corrección de color —")

// El color neutro debe pasar la imagen sin cambiarla: es la comprobación de que
// la cadena de filtros no introduce deriva.
let gris = CIImage(color: CIColor(red: 0.5, green: 0.5, blue: 0.5)).cropped(to: CGRect(x: 0, y: 0, width: 16, height: 16))
let neutro = CompositorDeColor.aplicar(ColorDeClip.neutro, a: gris)
comprobar(neutro != gris, "el filtro devuelve una imagen (no nil)")

let brillante = CompositorDeColor.aplicar(ColorDeClip(exposicion: 40), a: gris)
comprobar(brillante != neutro, "subir la exposición cambia la imagen")

var clip = Clip(mediaID: UUID(), nombre: "C", inicio: 0, duracion: 100, entradaEnOrigen: 0)
comprobar(clip.color.esNeutro, "un clip nuevo nace con color neutro")
clip.color.exposicion = 30
comprobar(!clip.color.esNeutro, "tocar la exposición deja de ser neutro")

let cadena = CompositorDeColor.aplicar(ColorDeClip(temperatura: 50, altas: 20), a: gris)
comprobar(cadena != neutro, "temperatura y altas juntas cambian la imagen")

if fallos == 0 {
    print("COLOR CORRECTO")
} else {
    print("COLOR ROTO — \(fallos) fallos")
    exit(1)
}
