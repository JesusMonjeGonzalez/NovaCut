import Foundation

/// Edición por transcript: seleccionar texto y que desaparezca el vídeo.
///
/// Es la ruta corta que hace que una entrevista de una hora se monte en veinte
/// minutos en vez de en tres horas, y el motivo por el que Descript existe. Aquí lo
/// difícil no es la interfaz, son dos cosas del modelo:
///
/// 1. El transcript pertenece al **medio** y los cortes viven en el **montaje**. Una
///    palabra puede no aparecer, aparecer una vez, o aparecer tres si el medio se usó
///    tres veces. Lo que se enseña en el panel tiene que ser lo que se oye en el
///    montaje, en el orden del montaje.
/// 2. Al borrar varios tramos con arrastre hay que ir **de atrás hacia delante**. Al
///    revés, el primer borrado corre todo lo que viene detrás y los rangos siguientes
///    apuntan a otro sitio: se come material que nadie pidió y no da ningún error.

var fallos = 0
func comprobar(_ condicion: Bool, _ mensaje: String) {
    if condicion { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje)"); fallos += 1 }
}
func igual<T: Equatable>(_ a: T, _ b: T, _ mensaje: String) {
    if a == b { print("  ok  \(mensaje)") } else { print("  FALLO  \(mensaje): \(a) != \(b)"); fallos += 1 }
}

let medio = UUID()
let tb = Timebase.p25

/// Una palabra por segundo, de 0 a n−1, con el texto que se le pase.
func transcripcionSeguida(_ textos: [String]) -> Transcripcion {
    Transcripcion(
        mediaID: medio,
        palabras: textos.enumerated().map { i, texto in
            Palabra(texto: texto, inicio: Double(i), duracion: 1)
        }
    )
}

// MARK: - Qué palabras se oyen en el montaje

print("— palabras del montaje —")

do {
    // Un clip que usa del segundo 2 al 5 del medio: las palabras 0 y 1 no se oyen.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let clip = Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 75, entradaEnOrigen: 50)
    t.sobrescribir(clip, enPista: v1, en: 0)

    let transcripcion = transcripcionSeguida(["cero", "uno", "dos", "tres", "cuatro", "cinco"])
    let leidas = t.palabrasDelMontaje(transcripcion)

    igual(leidas.map(\.texto), ["dos", "tres", "cuatro"], "solo se leen las palabras que el clip usa")
    igual(leidas[0].desde, 0, "la primera palabra arranca en el frame 0 del montaje")
    igual(leidas[0].hasta, 25, "y dura un segundo, 25 frames")
    igual(leidas[2].hasta, 75, "la última acaba justo donde acaba el clip")
    igual(leidas[0].clipID, clip.id, "cada palabra sabe de qué clip viene")
}

do {
    // El mismo medio dos veces y en orden invertido: manda el orden del montaje.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let segundo = Clip(mediaID: medio, nombre: "B", inicio: 0, duracion: 25, entradaEnOrigen: 75)
    let primero = Clip(mediaID: medio, nombre: "A", inicio: 25, duracion: 25, entradaEnOrigen: 0)
    t.sobrescribir(segundo, enPista: v1, en: 0)
    t.sobrescribir(primero, enPista: v1, en: 25)

    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["cero", "uno", "dos", "tres"]))
    igual(leidas.map(\.texto), ["tres", "cero"], "el orden es el del montaje, no el del medio")
}

do {
    // El medio usado dos veces: la palabra sale dos veces, cada una en su sitio.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "A", inicio: 0, duracion: 25, entradaEnOrigen: 0), enPista: v1, en: 0)
    t.sobrescribir(Clip(mediaID: medio, nombre: "B", inicio: 25, duracion: 25, entradaEnOrigen: 0), enPista: v1, en: 25)

    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["hola", "adios"]))
    igual(leidas.map(\.texto), ["hola", "hola"], "una palabra usada dos veces aparece dos veces")
    igual(leidas[1].desde, 25, "la segunda vez suena donde la pusieron")
}

do {
    // Con velocidad al doble, un segundo de habla ocupa medio segundo de montaje.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    var clip = Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 50, entradaEnOrigen: 0)
    clip.velocidad = 2
    t.sobrescribir(clip, enPista: v1, en: 0)

    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["cero", "uno", "dos", "tres"]))
    igual(leidas.count, 4, "al doble de velocidad caben cuatro palabras en dos segundos")
    igual(leidas[1].desde, 13, "la segunda palabra empieza a la mitad de su tiempo de origen")
}

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: UUID(), nombre: "otro", inicio: 0, duracion: 100, entradaEnOrigen: 0), enPista: v1, en: 0)
    igual(t.palabrasDelMontaje(transcripcionSeguida(["hola"])).count, 0, "un medio distinto no aporta palabras")
}

// MARK: - De la selección a los rangos

print("— rangos de la selección —")

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: v1, en: 0)
    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["a", "b", "c", "d", "e"]))

    let contiguo = TranscriptService.rangos(de: leidas, indices: [1, 2])
    igual(contiguo.count, 1, "dos palabras seguidas son un solo rango")
    igual(contiguo[0].desde, 25, "el rango arranca en la primera")
    igual(contiguo[0].hasta, 75, "y acaba en la última")

    let separado = TranscriptService.rangos(de: leidas, indices: [1, 3])
    igual(separado.count, 2, "dos palabras separadas son dos rangos")
    igual(separado[0].desde, 25, "el primero es el de delante")
    igual(separado[1].desde, 75, "y el segundo el de detrás")

    igual(TranscriptService.rangos(de: leidas, indices: []).count, 0, "sin selección no hay rangos")
    igual(TranscriptService.rangos(de: leidas, indices: [99]).count, 0, "un índice inventado no rompe nada")
}

do {
    // Dentro de un clip, una selección seguida se lleva también los silencios que
    // hay entre las palabras: es lo que espera quien selecciona un párrafo.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: v1, en: 0)
    let conPausas = Transcripcion(mediaID: medio, palabras: [
        Palabra(texto: "una", inicio: 0, duracion: 0.4),
        Palabra(texto: "frase", inicio: 1.2, duracion: 0.4),
    ])
    let leidas = t.palabrasDelMontaje(conPausas)
    let rangos = TranscriptService.rangos(de: leidas, indices: [0, 1])
    igual(rangos.count, 1, "una selección seguida es un rango")
    igual(rangos[0].desde, 0, "desde el principio de la primera palabra")
    igual(rangos[0].hasta, 40, "hasta el final de la última, pausa incluida")
}

do {
    // Y a través de un corte, no. Entre dos clips del mismo medio puede haber
    // material de otro que el panel de texto no enseña, y fundir los rangos lo
    // borraría sin que nadie lo haya pedido.
    let otro = UUID()
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "A", inicio: 0, duracion: 25, entradaEnOrigen: 0), enPista: v1, en: 0)
    t.sobrescribir(Clip(mediaID: otro, nombre: "medio", inicio: 25, duracion: 50, entradaEnOrigen: 0), enPista: v1, en: 25)
    t.sobrescribir(Clip(mediaID: medio, nombre: "B", inicio: 75, duracion: 25, entradaEnOrigen: 25), enPista: v1, en: 75)

    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["a", "b"]))
    igual(leidas.map(\.texto), ["a", "b"], "se leen las dos palabras, una por clip")
    let rangos = TranscriptService.rangos(de: leidas, indices: [0, 1])
    igual(rangos.count, 2, "dos clips distintos son dos rangos aunque el texto siga")

    t.extraerRangos(rangos)
    igual(t.pista(v1)!.clips.count, 1, "sobrevive el clip de en medio")
    igual(t.pista(v1)!.clips[0].nombre, "medio", "y es el que no se había seleccionado")
}

// MARK: - Borrar texto borra vídeo

print("— borrar el rango cerrando el hueco —")

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: v1, en: 0)

    t.extraerRango(desde: 25, hasta: 50)
    igual(t.duracion, 100, "la duración baja exactamente lo borrado")
    igual(t.pista(v1)!.clips.count, 2, "el clip queda partido en dos")
    igual(t.pista(v1)!.clips[1].inicio, 25, "y la segunda mitad se pega a la primera")
    igual(t.pista(v1)!.clips[1].entradaEnOrigen, 50, "conservando su entrada en origen")
}

do {
    // El fallo que se busca: borrar dos tramos en una sola pasada.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: v1, en: 0)
    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["a", "b", "c", "d", "e"]))

    // Fuera la «b» y la «d»: tienen que quedarse a, c y e, en ese orden.
    t.extraerRangos(TranscriptService.rangos(de: leidas, indices: [1, 3]))
    igual(t.duracion, 75, "quitadas dos palabras de cinco quedan tres segundos")

    let quedan = t.palabrasDelMontaje(transcripcionSeguida(["a", "b", "c", "d", "e"]))
    igual(quedan.map(\.texto), ["a", "c", "e"], "el texto que queda es el que se pidió")
}

do {
    // Y con tres tramos, para que un error de orden no pueda pasar por casualidad.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 175, entradaEnOrigen: 0), enPista: v1, en: 0)
    let textos = ["a", "b", "c", "d", "e", "f", "g"]
    let leidas = t.palabrasDelMontaje(transcripcionSeguida(textos))

    t.extraerRangos(TranscriptService.rangos(de: leidas, indices: [1, 3, 5]))
    igual(t.palabrasDelMontaje(transcripcionSeguida(textos)).map(\.texto), ["a", "c", "e", "g"], "tres tramos salen bien")
}

do {
    // El audio enlazado tiene que seguir al vídeo, o la voz se descoloca.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let a1 = t.pistas.first { $0.nombre == "A1" }!.id
    var video = Clip(mediaID: medio, nombre: "V", inicio: 0, duracion: 125, entradaEnOrigen: 0)
    var audio = Clip(mediaID: medio, nombre: "A", inicio: 0, duracion: 125, entradaEnOrigen: 0)
    let enlace = UUID()
    video.enlace = enlace
    audio.enlace = enlace
    t.sobrescribir(video, enPista: v1, en: 0)
    t.sobrescribir(audio, enPista: a1, en: 0)

    t.extraerRango(desde: 25, hasta: 50)
    igual(t.pista(a1)!.clips.count, 2, "el audio se corta igual que el vídeo")
    igual(t.pista(a1)!.clips[1].inicio, 25, "y se pega en el mismo sitio")
    igual(t.pista(v1)!.clips[1].inicio, t.pista(a1)!.clips[1].inicio, "imagen y voz siguen alineadas")
}

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 50, entradaEnOrigen: 0), enPista: v1, en: 0)
    let antes = t
    t.extraerRango(desde: 500, hasta: 600)
    igual(t.duracion, antes.duracion, "un rango que no toca nada no cambia el montaje")
    t.extraerRango(desde: 30, hasta: 30)
    igual(t.duracion, antes.duracion, "un rango vacío tampoco")
}

do {
    // Una pista bloqueada no se toca: es lo que significa bloquearla.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    let a1 = t.pistas.first { $0.nombre == "A1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "V", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: v1, en: 0)
    t.sobrescribir(Clip(mediaID: medio, nombre: "A", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: a1, en: 0)
    t.pistas[t.indiceDePista(a1)!].bloqueada = true

    t.extraerRango(desde: 25, hasta: 50)
    igual(t.pista(v1)!.clips.count, 2, "la pista libre se corta")
    igual(t.pista(a1)!.clips.count, 1, "la bloqueada se queda como estaba")
}

do {
    // Marcadores y subtítulos viven en frames de montaje: si no se corren, señalan
    // otra cosa en cuanto se borra algo delante, y eso es una deriva silenciosa.
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 125, entradaEnOrigen: 0), enPista: v1, en: 0)
    t.marcadores = [
        Marcador(frame: 10, nombre: "antes"),
        Marcador(frame: 35, nombre: "dentro"),
        Marcador(frame: 100, nombre: "despues"),
    ]
    t.subtitulos = [
        Subtitulo(inicio: 0, fin: 20, texto: "antes"),
        Subtitulo(inicio: 26, fin: 49, texto: "dentro"),
        Subtitulo(inicio: 75, fin: 100, texto: "despues"),
    ]

    t.extraerRango(desde: 25, hasta: 50)

    igual(t.marcadores.map(\.nombre), ["antes", "despues"], "el marcador de dentro se va con el material")
    igual(t.marcadores.first { $0.nombre == "antes" }!.frame, 10, "el de delante no se mueve")
    igual(t.marcadores.first { $0.nombre == "despues" }!.frame, 75, "el de detrás se corre lo borrado")
    igual(t.subtitulos!.map(\.texto), ["antes", "despues"], "el subtítulo de dentro también desaparece")
    igual(t.subtitulos!.first { $0.texto == "despues" }!.inicio, 50, "y el de detrás se corre igual")
}

// MARK: - Muletillas

print("— muletillas —")

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 250, entradaEnOrigen: 0), enPista: v1, en: 0)

    let textos = ["Bueno,", "eh", "esta", "mesa", "es", "o", "sea", "mía", "¿sabes?", "ya"]
    let leidas = t.palabrasDelMontaje(transcripcionSeguida(textos))
    let indices = TranscriptService.muletillas(en: leidas)

    comprobar(indices.contains(1), "«eh» es muletilla")
    comprobar(indices.contains(8), "«¿sabes?» es muletilla aunque lleve signos")
    comprobar(!indices.contains(2), "«esta» no es muletilla: «este» lo es, «esta mesa» no")
    comprobar(!indices.contains(3), "«mesa» no es muletilla")
    comprobar(indices.contains(5) && indices.contains(6), "«o sea» se detecta como las dos palabras que es")
    comprobar(!indices.contains(9), "«ya» suelta no se toca")
    // Decisión, no descuido: «bueno» y «pues» son muletilla a veces y palabra
    // siempre. Borrar de más en una propuesta automática cuesta la confianza.
    comprobar(!indices.contains(0), "«bueno» no entra en el diccionario")
}

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 75, entradaEnOrigen: 0), enPista: v1, en: 0)
    let leidas = t.palabrasDelMontaje(transcripcionSeguida(["EH", "Em...", "hola"]))
    let indices = TranscriptService.muletillas(en: leidas)
    igual(indices, [0, 1], "mayúsculas y puntos suspensivos no despistan")
}

// MARK: - Buscar por lo que se dice

print("— búsqueda en el transcript —")

let otroMedio = UUID()

func montajeConDosMedios() -> LineaDeTiempo {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "A", inicio: 0, duracion: 250, entradaEnOrigen: 0), enPista: v1, en: 0)
    t.sobrescribir(Clip(mediaID: otroMedio, nombre: "B", inicio: 250, duracion: 250, entradaEnOrigen: 0), enPista: v1, en: 250)
    t.transcripciones = [
        Transcripcion(mediaID: medio, palabras: [
            Palabra(texto: "el", inicio: 0, duracion: 1),
            Palabra(texto: "precio", inicio: 1, duracion: 1),
            Palabra(texto: "del", inicio: 2, duracion: 1),
            Palabra(texto: "café", inicio: 3, duracion: 1),
            Palabra(texto: "subió", inicio: 4, duracion: 1),
        ]),
        Transcripcion(mediaID: otroMedio, palabras: [
            Palabra(texto: "nos", inicio: 0, duracion: 1),
            Palabra(texto: "costó", inicio: 1, duracion: 1),
            Palabra(texto: "demasiado", inicio: 2, duracion: 1),
            Palabra(texto: "café", inicio: 3, duracion: 1),
        ]),
    ]
    return t
}

do {
    let t = montajeConDosMedios()
    let hallazgos = t.buscarEnLoQueSeDice("precio")
    igual(hallazgos.count, 1, "una palabra que solo está en un medio sale una vez")
    igual(hallazgos[0].mediaID, medio, "y trae el medio en el que está")
    igual(hallazgos[0].segundoEnElMedio, 1.0, "con el segundo exacto del archivo")
    igual(hallazgos[0].frame, 25, "y el frame del montaje donde se oye")
}

do {
    let t = montajeConDosMedios()
    let hallazgos = t.buscarEnLoQueSeDice("cafe")
    igual(hallazgos.count, 2, "«cafe» sin acento encuentra «café» en los dos medios")
    igual(hallazgos[0].frame, 75, "el primero es el del montaje que suena antes")
    igual(hallazgos[1].mediaID, otroMedio, "y el segundo el del otro medio")
}

do {
    let t = montajeConDosMedios()
    igual(t.buscarEnLoQueSeDice("PRECIO").count, 1, "las mayúsculas no importan")
    igual(t.buscarEnLoQueSeDice("del café").count, 1, "una frase de dos palabras se busca seguida")
    igual(t.buscarEnLoQueSeDice("café del").count, 0, "y en el orden en el que se dijo")
    igual(t.buscarEnLoQueSeDice("").count, 0, "una búsqueda vacía no devuelve todo")
    igual(t.buscarEnLoQueSeDice("hipopótamo").count, 0, "lo que no se dijo no aparece")
}

do {
    let t = montajeConDosMedios()
    let hallazgo = t.buscarEnLoQueSeDice("café")[0]
    comprobar(hallazgo.contexto.contains("café"), "el contexto incluye lo buscado: \(hallazgo.contexto)")
    comprobar(hallazgo.contexto.contains("precio"), "y lo que se dijo antes")
    comprobar(hallazgo.contexto.contains("subió"), "y lo que se dijo después")
}

do {
    // Un medio transcrito que no está en el montaje se encuentra igual: buscar sirve
    // justamente para localizar el material que todavía no se ha usado.
    var t = LineaDeTiempo.nueva(timebase: tb)
    t.transcripciones = [Transcripcion(mediaID: medio, palabras: [
        Palabra(texto: "material", inicio: 2, duracion: 1),
    ])]
    let hallazgos = t.buscarEnLoQueSeDice("material")
    igual(hallazgos.count, 1, "se encuentra en un medio que no está montado")
    comprobar(hallazgos[0].frame == nil, "y no tiene frame de montaje porque no se oye en ninguno")
}

// MARK: - Persistencia

print("— persistencia —")

do {
    var t = LineaDeTiempo.nueva(timebase: tb)
    let v1 = t.pistas.first { $0.nombre == "V1" }!.id
    t.sobrescribir(Clip(mediaID: medio, nombre: "C", inicio: 0, duracion: 50, entradaEnOrigen: 0), enPista: v1, en: 0)
    t.transcripciones = [transcripcionSeguida(["hola", "mundo"])]

    let datos = try! JSONEncoder().encode(t)
    let vuelta = try! JSONDecoder().decode(LineaDeTiempo.self, from: datos)
    igual(vuelta.transcripciones, t.transcripciones, "el transcript sobrevive al JSON")

    // Un proyecto guardado antes de que esto existiera tiene que seguir abriéndose.
    var sinTranscript = t
    sinTranscript.transcripciones = nil
    let datosViejos = try! JSONEncoder().encode(sinTranscript)
    let vueltaVieja = try! JSONDecoder().decode(LineaDeTiempo.self, from: datosViejos)
    comprobar(vueltaVieja.transcripciones == nil, "un proyecto sin transcript abre igual")
    igual(vueltaVieja.duracion, t.duracion, "y conserva su montaje")
}

print("")
print(fallos == 0 ? "TODO CORRECTO" : "\(fallos) FALLOS")
exit(fallos == 0 ? 0 : 1)
