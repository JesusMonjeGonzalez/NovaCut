import Foundation

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func igual<T: Equatable>(_ a: T, _ b: T, _ mensaje: String) {
    if a == b { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}

let media = UUID()

func montajeDePrueba() -> (LineaDeTiempo, UUID, [UUID]) {
    var t = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    var ids: [UUID] = []
    for i in 0..<3 {
        let c = Clip(mediaID: media, nombre: "C\(i)", inicio: Int64(i) * 100, duracion: 100, entradaEnOrigen: 0)
        ids.append(c.id)
        t.sobrescribir(c, enPista: v1, en: Int64(i) * 100)
    }
    return (t, v1, ids)
}

print("— timebase —")
let ntsc = Timebase.ntsc24
comprobar(abs(ntsc.fps - 23.976) < 0.001, "23,976 fps es 24000/1001")
igual(ntsc.tiempo(48).value, 48 * 1001, "CMTime exacto sin coma flotante")
igual(ntsc.tiempo(48).timescale, 24000, "escala del CMTime")
igual(Timebase.p25.timecode(25 * 61 + 7), "00:01:01:07", "timecode 25p")
igual(Timebase.p25.frames(timecode: "00:01:01:07"), 25 * 61 + 7, "timecode de vuelta")
igual(Timebase.p25.frames(timecode: "01:07"), 25 * 1 + 7, "timecode corto")
comprobar(Timebase.ntsc30.dropFrame, "NTSC 30 activa drop frame solo")
comprobar(!Timebase.p25.dropFrame, "25p no lleva drop frame")
igual(Timebase.ntsc30.timecode(1_798), "00:00:59;28", "drop frame mantiene las ultimas etiquetas del minuto")
igual(Timebase.ntsc30.timecode(1_800), "00:01:00;02", "drop frame salta 00 y 01 al cambiar de minuto")
igual(Timebase.ntsc30.timecode(17_982), "00:10:00;00", "drop frame alinea el primer bloque de diez minutos")
igual(Timebase.ntsc30.timecode(19_782), "00:11:00;02", "drop frame alinea los minutos posteriores")
comprobar(Timebase.ntsc30.frames(timecode: "00:11:00;02") == 19_782, "drop frame convierte timecode valido a frames")
comprobar(Timebase.ntsc30.frames(timecode: "00:01:00;00") == nil, "drop frame rechaza etiquetas inexistentes")
// Sin deriva: 100.000 frames sumados uno a uno son exactamente 100.000.
var acumulado: Int64 = 0
for _ in 0..<100_000 { acumulado += 1 }
igual(ntsc.segundos(acumulado), ntsc.segundos(100_000), "sin deriva al acumular")

print("— partir —")
var (t, v1, ids) = montajeDePrueba()
let creados = t.partir(en: 150)
igual(creados.count, 1, "partir crea un clip")
igual(t.pista(v1)!.clips.count, 4, "cuatro clips tras partir")
igual(t.pista(v1)!.clips[1].duracion, 50, "la mitad izquierda mide 50")
igual(t.pista(v1)!.clips[2].duracion, 50, "la mitad derecha mide 50")
igual(t.pista(v1)!.clips[2].entradaEnOrigen, 50, "la derecha avanza su entrada en origen")
igual(t.partir(en: 100).count, 0, "partir justo en un corte no hace nada")

print("— levantar y borrar con arrastre —")
(t, v1, ids) = montajeDePrueba()
t.levantar(ids[1])
igual(t.pista(v1)!.clips.count, 2, "levantar quita el clip")
igual(t.pista(v1)!.clips[1].inicio, 200, "levantar deja el hueco")

(t, v1, ids) = montajeDePrueba()
t.borrarConArrastre(ids[1])
igual(t.pista(v1)!.clips.count, 2, "borrar con arrastre quita el clip")
igual(t.pista(v1)!.clips[1].inicio, 100, "borrar con arrastre cierra el hueco")
igual(t.duracion, 200, "el montaje se acorta")

print("— insertar —")
(t, v1, ids) = montajeDePrueba()
let nuevo = Clip(mediaID: media, nombre: "N", inicio: 0, duracion: 40, entradaEnOrigen: 0)
t.insertar(nuevo, enPista: v1, en: 100)
igual(t.pista(v1)!.clips.count, 4, "insertar añade un clip")
igual(t.pista(v1)!.clips[1].duracion, 40, "el insertado va en su sitio")
igual(t.pista(v1)!.clips[2].inicio, 140, "lo de detrás se desplaza")
igual(t.duracion, 340, "el montaje crece lo insertado")

(t, v1, ids) = montajeDePrueba()
t.insertar(nuevo, enPista: v1, en: 150)
igual(t.pista(v1)!.clips.count, 5, "insertar en medio parte el clip")

print("— sobrescribir —")
(t, v1, ids) = montajeDePrueba()
let encima = Clip(mediaID: media, nombre: "E", inicio: 0, duracion: 50, entradaEnOrigen: 0)
t.sobrescribir(encima, enPista: v1, en: 120)
igual(t.duracion, 300, "sobrescribir no alarga el montaje")
igual(t.pista(v1)!.clips.count, 5, "el clip tapado queda partido a los dos lados")
comprobar(t.pista(v1)!.clips.contains { $0.inicio == 170 && $0.duracion == 30 }, "el resto del tapado sobrevive")

print("— recorte —")
(t, v1, ids) = montajeDePrueba()
var aplicado = t.recortar(ids[1], borde: .salida, delta: -30, modo: .normal, duracionDelMedio: 1000)
igual(aplicado, -30, "recorte normal aplicado")
igual(t.clip(ids[1])!.duracion, 70, "el clip se acorta")
igual(t.clip(ids[2])!.inicio, 200, "el vecino no se mueve en modo normal")

(t, v1, ids) = montajeDePrueba()
aplicado = t.recortar(ids[1], borde: .salida, delta: -30, modo: .ripple, duracionDelMedio: 1000)
igual(t.clip(ids[2])!.inicio, 170, "ripple arrastra la cola")
igual(t.duracion, 270, "ripple acorta el montaje")

(t, v1, ids) = montajeDePrueba()
aplicado = t.recortar(ids[1], borde: .entrada, delta: 20, modo: .ripple, duracionDelMedio: 1000)
igual(t.clip(ids[1])!.inicio, 100, "ripple por la entrada no mueve el clip")
igual(t.clip(ids[1])!.duracion, 80, "ripple por la entrada acorta")
igual(t.clip(ids[1])!.entradaEnOrigen, 20, "y avanza en el material original")
igual(t.clip(ids[2])!.inicio, 180, "la cola sube")

(t, v1, ids) = montajeDePrueba()
aplicado = t.recortar(ids[1], borde: .salida, delta: -30, modo: .roll, duracionDelMedio: 1000)
igual(t.clip(ids[2])!.inicio, 170, "roll mueve el corte")
igual(t.clip(ids[2])!.duracion, 130, "el vecino crece lo que el otro mengua")
igual(t.duracion, 300, "roll no cambia la duración total")

(t, v1, ids) = montajeDePrueba()
aplicado = t.recortar(ids[0], borde: .entrada, delta: -50, modo: .normal, duracionDelMedio: 1000)
igual(aplicado, 0, "no se puede entrar antes del principio del archivo")

(t, v1, ids) = montajeDePrueba()
aplicado = t.recortar(ids[2], borde: .salida, delta: 500, modo: .normal, duracionDelMedio: 150)
igual(aplicado, 50, "el recorte se detiene donde acaba el material")

print("— deslizar —")
(t, v1, ids) = montajeDePrueba()
aplicado = t.deslizarContenido(ids[1], delta: 40, duracionDelMedio: 1000)
igual(aplicado, 40, "slip aplicado")
igual(t.clip(ids[1])!.entradaEnOrigen, 40, "slip cambia el contenido")
igual(t.clip(ids[1])!.inicio, 100, "slip no mueve el clip")
igual(t.clip(ids[1])!.duracion, 100, "slip no cambia la duración")

(t, v1, ids) = montajeDePrueba()
aplicado = t.deslizarContenido(ids[1], delta: 5000, duracionDelMedio: 150)
igual(t.clip(ids[1])!.entradaEnOrigen, 50, "slip se detiene en el final del material")

(t, v1, ids) = montajeDePrueba()
aplicado = t.deslizarPosicion(ids[1], delta: 30, duracionDelMedio: 1000)
igual(aplicado, 30, "slide aplicado")
igual(t.clip(ids[1])!.inicio, 130, "slide mueve el clip")
igual(t.clip(ids[0])!.duracion, 130, "el anterior se alarga")
igual(t.clip(ids[2])!.inicio, 230, "el siguiente se acorta por delante")
igual(t.duracion, 300, "slide no cambia la duración total")

print("— mover entre pistas —")
(t, v1, ids) = montajeDePrueba()
let v2 = t.pistas.first { $0.nombre == "V2" }!.id
let a1 = t.pistas.first { $0.nombre == "A1" }!.id
t.mover(ids[1], aPista: v2, en: 500)
igual(t.pista(v1)!.clips.count, 2, "el clip sale de su pista")
igual(t.pista(v2)!.clips.count, 1, "y entra en la otra")
igual(t.clip(ids[1])!.inicio, 500, "en el frame pedido")

(t, v1, ids) = montajeDePrueba()
t.mover(ids[1], aPista: a1, en: 0)
igual(t.pista(v1)!.clips.count, 3, "no se puede mover vídeo a una pista de audio")

print("— enlace vídeo/audio —")
var enlazado = LineaDeTiempo.nueva(timebase: .p25)
let videoOrigen = enlazado.pistas.first { $0.nombre == "V1" }!.id
let videoDestino = enlazado.pistas.first { $0.nombre == "V2" }!.id
let audio = enlazado.pistas.first { $0.tipo == .audio }!.id
let enlace = UUID()
let videoEnlazado = Clip(mediaID: media, nombre: "vídeo", inicio: 40, duracion: 60, entradaEnOrigen: 0, enlace: enlace)
let audioEnlazado = Clip(mediaID: media, nombre: "audio", inicio: 40, duracion: 60, entradaEnOrigen: 0, enlace: enlace)
enlazado.sobrescribir(videoEnlazado, enPista: videoOrigen, en: 40)
enlazado.sobrescribir(audioEnlazado, enPista: audio, en: 40)
enlazado.mover(videoEnlazado.id, aPista: videoDestino, en: 120)
comprobar(enlazado.pista(videoDestino)?.clips.first?.inicio == 120, "mueve el vídeo enlazado a otra pista")
comprobar(enlazado.pista(audio)?.clips.first?.inicio == 120, "mantiene el audio enlazado en sincronía")
enlazado.levantar(videoEnlazado.id)
comprobar(enlazado.pista(audio)?.clips.isEmpty == true, "levantar quita también el audio enlazado")

var conAnimacion = LineaDeTiempo.nueva(timebase: .p25)
let videoAnimado = conAnimacion.pistas.first { $0.nombre == "V1" }!.id
let audioAnimado = conAnimacion.pistas.first { $0.tipo == .audio }!.id
let enlaceAnimado = UUID()
var clipAnimado = Clip(mediaID: media, nombre: "plano animado", inicio: 0, duracion: 100, entradaEnOrigen: 0, enlace: enlaceAnimado)
clipAnimado.keyframes = [
    ClipKeyframe(frame: 40, transformacion: .identidad, ganancia: 0),
    ClipKeyframe(frame: 80, transformacion: .identidad, ganancia: -6),
]
clipAnimado.rampasDeVelocidad = [RampaDeVelocidad(frame: 40, velocidad: 2)]
let audioAnimadoClip = Clip(mediaID: media, nombre: "voz", inicio: 0, duracion: 100, entradaEnOrigen: 0, enlace: enlaceAnimado)
conAnimacion.sobrescribir(clipAnimado, enPista: videoAnimado, en: 0)
conAnimacion.sobrescribir(audioAnimadoClip, enPista: audioAnimado, en: 0)
let mitades = conAnimacion.partir(en: 60, pistas: Set([videoAnimado]))
igual(mitades.count, 2, "la cuchilla enlazada crea la mitad de vídeo y audio")
igual(conAnimacion.pista(audioAnimado)!.clips.count, 2, "el audio enlazado también se parte")
let derechaAnimada = conAnimacion.pista(videoAnimado)!.clips.last!
igual(derechaAnimada.keyframes?.map(\.frame), [0, 20], "los keyframes se rebajan al nuevo inicio")
igual(derechaAnimada.rampasDeVelocidad?.first?.frame, 0, "la rampa se rebasa al partir")

print("— pistas bloqueadas —")
(t, v1, ids) = montajeDePrueba()
let indiceV1 = t.indiceDePista(v1)!
t.pistas[indiceV1].bloqueada = true
t.borrarConArrastre(ids[1])
igual(t.pista(v1)!.clips.count, 3, "una pista bloqueada no se toca")
igual(t.partir(en: 150).count, 0, "ni se parte")

print("— imán —")
(t, v1, ids) = montajeDePrueba()
igual(t.imantar(103, umbral: 8), 100, "se engancha al corte cercano")
igual(t.imantar(140, umbral: 8), 140, "y no al lejano")
t.marcadores.append(Marcador(frame: 250))
igual(t.imantar(247, umbral: 8), 250, "también se engancha a los marcadores")
igual(t.imantar(103, umbral: 8, excluyendo: [ids[0], ids[1]]), 103, "los clips excluidos no aportan imán")

print("— navegación —")
(t, v1, ids) = montajeDePrueba()
igual(t.corte(desde: 50, haciaDelante: true), 100, "siguiente corte")
igual(t.corte(desde: 150, haciaDelante: false), 100, "corte anterior")
comprobar(t.corte(desde: 300, haciaDelante: true) == nil, "no hay corte después del final")

print("— huecos —")
(t, v1, ids) = montajeDePrueba()
t.levantar(ids[1])
let h = t.hueco(enPista: v1, en: 150)
comprobar(h != nil && h!.inicio == 100 && h!.fin == 200, "encuentra el hueco")
t.cerrarHuecos(enPista: v1)
igual(t.pista(v1)!.clips[1].inicio, 100, "cerrar huecos pega los clips")
igual(t.duracion, 200, "y acorta el montaje")

print("— serialización —")
(t, v1, ids) = montajeDePrueba()
t.marcadores.append(Marcador(frame: 42, nombre: "Buena toma", etiqueta: .verde))
let datos = try! JSONEncoder().encode(t)
let vuelta = try! JSONDecoder().decode(LineaDeTiempo.self, from: datos)
igual(vuelta, t, "el montaje sobrevive a ida y vuelta a JSON")

print("— multicámara —")
let anguloA = UUID()
let anguloB = UUID()
let anguloC = UUID()
var multicam = MulticamDeClip(grupoID: UUID(), inicial: anguloA)
igual(multicam.medioActivo(en: 0), anguloA, "sin cortes manda el ángulo inicial")
igual(multicam.medioActivo(en: 500), anguloA, "y sigue mandando en el 500")
multicam.cambiar(en: 100, a: anguloB)
igual(multicam.medioActivo(en: 99), anguloA, "el corte en 100 no toca el 99")
igual(multicam.medioActivo(en: 100), anguloB, "y desde 100 manda el ángulo B")
multicam.cambiar(en: 250, a: anguloC)
multicam.cambiar(en: 300, a: anguloA)
igual(multicam.medioActivo(en: 120), anguloB, "cambiar en 300 no borra el corte de 100")
igual(multicam.medioActivo(en: 260), anguloC, "el corte de 250 sigue")
igual(multicam.medioActivo(en: 320), anguloA, "y el de 300 manda al final")
let cortesEsperados = [(0, 100, anguloA), (100, 250, anguloB), (250, 300, anguloC), (300, 400, anguloA)]
let segmentos = multicam.segmentos(duracion: 400)
comprobar(segmentos.count == cortesEsperados.count, "segmentos contiguos por ángulo")
comprobar(zip(segmentos, cortesEsperados).allSatisfy {
    $0.desde == $1.0 && $0.hasta == $1.1 && $0.mediaID == $1.2
}, "y cada tramo apunta a su ángulo")
multicam.cambiar(en: 150, a: anguloB)
igual(multicam.cortes.count, 3, "cambiar al ángulo que ya manda no añade corte")
multicam.cambiar(en: 0, a: anguloC)
igual(multicam.inicial, anguloC, "cambiar en el primer frame cambia el inicial")
igual(multicam.cortes.isEmpty, true, "y barre los cortes de toda la vida")
multicam.cambiar(en: 30, a: anguloB)
let datosMulticam = try! JSONEncoder().encode(multicam)
let vueltaMulticam = try! JSONDecoder().decode(MulticamDeClip.self, from: datosMulticam)
igual(vueltaMulticam, multicam, "el clip multicámara sobrevive al JSON")

var grupo = GrupoMulticam(nombre: "MC", mediaIDs: [anguloA, anguloB])
grupo.desfases = [anguloA: 0, anguloB: 40]
let datosGrupo = try! JSONEncoder().encode(grupo)
let vueltaGrupo = try! JSONDecoder().decode(GrupoMulticam.self, from: datosGrupo)
igual(vueltaGrupo, grupo, "el grupo guarda los desfases de sincronización")

// Cambiar la base de tiempo conserva el tiempo real de los desfases: un desfase
// de 40 frames a 25 fps son 1,6 s, que a 60 fps son 96 frames.
let convertidos = GrupoMulticam.convertirDesfases([anguloA: 0, anguloB: 40], de: Timebase.p25, a: Timebase.p60)
igual(convertidos[anguloB], 96, "los desfases se convierten por tiempo (40 f @ 25 = 96 f @ 60)")
igual(convertidos[anguloA], 0, "y el cero sigue siendo cero")
igual(GrupoMulticam.convertirDesfases([:], de: Timebase.p25, a: Timebase.p60), [:],
      "sin desfases, nada que convertir")

(t, v1, ids) = montajeDePrueba()
var clipMulticam = Clip(mediaID: media, nombre: "MC", inicio: 50, duracion: 100, entradaEnOrigen: 0)
clipMulticam.multicam = multicam
t.sobrescribir(clipMulticam, enPista: v1, en: 50)
igual(t.clip(clipMulticam.id)?.multicam, multicam, "el clip lleva su multicámara en el montaje")
let datosMontajeMulticam = try! JSONEncoder().encode(t)
let vueltaMontajeMulticam = try! JSONDecoder().decode(LineaDeTiempo.self, from: datosMontajeMulticam)
igual(vueltaMontajeMulticam.clip(clipMulticam.id)?.multicam, multicam, "y el montaje entero la conserva")

// Aplanar multicámara: el «flatten» de Premiere. Cada tramo de ángulo se
// convierte en un clip normal con su medio, su entrada en origen calculada
// con el desfase del grupo (la misma cuenta del constructor) y su enlace
// propio de vídeo/audio. Los keyframes se rebasan al tramo.
print("— aplanar multicámara —")
do {
    var t = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let a1 = t.pistas.first { $0.nombre == "A1" }!.id
    let anguloA = UUID(), anguloB = UUID(), anguloC = UUID()
    var grupo = GrupoMulticam(nombre: "Entrevista", mediaIDs: [anguloA, anguloB, anguloC])
    // A arranca 1 s (25 f) antes que la referencia, B es la referencia y C
    // 0,5 s (13 f) antes. El clip empieza en t=50 con 100 frames.
    grupo.desfases = [anguloA: 25, anguloB: 0, anguloC: 13]
    t.gruposMulticam = [grupo]
    var clip = Clip(mediaID: anguloA, nombre: "Entrevista", inicio: 50, duracion: 100, entradaEnOrigen: 0)
    var multicam = MulticamDeClip(grupoID: grupo.id, inicial: anguloA)
    multicam.cambiar(en: 40, a: anguloB)
    multicam.cambiar(en: 70, a: anguloC)
    clip.multicam = multicam
    var transformada = TransformacionDeClip.identidad
    transformada.escala = 50
    clip.transformacion = transformada
    clip.ganancia = -6
    clip.keyframes = [
        ClipKeyframe(frame: 10, transformacion: .identidad, ganancia: 0),
        ClipKeyframe(frame: 45, transformacion: .identidad, ganancia: -12),
    ]
    let enlace = UUID()
    clip.enlace = enlace
    t.sobrescribir(clip, enPista: v1, en: 50)
    var audio = Clip(mediaID: anguloA, nombre: "Entrevista", inicio: 50, duracion: 100, entradaEnOrigen: 0, enlace: enlace)
    audio.multicam = multicam
    audio.ganancia = -3
    t.sobrescribir(audio, enPista: a1, en: 50)

    let planos = t.aplanarMulticam(clipID: clip.id)
    igual(planos.count, 3, "aplanar crea un clip por tramo de ángulo")
    igual(t.clip(clip.id), nil, "y el clip multicámara desaparece")
    igual(t.pista(v1)?.clips.count, 3, "los tramos viven en la pista de vídeo")
    igual(t.pista(a1)?.clips.count, 3, "y el audio enlazado se reparte en su pista")

    // Tramo 1: A de t=50 a 90 (inicio 50, desde 0, desfase 25 → origen 25).
    let plano0 = t.pista(v1)!.clips[0]
    igual(plano0.mediaID, anguloA, "el tramo 1 usa el ángulo A")
    igual(plano0.inicio, 50, "el tramo 1 empieza donde el clip")
    igual(plano0.duracion, 40, "el tramo 1 mide hasta el primer corte")
    igual(plano0.entradaEnOrigen, 50 + 0 - 25, "el tramo 1 entra en origen con el desfase")
    igual(plano0.multicam, nil, "el tramo 1 ya no es multicámara")
    igual(plano0.enlace != enlace, true, "el tramo 1 tiene enlace propio")
    igual(plano0.transformacion.escala, 50, "el tramo hereda la transformación")
    igual(plano0.ganancia, -6, "y la ganancia del clip")
    igual(plano0.keyframes?.count, 1, "los keyframes del tramo se rebasan")
    igual(plano0.keyframes?.first?.frame, 10, "el keyframe de dentro del tramo conserva su frame")
    igual(t.pista(a1)!.clips[0].mediaID, anguloA, "el audio del tramo 1 usa el mismo ángulo")
    igual(t.pista(a1)!.clips[0].enlace, plano0.enlace, "y comparte enlace con su vídeo")
    igual(t.pista(a1)!.clips[0].ganancia, -3, "el audio hereda la ganancia del audio original")

    // Tramo 2: B de t=90 a 120 (inicio 90, desde 40, desfase 0 → origen 90).
    let plano1 = t.pista(v1)!.clips[1]
    igual(plano1.mediaID, anguloB, "el tramo 2 usa el ángulo B")
    igual(plano1.inicio, 90, "el tramo 2 empieza en el corte")
    igual(plano1.duracion, 30, "el tramo 2 mide hasta el segundo corte")
    igual(plano1.entradaEnOrigen, 90, "sin desfase la entrada es el inicio de grupo")
    igual(plano1.keyframes?.count, 1, "el tramo 2 rebasa su keyframe")
    igual(plano1.keyframes?.first?.frame, 5, "el keyframe del tramo 2 queda en 45−40")
    igual(plano1.enlace != plano0.enlace, true, "los tramos son independientes entre sí")

    // Tramo 3: C de t=120 a 150 (inicio 120, desde 70, desfase 13 → origen 107).
    let plano2 = t.pista(v1)!.clips[2]
    igual(plano2.mediaID, anguloC, "el tramo 3 usa el ángulo C")
    igual(plano2.inicio, 120, "el tramo 3 empieza en el segundo corte")
    igual(plano2.duracion, 30, "el tramo 3 mide hasta el final del clip")
    igual(plano2.entradaEnOrigen, 50 + 70 - 13, "el tramo 3 entra en origen con su desfase")

    // Un tramo cuyo desfase deja el material antes del arranque del ángulo no
    // se aplana: el constructor lo habría marcado como crítico.
    var temprano = LineaDeTiempo.nueva(timebase: .p25)
    let v = temprano.pistas.first { $0.nombre == "V1" }!.id
    var grupoTemprano = GrupoMulticam(nombre: "T", mediaIDs: [anguloA])
    grupoTemprano.desfases = [anguloA: 100]
    temprano.gruposMulticam = [grupoTemprano]
    var clipTemprano = Clip(mediaID: anguloA, nombre: "T", inicio: 50, duracion: 20, entradaEnOrigen: 0)
    clipTemprano.multicam = MulticamDeClip(grupoID: grupoTemprano.id, inicial: anguloA)
    temprano.sobrescribir(clipTemprano, enPista: v, en: 50)
    let planosTempranos = temprano.aplanarMulticam(clipID: clipTemprano.id)
    igual(planosTempranos.isEmpty, true, "el tramo sin material no se aplana")
    igual(temprano.pista(v)?.clips.isEmpty, true, "y el clip multicámara tampoco queda a medias")

    // Aplanar un clip que no es multicámara no toca nada.
    var plano = LineaDeTiempo.nueva(timebase: .p25)
    let vp = plano.pistas.first { $0.nombre == "V1" }!.id
    let normal = Clip(mediaID: media, nombre: "N", inicio: 0, duracion: 30, entradaEnOrigen: 0)
    plano.sobrescribir(normal, enPista: vp, en: 0)
    let planosNormales = plano.aplanarMulticam(clipID: normal.id)
    igual(planosNormales.isEmpty, true, "aplanar un clip normal no hace nada")
    igual(plano.clip(normal.id) != nil, true, "y el clip normal sigue ahí")
}

// Sincronía manual: los nudges del visor corrigen el desfase de un ángulo.
print("— sincronía manual —")
do {
    var t = LineaDeTiempo.nueva(timebase: .p25)
    let grupoID = UUID()
    let medioA = UUID(), medioB = UUID()
    var grupo = GrupoMulticam(id: grupoID, nombre: "MC", mediaIDs: [medioA, medioB])
    grupo.desfases = [medioA: 25, medioB: 0]
    t.gruposMulticam = [grupo]

    igual(t.ajustarDesfase(grupoID: grupoID, medioID: medioB, delta: 1), 1,
          "un +1 frame avanza el desfase del ángulo")
    igual(t.gruposMulticam?[0].desfases[medioB], 1, "y queda guardado en el grupo")
    igual(t.ajustarDesfase(grupoID: grupoID, medioID: medioB, delta: -10), 0,
          "el desfase nunca baja de cero")
    igual(t.gruposMulticam?[0].desfases[medioB], 0, "el tope aplica de verdad")
    igual(t.ajustarDesfase(grupoID: grupoID, medioID: medioA, delta: -1), 24,
          "−1 frame atrasa el desfase de otro ángulo")
    igual(t.ajustarDesfase(grupoID: UUID(), medioID: medioA, delta: 5), 0,
          "un grupo que no existe no cambia nada")
    var sinDesfases = GrupoMulticam(nombre: "MC", mediaIDs: [medioA])
    igual(sinDesfases.posicionDeAngulo(medioA, enTiempoDeGrupo: 100), 100,
          "sin desfase el ángulo enseña el instante de grupo")
    sinDesfases.desfases = [medioA: 30]
    igual(sinDesfases.posicionDeAngulo(medioA, enTiempoDeGrupo: 100), 70,
          "con desfase resta: el ángulo que arranca tarde se ve retrasado")
    igual(sinDesfases.posicionDeAngulo(medioA, enTiempoDeGrupo: 10), 0,
          "y nunca pide material antes del arranque")
}

print("")
print("— retime con rampas de velocidad —")
do {
    // Sin rampas, la velocidad es la del clip.
    let normal = Clip(mediaID: media, nombre: "N", inicio: 0, duracion: 100, entradaEnOrigen: 0)
    igual(normal.velocidadEn(frame: 50), 1, "sin rampas la velocidad es la del clip")
    igual(normal.duracionEnOrigen, 100, "y consume un frame por frame")

    // Una rampa de 1 → 2 en la mitad: la velocidad interpola en línea recta.
    var rampa = Clip(mediaID: media, nombre: "R", inicio: 0, duracion: 100, entradaEnOrigen: 0)
    rampa.rampasDeVelocidad = [
        RampaDeVelocidad(frame: 0, velocidad: 1),
        RampaDeVelocidad(frame: 100, velocidad: 2),
    ]
    igual(rampa.velocidadEn(frame: 0), 1, "la rampa arranca a 1×")
    igual(rampa.velocidadEn(frame: 50), 1.5, "la rampa interpola a 1,5× en el centro")
    igual(rampa.velocidadEn(frame: 100), 2, "y llega a 2× al final")
    // Integral trapezoidal de 1→2 sobre 100 frames = 100 * 1,5 = 150.
    igual(rampa.duracionEnOrigen, 150, "el consumo es la integral de la velocidad")

    // Un congelado: velocidad 0 desde el frame 40 hasta el 60 (dos keyframes
    // a cero, uno en cada borde del tramo congelado).
    var congelado = Clip(mediaID: media, nombre: "F", inicio: 0, duracion: 100, entradaEnOrigen: 0)
    congelado.rampasDeVelocidad = [
        RampaDeVelocidad(frame: 0, velocidad: 1),
        RampaDeVelocidad(frame: 40, velocidad: 0),
        RampaDeVelocidad(frame: 60, velocidad: 0),
        RampaDeVelocidad(frame: 100, velocidad: 1),
    ]
    igual(congelado.velocidadEn(frame: 50), 0, "el tramo central está congelado")
    // Tramo 1: 1→0 en 40 frames consume 20. Tramo 2: 0→0 en 20 consume 0.
    // Tramo 3: 0→1 en 40 consume 20. Total 40 de 100.
    igual(congelado.duracionEnOrigen, 40, "el congelado no consume material (20 + 0 + 20)")

    // Las rampas sobreviven al JSON (proyecto portable).
    let datosRampa = try! JSONEncoder().encode(rampa)
    let vueltaRampa = try! JSONDecoder().decode(Clip.self, from: datosRampa)
    igual(vueltaRampa.rampasDeVelocidad, rampa.rampasDeVelocidad, "las rampas viajan en el proyecto")

    // La velocidad nunca baja de cero, aunque se pida un valor negativo.
    let negativa = RampaDeVelocidad(frame: 0, velocidad: -3)
    igual(negativa.velocidad, 0, "una velocidad negativa se trunca a congelado")

    // Las piezas para el render: contiguas, cubriendo el clip entero, con el
    // consumo de origen resuelto. Un tramo congelado consume cero.
    let piezas = congelado.piezasDeVelocidad()
    igual(piezas.count, 3, "el render parte el clip en tres tramos")
    igual(piezas[0].desde, 0, "el primer tramo empieza en el inicio")
    igual(piezas[0].hasta, 40, "y acaba en el primer keyframe")
    igual(piezas[0].consumo, 20, "la rampa de entrada consume 20 frames")
    igual(piezas[1].consumo, 0, "el tramo congelado no consume material")
    igual(piezas[2].hasta, 100, "el último tramo llega al final del clip")
    igual(piezas[2].consumo, 20, "y la rampa de salida consume 20")
    let suma = piezas.reduce(Int64(0)) { $0 + $1.consumo }
    igual(suma, congelado.duracionEnOrigen, "el consumo de las piezas suma la duración en origen")

    // Sin rampas, una sola pieza con la velocidad del clip.
    var doble = Clip(mediaID: media, nombre: "2x", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    doble.velocidad = 2
    let piezasDoble = doble.piezasDeVelocidad()
    igual(piezasDoble.count, 1, "sin rampas hay una sola pieza")
    igual(piezasDoble[0].consumo, 100, "y consume el doble de material")
}

print("— títulos —")
do {
    var titulo = TituloDeClip(texto: "Entrevista", posicionX: 0.3, posicionY: 0.8)
    igual(titulo.texto, "Entrevista", "el título lleva su texto")
    igual(titulo.posicionX, 0.3, "y su posición")
    let fuera = TituloDeClip(posicionX: 2, posicionY: -1)
    igual(fuera.posicionX, 1, "la posición se recorta al lienzo")
    igual(fuera.posicionY, 0, "también en vertical")
    let datosTitulo = try! JSONEncoder().encode(titulo)
    let vueltaTitulo = try! JSONDecoder().decode(TituloDeClip.self, from: datosTitulo)
    igual(vueltaTitulo, titulo, "el título viaja en el proyecto")

    var clip = Clip(mediaID: media, nombre: "T", inicio: 0, duracion: 100, entradaEnOrigen: 0)
    clip.esTitulo = true
    clip.titulo = titulo
    let datosClipTitulo = try! JSONEncoder().encode(clip)
    let vueltaClipTitulo = try! JSONDecoder().decode(Clip.self, from: datosClipTitulo)
    igual(vueltaClipTitulo.titulo, titulo, "el clip de título guarda su título")
    igual(vueltaClipTitulo.esTitulo, true, "y se sabe que es un título")
}

print("— subclips —")
do {
    // Un recorte del medio base: entrada/salida recortadas al archivo y nunca vacías.
    let origen = SubclipOrigen(medioBase: media, entrada: 100, salida: 200)
    igual(origen.duracion, 100, "el subclip mide su recorte")
    let recortado = SubclipOrigen(medioBase: media, entrada: -5, salida: 0)
    igual(recortado.entrada, 0, "la entrada negativa se recorta a cero")
    igual(recortado.duracion, 1, "y un recorte vacío mide al menos un frame")
    let datosSubclip = try! JSONEncoder().encode(origen)
    let vueltaSubclip = try! JSONDecoder().decode(SubclipOrigen.self, from: datosSubclip)
    igual(vueltaSubclip, origen, "el subclip viaja en el proyecto")
}

print("")
print("— firma de composición (preview incremental) —")
do {
    var t = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let a = Clip(mediaID: media, nombre: "A", inicio: 0, duracion: 100, entradaEnOrigen: 0)
    t.sobrescribir(a, enPista: v1, en: 0)
    let firmaBase = t.firmaDeComposicion

    // Cambiar un atributo (ganancia) no cambia la estructura: el preview puede
    // reutilizar las pistas y solo rehacer las instrucciones y la mezcla.
    var conGanancia = t
    conGanancia.pistas[conGanancia.indiceDePista(v1)!].clips[0].ganancia = -6
    igual(conGanancia.firmaDeComposicion, firmaBase, "un cambio de ganancia no toca la estructura")
    var conColor = t
    var color = ColorDeClip.neutro
    color.contraste = 10
    conColor.pistas[conColor.indiceDePista(v1)!].clips[0].color = color
    igual(conColor.firmaDeComposicion, firmaBase, "un cambio de color tampoco")

    // Un cambio geométrico (duración, posición, clip nuevo) sí cambia la firma.
    var masLargo = t
    masLargo.pistas[masLargo.indiceDePista(v1)!].clips[0].duracion = 200
    igual(masLargo.firmaDeComposicion == firmaBase, false, "cambiar la duración recompone las pistas")
    var conOtroClip = t
    let b = Clip(mediaID: media, nombre: "B", inicio: 100, duracion: 50, entradaEnOrigen: 0)
    conOtroClip.sobrescribir(b, enPista: v1, en: 100)
    igual(conOtroClip.firmaDeComposicion == firmaBase, false, "añadir un clip recompone las pistas")

    // Rampas de velocidad y multicámara entran en la firma.
    var conRampa = t
    conRampa.pistas[conRampa.indiceDePista(v1)!].clips[0].rampasDeVelocidad = [RampaDeVelocidad(frame: 50, velocidad: 0.5)]
    igual(conRampa.firmaDeComposicion == firmaBase, false, "las rampas de velocidad recompone las pistas")
    igual(conRampa.firmaDeComposicion, conRampa.firmaDeComposicion, "y la firma es estable entre lecturas")
}

print("")
print("— modos de fusión, máscaras, curvas y formas —")
do {
    // Modos de fusión: serialización y filtro de Core Image.
    igual(ModoDeFusion.multiplicar.nombre, "Multiplicar", "el modo tiene nombre")
    igual(ModoDeFusion.multiplicar.filtroCI, "CIMultiplyBlendMode", "y su filtro de Core Image")
    igual(ModoDeFusion.normal.filtroCI, nil, "normal no tiene filtro: es el encima de siempre")
    let datosModo = try! JSONEncoder().encode(ModoDeFusion.superponer)
    let vueltaModo = try! JSONDecoder().decode(ModoDeFusion.self, from: datosModo)
    igual(vueltaModo, .superponer, "el modo viaja en el proyecto")

    // Máscaras: recorte de posición, tamaño y pluma.
    let mascara = MascaraDeClip(forma: .elipse, posicionX: 0.3, posicionY: 0.7,
                                tamanoX: 0.4, tamanoY: 0.6, pluma: 0.2, invertida: true)
    igual(mascara.forma, .elipse, "la máscara guarda su forma")
    igual(mascara.posicionX, 0.3, "y su posición")
    igual(mascara.activa, true, "y con tamaño positivo está activa")
    igual(MascaraDeClip(forma: .rectangulo, tamanoX: 0, tamanoY: 0).activa, false,
          "una máscara sin tamaño está inactiva")
    let fueraDeLienzo = MascaraDeClip(posicionX: 2, posicionY: -1)
    igual(fueraDeLienzo.posicionX, 1, "la posición se recorta al lienzo")
    igual(fueraDeLienzo.posicionY, 0, "también en vertical")
    let datosMascara = try! JSONEncoder().encode(mascara)
    let vueltaMascara = try! JSONDecoder().decode(MascaraDeClip.self, from: datosMascara)
    igual(vueltaMascara, mascara, "la máscara viaja en el proyecto")

    // Curvas: interpolación y tabla.
    igual(ColorDeClip.interpolar([(0, 0), (1, 1)], en: 0.5), 0.5, "la curva identidad interpola en la diagonal")
    igual(ColorDeClip.interpolar([(0, 0), (0.5, 1), (1, 0)], en: 0.25), 0.5, "la interpolación es lineal entre puntos")
    igual(ColorDeClip.interpolar([(0, 0), (1, 1)], en: -0.5), 0, "fuera de rango se ancla al primer punto")
    let curvas = CurvasDeClip()
    igual(curvas.esIdentidad, true, "las curvas por defecto son la identidad")
    var curvasEditadas = CurvasDeClip()
    curvasEditadas.rojo = [.init(x: 0, y: 0), .init(x: 0.5, y: 0.8), .init(x: 1, y: 1)]
    igual(curvasEditadas.esIdentidad, false, "tocar un canal deja de ser identidad")
    // La tabla de la identidad debe ser {i/255, i/255, i/255}.
    let tabla = curvas.tabla
    let floats = tabla.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
    igual(abs(floats[128 * 3] - 128.0 / 255) < 0.001, true, "la tabla identidad pasa por el medio")
    igual(abs(floats[255 * 3] - 1) < 0.001, true, "y llega al blanco")
    let datosCurvas = try! JSONEncoder().encode(curvasEditadas)
    let vueltaCurvas = try! JSONDecoder().decode(CurvasDeClip.self, from: datosCurvas)
    igual(vueltaCurvas, curvasEditadas, "las curvas viajan en el proyecto")

    // Formas de título.
    igual(FormaDeTitulo.elipse.nombre, "Elipse", "la forma tiene nombre")
    var titulo = TituloDeClip(texto: "Caja", forma: .rectangulo, ancho: 0.5, alto: 0.3)
    igual(titulo.forma, .rectangulo, "el título puede ser una forma")
    igual(titulo.ancho, 0.5, "y guarda su tamaño")
    let datosForma = try! JSONEncoder().encode(titulo)
    let vueltaForma = try! JSONDecoder().decode(TituloDeClip.self, from: datosForma)
    igual(vueltaForma, titulo, "la forma viaja en el proyecto")

    // Viñeta y desenfoque entran en «tieneAjustes».
    var conVignette = ColorDeClip.neutro
    conVignette.vignette = 0.3
    igual(conVignette.tieneAjustes, true, "la viñeta activa la cadena de color")
    igual(conVignette.esNeutro, false, "y ya no es neutro")
    var conDesenfoque = ColorDeClip.neutro
    conDesenfoque.desenfoque = 0.1
    igual(conDesenfoque.tieneAjustes, true, "el desenfoque también")
}

print("")
print("— ruedas de color, chroma key, smart bins y EDL —")
do {
    // Ruedas: la curva de un canal desde sus tres ruedas.
    let curva = RuedasDeColor.curvaDe(0.3, -0.2, 0)
    igual(curva.count, 3, "la rueda se convierte en tres puntos")
    igual(curva[0].0, 0, "el primero es la sombra")
    igual(curva[1].0, 0.5, "el segundo los medios")
    igual(curva[2].0, 1, "y el tercero las altas")
    igual(curva[0].1, 0.15, "la sombra se atenúa a la mitad en la curva")
    let ruedas = RuedasDeColor(sombrasAzul: 0.5, altasRojo: -0.3)
    igual(ruedas.esNeutro, false, "una rueda tocada no es neutra")
    igual(RuedasDeColor().esNeutro, true, "las ruedas por defecto sí lo son")
    let datosRuedas = try! JSONEncoder().encode(ruedas)
    let vueltaRuedas = try! JSONDecoder().decode(RuedasDeColor.self, from: datosRuedas)
    igual(vueltaRuedas, ruedas, "las ruedas viajan en el proyecto")

    // Chroma key.
    let croma = ChromaKeyDeClip(rojo: 0, verde: 1, azul: 0, tolerancia: 0.3, suavizado: 0.1, suprimirDerrame: 0.6)
    igual(croma.esNeutro, false, "el chroma key de pantalla verde no es neutro")
    igual(ChromaKeyDeClip().esNeutro, false, "el verde por defecto tampoco")
    var gris = ChromaKeyDeClip()
    gris.rojo = 0; gris.verde = 0; gris.azul = 0
    igual(gris.esNeutro, true, "una clave negra (sin color) es neutra")
    let datosCroma = try! JSONEncoder().encode(croma)
    let vueltaCroma = try! JSONDecoder().decode(ChromaKeyDeClip.self, from: datosCroma)
    igual(vueltaCroma, croma, "el chroma key viaja en el proyecto")

    // Bins inteligentes.
    let normal = MediaBin(nombre: "Normal")
    igual(normal.esInteligente, false, "un bin sin filtro es normal")
    igual(normal.contiene(("cualquier cosa", false, false)), true, "y deja entrar todo")
    let smartVFR = MediaBin(nombre: "Smart: VFR", filtro: "VFR")
    igual(smartVFR.esInteligente, true, "con filtro es inteligente")
    igual(smartVFR.contiene(("clip", true, false)), true, "VFR filtra los medios variables")
    igual(smartVFR.contiene(("clip", false, false)), false, "y deja fuera los de cadencia fija")
    let smartAudio = MediaBin(nombre: "Smart: Audio", filtro: "Audio")
    igual(smartAudio.contiene(("pista", false, true)), true, "Audio filtra los medios sin vídeo")
    let smartNombre = MediaBin(nombre: "Smart: entrevista", filtro: "entrevista")
    igual(smartNombre.contiene(("Entrevista 2", false, false)), true, "cualquier otra palabra busca en el nombre")
    igual(smartNombre.contiene(("Clase", false, false)), false, "y rechaza lo que no la lleva")
    let datosBin = try! JSONEncoder().encode(smartVFR)
    let vueltaBin = try! JSONDecoder().decode(MediaBin.self, from: datosBin)
    igual(vueltaBin, smartVFR, "el bin inteligente viaja en el proyecto")

    // Sidechain selectivo: la pista guarda su fuente de ducking.
    var tSidechain = LineaDeTiempo.nueva(timebase: .p25)
    let a1 = tSidechain.pistas.first { $0.nombre == "A1" }!.id
    let a2 = tSidechain.pistas.first { $0.nombre == "A2" }!.id
    tSidechain.pistas[tSidechain.indiceDePista(a2)!].fuenteDeDucking = a1
    igual(tSidechain.pista(a2)?.fuenteDeDucking, a1, "la pista guarda su fuente de sidechain")
    let datosSidechain = try! JSONEncoder().encode(tSidechain)
    let vueltaSidechain = try! JSONDecoder().decode(LineaDeTiempo.self, from: datosSidechain)
    igual(vueltaSidechain.pista(a2)?.fuenteDeDucking, a1, "y viaja en el proyecto")
}

print("")
print("— describirCambio (historia de deshacer) —")
do {
    var t = LineaDeTiempo.nueva(timebase: .p25)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let a = Clip(mediaID: media, nombre: "A", inicio: 0, duracion: 100, entradaEnOrigen: 0)
    t.sobrescribir(a, enPista: v1, en: 0)
    let base = t

    var movido = base
    movido.pistas[movido.indiceDePista(v1)!].clips[0].inicio = 50
    igual(LineaDeTiempo.describirCambio(antes: base, despues: movido), "Movido «A»",
          "mover un clip se describe")

    var recortado = base
    recortado.pistas[recortado.indiceDePista(v1)!].clips[0].duracion = 60
    igual(LineaDeTiempo.describirCambio(antes: base, despues: recortado), "Recortado «A»",
          "recortar se describe")

    var conColor = base
    conColor.pistas[conColor.indiceDePista(v1)!].clips[0].color.exposicion = 20
    igual(LineaDeTiempo.describirCambio(antes: base, despues: conColor), "Color de «A»",
          "el color se describe")

    var conGanancia = base
    conGanancia.pistas[conGanancia.indiceDePista(v1)!].clips[0].ganancia = -6
    igual(LineaDeTiempo.describirCambio(antes: base, despues: conGanancia), "Ganancia de «A»",
          "la ganancia se describe")

    var conOtro = base
    let b = Clip(mediaID: media, nombre: "B", inicio: 100, duracion: 50, entradaEnOrigen: 0)
    conOtro.sobrescribir(b, enPista: v1, en: 100)
    igual(LineaDeTiempo.describirCambio(antes: base, despues: conOtro), "Añadido «B»",
          "añadir se describe")

    var sinNada = base
    igual(LineaDeTiempo.describirCambio(antes: base, despues: sinNada), "Edición",
          "sin cambios reconocibles, descripción genérica")
}

print("")
print(fallos == 0 ? "TODO CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
