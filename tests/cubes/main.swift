import Foundation

// El parseador de LUTs `.cube` es lógica pura: se verifica sin archivos reales.
// El formato: `LUT_3D_SIZE N` + N³ líneas «r g b» (el índice azul varía más
// rápido), con comentarios y dominio opcional.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}

func floats(_ datos: Data) -> [Float] {
    datos.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
}

print("— cubo 3D de tamaño 2 —")
// 8 entradas: en cada una, R=G=B=indice/7 (una identidad tosca de 8 pasos).
var contenido = "LUT_3D_SIZE 2\n"
for i in 0..<8 {
    let v = Double(i) / 7.0
    contenido += String(format: "%.4f %.4f %.4f\n", v, v, v)
}
let cubo = ParseadorDeCubes.parsear(contenido: contenido)
comprobar(cubo != nil, "un cubo 3D válido se parsea")
if let cubo {
    comprobar(cubo.tamano == 2, "el tamaño es 2")
    let datos = floats(cubo.datos)
    comprobar(datos.count == 8 * 4, "8 entradas RGBA (\(datos.count))")
    // Entrada (r=1, g=0, b=1): índice = 1*4 + 0*2 + 1 = 5 → valor 5/7.
    let esperado = Float(5.0 / 7.0)
    comprobar(abs(datos[5 * 4] - esperado) < 0.001, "la entrada (1,0,1) cae en el índice 5")
    comprobar(datos[5 * 4 + 3] == 1, "el alfa va a 1")
    // El índice 0 es la entrada (0,0,0).
    comprobar(abs(datos[0] - 0) < 0.001, "la entrada (0,0,0) es la primera")
}

print("— dominio distinto de 0…1 —")
let conDominio = """
LUT_3D_SIZE 1
DOMAIN_MIN 0 0 0
DOMAIN_MAX 1 1 1
0.5 0.5 0.5
"""
if let cubo = ParseadorDeCubes.parsear(contenido: conDominio) {
    let datos = floats(cubo.datos)
    comprobar(abs(datos[0] - 0.5) < 0.001, "con dominio 0…1 el valor pasa tal cual")
} else {
    comprobar(false, "un cubo de una entrada se parsea")
}

let conDominioExtendido = """
LUT_3D_SIZE 1
DOMAIN_MIN 0 0 0
DOMAIN_MAX 2 2 2
1.0 1.0 1.0
"""
if let cubo = ParseadorDeCubes.parsear(contenido: conDominioExtendido) {
    let datos = floats(cubo.datos)
    comprobar(abs(datos[0] - 0.5) < 0.001, "con dominio 0…2, el 1.0 se normaliza a 0.5")
} else {
    comprobar(false, "un cubo con dominio extendido se parsea")
}

print("— tolerancia a comentarios y espacios —")
let conBasura = """
# LUT generada de prueba
LUT_3D_SIZE   2

0.0 0.0 0.0
0.142857 0.142857 0.142857
0.285714 0.285714 0.285714
0.428571 0.428571 0.428571
0.571429 0.571429 0.571429
0.714286 0.714286 0.714286
0.857143 0.857143 0.857143
1.0 1.0 1.0
"""
comprobar(ParseadorDeCubes.parsear(contenido: conBasura)?.tamano == 2,
          "los comentarios y los espacios sobrantes no molestan")

print("— LUT 1D —")
// Una LUT 1D de 2 puntos: f(0)=0 y f(1)=1 (identidad). La expansión a cubo
// aplica la curva a cada canal: la entrada (r,g,b) vale (f[r], f[g], f[b]).
let unidimensional = """
LUT_1D_SIZE 2
0.0 0.0 0.0
1.0 1.0 1.0
"""
if let cubo = ParseadorDeCubes.parsear(contenido: unidimensional) {
    let datos = floats(cubo.datos)
    comprobar(cubo.tamano == 2 && datos.count == 8 * 4, "la 1D se expande a un cubo de 2")
    // Entrada (r=1, g=1, b=0): índice = 1*4 + 1*2 + 0 = 6 → (1,1,0,1).
    comprobar(abs(datos[6 * 4] - 1) < 0.001 && abs(datos[6 * 4 + 1] - 1) < 0.001 && abs(datos[6 * 4 + 2]) < 0.001,
              "la entrada (1,1,0) vale (f[1], f[1], f[0]) = (1,1,0)")
} else {
    comprobar(false, "una LUT 1D se parsea")
}

print("— contenido inválido —")
comprobar(ParseadorDeCubes.parsear(contenido: "no es una LUT") == nil,
          "sin tamaño ni valores, nil")
comprobar(ParseadorDeCubes.parsear(contenido: "LUT_3D_SIZE 3\n0.5 0.5 0.5") == nil,
          "un cubo 3×3 con una sola entrada es inválido")

if fallos == 0 {
    print("CUBES CORRECTO")
} else {
    print("CUBES ROTO — \(fallos) fallos")
    exit(1)
}
