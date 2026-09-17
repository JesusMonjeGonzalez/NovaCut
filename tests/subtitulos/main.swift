import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("FALLO  \(mensaje)"); fallos += 1 }
}

let texto = "\u{feff}00:00:01,250-->00:00:03,000 X1:10 X2:20\r\nHola\r\nmundo\r\n \t\r\n2\r\n00:00:04.000 --> 00:00:05.500\r\nFin\r\n"
let cues = SubtitulosService.leerSRT(texto, timebase: .p25)
comprobar(cues.count == 2, "BOM, flechas sin espacios y separadores con espacios")
comprobar(cues.first?.texto == "Hola\nmundo", "conserva el texto multilínea")
comprobar(cues.first?.inicio == 31 && cues.first?.fin == 75, "redondea a la cadencia del proyecto")
comprobar(cues.last?.fin == 138, "acepta punto decimal y posiciones opcionales")

for tiempo in ["-1:00:00,000", "00:60:00,000", "00:00:60,000", "00:00:NaN", "00:00:inf",
               "1e9:00:00,000", "00:00:01.2.3", "00:00:01,0000", "999999999999999999999999:00:00,000"] {
    let srt = "1\n00:00:00,000 --> \(tiempo)\nNo debe importar\n"
    comprobar(SubtitulosService.leerSRT(srt, timebase: .p25).isEmpty, "rechaza tiempo inválido \(tiempo)")
}
comprobar(SubtitulosService.leerSRT("1\n00:00:02,000 --> 00:00:01,000\nInverso", timebase: .p25).isEmpty,
          "rechaza intervalos invertidos")
let ntsc = SubtitulosService.escribirSRT([Subtitulo(inicio: 8991, fin: 8992, texto: "Salto de minuto")], timebase: .ntsc30)
comprobar(ntsc.contains("00:05:00,000"), "el redondeo de milisegundos acarrea al minuto siguiente")
let exportado = SubtitulosService.escribirSRT(cues.reversed(), timebase: .p25)
let vuelta = SubtitulosService.leerSRT(exportado, timebase: .p25)
comprobar(vuelta.map(\.inicio) == cues.map(\.inicio) && vuelta.map(\.fin) == cues.map(\.fin), "round-trip conserva frames")
comprobar(vuelta.map(\.texto) == cues.map(\.texto), "exportación ordenada conserva texto")
let originales = [Subtitulo(inicio: 100, fin: 150, texto: "Hola\nmundo", estilo: "destacado"),
                  Subtitulo(inicio: 20, fin: 40, texto: "HOLA otra vez")]
let encontrados = SubtitulosService.buscar(originales, consulta: "  hola  ")
comprobar(encontrados.map(\.inicio) == [20, 100], "búsqueda ordenada sin distinguir mayúsculas")
comprobar(encontrados.map(\.id) == [originales[1].id, originales[0].id], "la búsqueda conserva la identidad de los cues")
comprobar(SubtitulosService.buscar(originales, consulta: "ausente").isEmpty, "búsqueda sin resultados")
comprobar(SubtitulosService.buscar(originales, consulta: " \n ").count == 2, "consulta vacía muestra todos")
comprobar(SubtitulosService.buscar(originales, consulta: "[a-z]").isEmpty, "la búsqueda no interpreta expresiones regulares")
do {
    let movidos = try SubtitulosService.desplazar(originales, frames: -20)
    comprobar(movidos.map(\.inicio) == [80, 0], "desplazamiento uniforme hasta el origen")
    comprobar(movidos.map(\.fin) == [130, 20], "desplazamiento conserva duraciones y solapes")
    comprobar(movidos.map(\.id) == originales.map(\.id) && movidos.map(\.estilo) == originales.map(\.estilo),
              "sincronización conserva IDs y estilos")
    comprobar(try SubtitulosService.desplazar(movidos, frames: 20) == originales, "desplazamiento reversible sin pérdida de frames")
    comprobar(try SubtitulosService.desplazar(originales, frames: 0) == originales, "cero es una operación neutra")
} catch { comprobar(false, "desplazamiento válido: \(error)") }
for frames: Int64 in [-21, .min, .max] {
    do { _ = try SubtitulosService.desplazar(originales, frames: frames); comprobar(false, "rechaza ajuste inválido \(frames)") }
    catch { comprobar(true, "rechaza ajuste inválido \(frames) sin tocar los originales") }
}
comprobar(originales.map(\.inicio) == [100, 20], "un fallo no aplica cambios parciales")
exit(fallos == 0 ? 0 : 1)
