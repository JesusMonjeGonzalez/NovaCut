import AVFoundation
import Foundation

// MARK: - Base de tiempo

/// La cadencia del proyecto como fracción exacta.
///
/// 23,976 fps no existe: es 24000/1001. Guardar `23.976` como decimal parece
/// inofensivo hasta que, a los cuarenta minutos de montaje, el audio va tres
/// frames por delante de la imagen y no hay forma de saber por qué. Toda posición
/// del proyecto se cuenta en **frames enteros** sobre esta fracción, así que no
/// existe deriva posible: sumar dos mil clips es sumar dos mil enteros.
struct Timebase: Codable, Hashable, Sendable {
    var numerador: Int32
    var denominador: Int32
    /// Timecode con salto de frames, obligatorio en cadencias NTSC para que la hora
    /// de reloj y la hora de timecode no se separen 3,6 segundos por hora.
    var dropFrame: Bool

    init(numerador: Int32, denominador: Int32, dropFrame: Bool? = nil) {
        self.numerador = max(1, numerador)
        self.denominador = max(1, denominador)
        // El drop frame solo tiene sentido —y solo está definido— en las cadencias
        // NTSC, donde el denominador es 1001.
        self.dropFrame = dropFrame ?? (denominador == 1001)
    }

    static let p24 = Timebase(numerador: 24, denominador: 1)
    static let ntsc24 = Timebase(numerador: 24000, denominador: 1001)
    static let p25 = Timebase(numerador: 25, denominador: 1)
    static let ntsc30 = Timebase(numerador: 30000, denominador: 1001)
    static let p30 = Timebase(numerador: 30, denominador: 1)
    static let p50 = Timebase(numerador: 50, denominador: 1)
    static let ntsc60 = Timebase(numerador: 60000, denominador: 1001)
    static let p60 = Timebase(numerador: 60, denominador: 1)

    static let habituales: [Timebase] = [.p24, .ntsc24, .p25, .ntsc30, .p30, .p50, .ntsc60, .p60]

    var fps: Double { Double(numerador) / Double(denominador) }

    /// Frames por segundo redondeados, que es la cuenta que usa el timecode.
    var fpsNominal: Int { Int((fps).rounded()) }

    private var cuadrosDescartadosPorMinuto: Int64 {
        guard dropFrame else { return 0 }
        // SMPTE solo define estas cadencias para el timecode drop-frame.
        switch fpsNominal {
        case 30: return 2
        case 60: return 4
        default: return Int64((Double(fpsNominal) / 15).rounded())
        }
    }

    private var cuadrosPorDiezMinutosDrop: Int64 {
        let nominales = Int64(fpsNominal) * 60 * 10
        return nominales - cuadrosDescartadosPorMinuto * 9
    }

    private var cuadrosPorMinutoDrop: Int64 {
        Int64(fpsNominal) * 60 - cuadrosDescartadosPorMinuto
    }

    var nombre: String {
        let valor = fps
        let texto = abs(valor.rounded() - valor) < 0.001
            ? String(Int(valor.rounded()))
            : String(format: "%.3f", valor)
        return "\(texto) fps" + (dropFrame ? " DF" : "")
    }

    func segundos(_ frames: Int64) -> Double {
        Double(frames) * Double(denominador) / Double(numerador)
    }

    func frames(segundos: Double) -> Int64 {
        guard segundos.isFinite else { return 0 }
        return Int64((segundos * Double(numerador) / Double(denominador)).rounded())
    }

    /// `CMTime` exacto, sin pasar por coma flotante en ningún punto.
    func tiempo(_ frames: Int64) -> CMTime {
        CMTime(value: frames * Int64(denominador), timescale: numerador)
    }

    func frames(_ tiempo: CMTime) -> Int64 {
        guard tiempo.isNumeric else { return 0 }
        let escala = Int64(tiempo.timescale)
        let factor = Int64(numerador)
        let divisor = escala * Int64(denominador)
        let resultado = tiempo.value.multipliedReportingOverflow(by: factor)
        guard divisor > 0, !resultado.overflow else {
            return frames(segundos: tiempo.seconds)
        }
        let producto = resultado.partialValue
        let cociente = producto / divisor
        let resto = abs(producto % divisor)
        guard resto >= (divisor + 1) / 2 else { return cociente }
        return cociente + (producto >= 0 ? 1 : -1)
    }

    /// Timecode `HH:MM:SS:FF`, con `;` antes de los frames cuando hay drop frame.
    func timecode(_ frames: Int64) -> String {
        let fps = fpsNominal
        guard fps > 0 else { return "00:00:00:00" }
        var n = max(0, frames)

        if dropFrame {
            // Se descartan las etiquetas 00 y 01 al empezar cada minuto salvo
            // los múltiplos de diez. Los frames del medio nunca se descartan.
            let salto = cuadrosDescartadosPorMinuto
            let bloque = n / cuadrosPorDiezMinutosDrop
            let resto = n % cuadrosPorDiezMinutosDrop
            n += salto * 9 * bloque
            if resto > salto {
                n += salto * ((resto - salto) / cuadrosPorMinutoDrop)
            }
        }

        let f = Int(n % Int64(fps))
        let totalSegundos = n / Int64(fps)
        let s = Int(totalSegundos % 60)
        let m = Int((totalSegundos / 60) % 60)
        let h = Int(totalSegundos / 3600)
        let separador = dropFrame ? ";" : ":"
        return String(format: "%02d:%02d:%02d\(separador)%02d", h, m, s, f)
    }

    /// Lee `1:23:04:12`, `23:04:12`, `04:12` o un número suelto de frames.
    func frames(timecode texto: String) -> Int64? {
        let partes = texto.split(whereSeparator: { $0 == ":" || $0 == ";" }).map(String.init)
        guard !partes.isEmpty, partes.allSatisfy({ Int($0) != nil }) else { return nil }
        let numeros = partes.compactMap { Int64($0) }
        let fps = Int64(fpsNominal)
        guard numeros.allSatisfy({ $0 >= 0 }), fps > 0 else { return nil }

        switch numeros.count {
        case 1:
            return numeros[0]
        case 2:
            guard numeros[1] < fps else { return nil }
            return numeros[0] * fps + numeros[1]
        case 3:
            guard numeros[1] < 60, numeros[2] < fps else { return nil }
            return convertirTimecodeADesdeSegundos(segundos: numeros[0] * 60 + numeros[1], frame: numeros[2])
        case 4:
            guard numeros[1] < 60, numeros[2] < 60, numeros[3] < fps else { return nil }
            return convertirTimecodeADesdeSegundos(segundos: numeros[0] * 3600 + numeros[1] * 60 + numeros[2], frame: numeros[3])
        default: return nil
        }
    }

    private func convertirTimecodeADesdeSegundos(segundos: Int64, frame: Int64) -> Int64? {
        guard dropFrame else { return segundos * Int64(fpsNominal) + frame }

        let salto = cuadrosDescartadosPorMinuto
        let minutos = segundos / 60
        let segundoDelMinuto = segundos % 60
        // 00 y 01 no son etiquetas validas al principio de un minuto DF.
        if segundoDelMinuto == 0 && minutos % 10 != 0 && frame < salto { return nil }

        let etiquetas = segundos * Int64(fpsNominal) + frame
        let etiquetasDescartadas = salto * (minutos - minutos / 10)
        return etiquetas - etiquetasDescartadas
    }
}

// MARK: - Piezas del montaje

enum TipoDePista: String, Codable, Hashable, Sendable {
    case video
    case audio

    var esVideo: Bool { self == .video }
}

/// Transformación geométrica de un clip, en la convención de todos los NLE:
/// posición en píxeles desde el centro, escala en porcentaje, giro en grados.
struct TransformacionDeClip: Codable, Hashable, Sendable {
    var posicionX: Double = 0
    var posicionY: Double = 0
    var escala: Double = 100
    var rotacion: Double = 0
    var opacidad: Double = 100
    var recorteIzquierda: Double = 0
    var recorteDerecha: Double = 0
    var recorteArriba: Double = 0
    var recorteAbajo: Double = 0

    static let identidad = TransformacionDeClip()
    var esIdentidad: Bool { self == .identidad }
}

/// Un punto de control de una curva de color: entrada y salida en 0…1.
struct PuntoDeCurva: Codable, Hashable, Sendable {
    var x: Double
    var y: Double
}

/// Ruedas de color de un clip: sombras, medios y altas por canal.
///
/// Es el «color balance» de Photoshop aplicado a la manera de las ruedas de
/// Resolve: cada rueda desplaza un rango de luminancia hacia un tinte. Los
/// valores van de −1 a 1 y se convierten en una curva de tres puntos por canal
/// (0 = sombras, 0,5 = medios, 1 = altas), interpolada a la tabla de
/// `CIColorCurves` como las curvas RGB.
struct RuedasDeColor: Codable, Hashable, Sendable {
    /// Desplazamiento del canal rojo en sombras/medios/altas (−1…1).
    var sombrasRojo: Double = 0
    var sombrasVerde: Double = 0
    var sombrasAzul: Double = 0
    var mediosRojo: Double = 0
    var mediosVerde: Double = 0
    var mediosAzul: Double = 0
    var altasRojo: Double = 0
    var altasVerde: Double = 0
    var altasAzul: Double = 0

    static let neutras = RuedasDeColor()
    var esNeutro: Bool { self == .neutras }

    /// La curva de un canal desde sus tres ruedas: puntos (x, y) en 0…1.
    static func curvaDe(_ baja: Double, _ media: Double, _ alta: Double) -> [(Double, Double)] {
        // La y es el desplazamiento relativo al valor de entrada: un tinte de
        // +0,3 en sombras oscurece/acentúa el canal en la zona baja.
        [
            (0, min(max(baja * 0.5, -0.5), 0.5)),
            (0.5, min(max(media * 0.5, -0.5), 0.5)),
            (1, min(max(alta * 0.5, -0.5), 0.5)),
        ]
    }
}

/// Chroma key (pantalla verde o azul) de un clip.
///
/// `CIColorDistance` mide la distancia de cada píxel al color de clave; esa
/// distancia pasa por una curva de tolerancia que decide el alfa —dentro de la
/// tolerancia, transparente; fuera, opaco, con el suavizado como transición—.
/// El derrame (el reflejo del color de clave en los bordes del sujeto) se
/// suprime restando el color de clave proporcionalmente a (1 − alfa).
struct ChromaKeyDeClip: Codable, Hashable, Sendable {
    /// Color de la pantalla que se quiere hacer transparente (0…1).
    var rojo: Double = 0
    var verde: Double = 1
    var azul: Double = 0
    /// Cuánto del color de clave se elimina (0…1). Bajo, agresivo; alto, laxo.
    var tolerancia: Double = 0.4
    /// Transición entre transparente y opaco (0…1).
    var suavizado: Double = 0.15
    /// Cuánto se suprime el derrame del color de clave en los bordes (0…1).
    var suprimirDerrame: Double = 0.5

    var esNeutro: Bool {
        tolerancia >= 1 || (rojo == 0 && verde == 0 && azul == 0)
    }
}

/// Curvas RGB de un clip: una curva por canal y una de luminancia.
///
/// La luminancia se aplica primero a los tres canales y después cada canal su
/// propia curva —el orden del nodo de curvas de Resolve—. Las curvas viajan
/// como puntos de control editables en el proyecto; el render las interpola a
/// la tabla que espera `CIColorCurves`.
struct CurvasDeClip: Codable, Hashable, Sendable {
    var luma: [PuntoDeCurva] = [.init(x: 0, y: 0), .init(x: 1, y: 1)]
    var rojo: [PuntoDeCurva] = [.init(x: 0, y: 0), .init(x: 1, y: 1)]
    var verde: [PuntoDeCurva] = [.init(x: 0, y: 0), .init(x: 1, y: 1)]
    var azul: [PuntoDeCurva] = [.init(x: 0, y: 0), .init(x: 1, y: 1)]

    static let identidad = CurvasDeClip()

    var esIdentidad: Bool { self == .identidad }

    /// La tabla de 256×3 floats que espera `CIColorCurves`: cada canal con su
    /// curva propia y la de luminancia como curva maestra encima —primero se
    /// curva el canal, luego la luminancia actúa sobre el resultado, como la
    /// curva «maestra» de Photoshop. Multiplicar por la luminancia rompería la
    /// identidad (x·x ≠ x), que fue el fallo del primer intento.
    var tabla: Data {
        var datos = Data(capacity: 256 * 3 * MemoryLayout<Float>.size)
        for i in 0..<256 {
            let x = Double(i) / 255
            let r = ColorDeClip.interpolar(puntosDe(luma), en: ColorDeClip.interpolar(puntosDe(rojo), en: x))
            let g = ColorDeClip.interpolar(puntosDe(luma), en: ColorDeClip.interpolar(puntosDe(verde), en: x))
            let b = ColorDeClip.interpolar(puntosDe(luma), en: ColorDeClip.interpolar(puntosDe(azul), en: x))
            for v in [r, g, b] {
                let f = Float(min(max(v, 0), 1))
                datos.append(contentsOf: withUnsafeBytes(of: f) { Array($0) })
            }
        }
        return datos
    }

    private func puntosDe(_ puntos: [PuntoDeCurva]) -> [(Double, Double)] {
        puntos.sorted { $0.x < $1.x }.map { ($0.x, $0.y) }
    }
}

/// Corrección de color por clip. Son los mandos que resuelven el 90 % de los
/// casos; el resto es trabajo de una herramienta de color, no de un montaje.
struct ColorDeClip: Codable, Hashable, Sendable {
    var exposicion: Double = 0
    var contraste: Double = 0
    var saturacion: Double = 0
    var temperatura: Double = 0
    /// Altas luces: recupera o recorta el brillo de la zona clara.
    var altas: Double = 0
    /// Sombras: levanta o hunde la zona oscura.
    var sombras: Double = 0
    /// Curvas RGB opcionales del clip; `nil` significa sin curvas.
    var curvas: CurvasDeClip? = nil
    /// Ruedas de color (sombras/medios/altas por canal); `nil` = neutro.
    var ruedas: RuedasDeColor? = nil
    /// Viñeta: oscurecimiento en los bordes, en unidades de intensidad (0…1).
    var vignette: Double = 0
    /// Radio de la viñeta como fracción del lado corto (0,4 es muy cerrada,
    /// 0,8 casi invisible). Se ignora con `vignette` a cero.
    var radioDeVignette: Double = 0.75
    /// Desenfoque gaussiano del clip, en fracción del lado corto (0 = nítido).
    var desenfoque: Double = 0

    static let neutro = ColorDeClip()
    var esNeutro: Bool { self == .neutro }

    /// ¿La cadena de color tiene algo que aplicar?
    var tieneAjustes: Bool {
        !esNeutro || curvas?.esIdentidad == false || ruedas?.esNeutro == false
            || vignette > 0.001 || desenfoque > 0.001
    }

    /// Interpola linealmente una curva de puntos en la x pedida.
    static func interpolar(_ puntos: [(Double, Double)], en x: Double) -> Double {
        guard !puntos.isEmpty else { return x }
        if x <= puntos[0].0 { return puntos[0].1 }
        if x >= puntos[puntos.count - 1].0 { return puntos[puntos.count - 1].1 }
        for i in 1..<puntos.count where puntos[i].0 >= x {
            let a = puntos[i - 1]
            let b = puntos[i]
            let t = (x - a.0) / max(b.0 - a.0, 1e-9)
            return a.1 + (b.1 - a.1) * t
        }
        return puntos[puntos.count - 1].1
    }
}

/// Cómo se mezcla una capa con las que tiene debajo: el modo de fusión.
///
/// Es lo que separa un montaje plano de uno con profundidad: un clip en modo
/// «multiplicar» oscurece lo que hay debajo, uno en «pantalla» lo aclara, y el
/// «superponer» da el contraste de los looks de cine. Core Image traduce cada
/// modo a su filtro de mezcla; `ninguno` es la mezcla normal con opacidad.
enum ModoDeFusion: String, Codable, CaseIterable, Hashable, Sendable {
    case normal
    case multiplicar
    case pantalla
    case superponer
    case aclarar
    case oscurecer
    case colorDodge
    case colorBurn
    case luzFuerte
    case luzSuave
    case diferencia
    case exclusion
    case color
    case luminosidad

    var nombre: String {
        switch self {
        case .normal: "Normal"
        case .multiplicar: "Multiplicar"
        case .pantalla: "Pantalla"
        case .superponer: "Superponer"
        case .aclarar: "Aclarar"
        case .oscurecer: "Oscurecer"
        case .colorDodge: "Color dodge"
        case .colorBurn: "Color burn"
        case .luzFuerte: "Luz fuerte"
        case .luzSuave: "Luz suave"
        case .diferencia: "Diferencia"
        case .exclusion: "Exclusión"
        case .color: "Color"
        case .luminosidad: "Luminosidad"
        }
    }

    /// El filtro de Core Image que implementa el modo, o `nil` para normal.
    var filtroCI: String? {
        switch self {
        case .normal: nil
        case .multiplicar: "CIMultiplyBlendMode"
        case .pantalla: "CIScreenBlendMode"
        case .superponer: "CIOverlayBlendMode"
        case .aclarar: "CILightenBlendMode"
        case .oscurecer: "CIDarkenBlendMode"
        case .colorDodge: "CIColorDodgeBlendMode"
        case .colorBurn: "CIColorBurnBlendMode"
        case .luzFuerte: "CIHardLightBlendMode"
        case .luzSuave: "CISoftLightBlendMode"
        case .diferencia: "CIDifferenceBlendMode"
        case .exclusion: "CIExclusionBlendMode"
        case .color: "CIColorBlendMode"
        case .luminosidad: "CILuminosityBlendMode"
        }
    }
}

/// Máscara de un clip: una forma que recorta lo que se ve.
///
/// La máscara vive en el lienzo del proyecto (fracciones 0…1), no en el medio:
/// así sobrevive a cambios de orientación y de tamaño del original. El
/// rectángulo y la elipse son las dos formas que resuelven el 95 % de los usos
/// —enmarcar a una persona, aislar un objeto, hacer una ventana— y la pluma
/// suaviza el borde para que el recorte no delate la forma.
struct MascaraDeClip: Codable, Hashable, Sendable {
    enum Forma: String, Codable, CaseIterable, Sendable {
        case rectangulo
        case elipse

        var nombre: String {
            switch self {
            case .rectangulo: "Rectángulo"
            case .elipse: "Elipse"
            }
        }
    }

    var forma: Forma = .rectangulo
    /// Centro en fracciones del lienzo (0…1); 0,5 es el centro.
    var posicionX: Double = 0.5
    var posicionY: Double = 0.5
    /// Tamaño como fracción del lado correspondiente del lienzo (0…1).
    var tamanoX: Double = 0.5
    var tamanoY: Double = 0.5
    /// Pluma: suavizado del borde como fracción del tamaño menor.
    var pluma: Double = 0.1
    /// Invertida: se recorta todo menos la forma.
    var invertida: Bool = false

    static let inactiva = MascaraDeClip()
    var activa: Bool { tamanoX > 0.001 && tamanoY > 0.001 }

    init(forma: Forma = .rectangulo, posicionX: Double = 0.5, posicionY: Double = 0.5,
         tamanoX: Double = 0.5, tamanoY: Double = 0.5, pluma: Double = 0.1, invertida: Bool = false) {
        self.forma = forma
        self.posicionX = min(max(posicionX, 0), 1)
        self.posicionY = min(max(posicionY, 0), 1)
        self.tamanoX = min(max(tamanoX, 0), 1)
        self.tamanoY = min(max(tamanoY, 0), 1)
        self.pluma = min(max(pluma, 0), 1)
        self.invertida = invertida
    }
}

struct ClipKeyframe: Codable, Hashable, Sendable {
    /// Posición relativa al inicio del clip, en frames de proyecto.
    var frame: Int64
    var transformacion: TransformacionDeClip
    var ganancia: Double

    init(frame: Int64, transformacion: TransformacionDeClip, ganancia: Double) {
        self.frame = max(0, frame)
        self.transformacion = transformacion
        self.ganancia = ganancia
    }
}

enum TipoDeTransicion: String, Codable, CaseIterable, Hashable, Sendable {
    case disolucion
    case fundidoNegro

    var nombre: String {
        switch self {
        case .disolucion: "Disolución"
        case .fundidoNegro: "Fundido a negro"
        }
    }
}

struct Transicion: Codable, Hashable, Sendable {
    var tipo: TipoDeTransicion = .disolucion
    var duracion: Int64 = 12

    init(tipo: TipoDeTransicion = .disolucion, duracion: Int64 = 12) {
        self.tipo = tipo
        self.duracion = max(1, duracion)
    }
}

/// Un punto de retime: a partir de ese frame relativo al clip, la velocidad
/// interpolada va hacia el valor de este keyframe. Con velocidad 0 en un tramo,
/// ese tramo es un frame congelado —el freeze frame de cualquier NLE—.
struct RampaDeVelocidad: Codable, Hashable, Sendable {
    /// Frame relativo al inicio del clip donde empieza la rampa.
    var frame: Int64
    /// Velocidad en ese punto. 1 es normal; 0 congela.
    var velocidad: Double

    init(frame: Int64, velocidad: Double) {
        self.frame = max(0, frame)
        self.velocidad = max(0, velocidad)
    }
}

/// Un título quemado sobre el vídeo, en el tramo del clip.
///
/// Es la pieza de «Essential Graphics» más humilde y la más usada: texto con
/// posición, tamaño y color sobre la imagen. Los títulos no tienen medio: el
/// clip solo lleva el texto y el render lo dibuja encima de todo lo que haya
/// debajo, como un cartel en el plató.
struct TituloDeClip: Codable, Hashable, Sendable {
    var texto: String = ""
    /// Posición del centro, en fracción del lienzo (0…1). 0,5 es el centro.
    var posicionX: Double = 0.5
    var posicionY: Double = 0.5
    /// Tamaño del cuerpo en puntos del lienzo.
    var tamano: Double = 96
    /// Nombre de la fuente, o la fuente del sistema si no se encuentra.
    var fuente: String = "HelveticaNeue-Bold"
    var rojo: Double = 1
    var verde: Double = 1
    var azul: Double = 1
    /// Grosor del contorno en puntos; cero lo apaga.
    var contorno: Double = 0
    /// Fundido de entrada y salida en frames del proyecto.
    var fundido: Int64 = 12
    /// Si es una forma en vez de texto: rectángulo, elipse o línea.
    var forma: FormaDeTitulo = .texto
    /// Ruta al archivo de imagen cuando `forma == .imagen` (logotipo, PNG con
    /// transparencia…). Se dibuja con su relación de aspecto natural.
    var rutaDeImagen: String? = nil

    var color: ColorDeSubtitulo { ColorDeSubtitulo(rojo: rojo, verde: verde, azul: azul) }

    /// El ancho de la forma como fracción del lienzo (solo forma).
    var ancho: Double = 0.3
    /// El alto de la forma como fracción del lienzo (solo forma).
    var alto: Double = 0.2

    init(
        texto: String = "",
        posicionX: Double = 0.5, posicionY: Double = 0.5,
        tamano: Double = 96, fuente: String = "HelveticaNeue-Bold",
        rojo: Double = 1, verde: Double = 1, azul: Double = 1,
        contorno: Double = 0, fundido: Int64 = 12,
        forma: FormaDeTitulo = .texto,
        ancho: Double = 0.3, alto: Double = 0.2,
        rutaDeImagen: String? = nil
    ) {
        self.texto = texto
        self.posicionX = min(max(posicionX, 0), 1)
        self.posicionY = min(max(posicionY, 0), 1)
        self.tamano = max(8, tamano)
        self.fuente = fuente
        self.rojo = rojo
        self.verde = verde
        self.azul = azul
        self.contorno = contorno
        self.fundido = max(0, fundido)
        self.forma = forma
        self.ancho = min(max(ancho, 0.01), 1)
        self.alto = min(max(alto, 0.01), 1)
        self.rutaDeImagen = rutaDeImagen
    }
}

/// Qué dibuja un título: texto, una forma geométrica de relleno o una imagen.
enum FormaDeTitulo: String, Codable, CaseIterable, Hashable, Sendable {
    case texto
    case rectangulo
    case elipse
    case linea
    case imagen

    var nombre: String {
        switch self {
        case .texto: "Texto"
        case .rectangulo: "Rectángulo"
        case .elipse: "Elipse"
        case .linea: "Línea"
        case .imagen: "Imagen"
        }
    }
}

/// Un recorte con nombre de un medio: el subclip de Premiere.
///
/// Guarda el medio de origen y su rango; la biblioteca enseña el recorte como
/// un medio más, y los clips creados desde él empiezan en `entrada` con la
/// duración del recorte. Mismo archivo, dos recortes distintos, cero copias.
struct SubclipOrigen: Codable, Hashable, Sendable {
    var medioBase: UUID
    /// Frame del medio base donde empieza el recorte.
    var entrada: Int64
    /// Frame del medio base donde acaba el recorte (exclusivo).
    var salida: Int64

    init(medioBase: UUID, entrada: Int64, salida: Int64) {
        self.medioBase = medioBase
        self.entrada = max(0, entrada)
        self.salida = max(self.entrada + 1, salida)
    }

    var duracion: Int64 { salida - entrada }
}

struct Clip: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var mediaID: UUID
    var nombre: String = ""

    /// Frame de la línea de tiempo en el que empieza, dentro de su pista.
    var inicio: Int64
    var duracion: Int64
    /// Frame del medio original que se ve en el primer frame del clip.
    var entradaEnOrigen: Int64

    var habilitado = true
    /// 1 es velocidad normal. Negativa reproduce hacia atrás.
    var velocidad: Double = 1
    /// Ganancia en decibelios. 0 es el nivel original.
    var ganancia: Double = 0
    var entradaFundido: Int64 = 0
    var salidaFundido: Int64 = 0
    var transformacion = TransformacionDeClip.identidad
    var color = ColorDeClip.neutro
    /// Cómo se mezcla esta capa con las de debajo.
    var modoDeFusion: ModoDeFusion = .normal
    /// Máscara de recorte del clip, o `nil` para verlo entero.
    var mascara: MascaraDeClip? = nil
    /// Chroma key (pantalla verde/azul), o `nil` sin recorte de croma.
    var croma: ChromaKeyDeClip? = nil
    /// Une vídeo y audio del mismo plano: seleccionar o mover uno mueve al otro.
    var enlace: UUID?
    var etiqueta: EtiquetaDeColor = .ninguna
    /// Valores animados relativos al clip. Es opcional para abrir proyectos v2
    /// anteriores sin migración destructiva.
    var keyframes: [ClipKeyframe]? = nil
    /// Transiciones declaradas en los cortes de entrada y salida.
    var transicionEntrada: Transicion? = nil
    var transicionSalida: Transicion? = nil
    /// Si es un clip multicámara, qué ángulo manda en cada tramo.
    var multicam: MulticamDeClip? = nil
    /// Pista de ajuste: sin medio, su color (y su LUT) se aplican sobre todo lo
    /// que hay debajo en el tramo. Es el «adjustment layer» de cualquier NLE.
    var esAjuste: Bool = false
    /// Ruta al archivo `.cube` de la LUT del clip. Se aplica en el compositor,
    /// después de la cadena de color primaria.
    var lutDeColor: String? = nil
    /// Título: texto dibujado sobre el vídeo en el tramo del clip. Un clip de
    /// título no usa su medio (como una pista de ajuste no usa el suyo).
    var esTitulo: Bool = false
    var titulo: TituloDeClip? = nil
    /// Clip anidado: su contenido es otra línea de tiempo, como la secuencia
    /// dentro de una secuencia de Premiere. Al renderizar se expande en el
    /// tramo del clip; guardar el interior permite desanidarlo después.
    var nido: LineaDeTiempo? = nil

    var fin: Int64 { inicio + duracion }
    var salidaEnOrigen: Int64 { entradaEnOrigen + duracionEnOrigen }

    /// La velocidad en un frame relativo, interpolada entre rampas.
    ///
    /// Sin rampas devuelve `velocidad`. Con ellas, la velocidad viaja de un
    /// keyframe a otro en línea recta: una rampa de 1 → 3 en diez frames acelera
    /// suavemente, que es el retime que se ve en cualquier NLE grande.
    func velocidadEn(frame relativo: Int64) -> Double {
        guard let rampas = rampasDeVelocidad, !rampas.isEmpty else { return velocidad }
        let ordenadas = rampas.sorted { $0.frame < $1.frame }
        guard let anterior = ordenadas.last(where: { $0.frame <= relativo }) else { return velocidad }
        guard let siguiente = ordenadas.first(where: { $0.frame > relativo }) else {
            return anterior.velocidad
        }
        let t = Double(relativo - anterior.frame) / Double(siguiente.frame - anterior.frame)
        return anterior.velocidad + (siguiente.velocidad - anterior.velocidad) * t
    }

    /// Cuántos frames del medio consume el clip. A doble velocidad, un clip de
    /// 100 frames en la línea de tiempo se come 200 del original. Con rampas
    /// es la integral de la velocidad sobre la duración (trapezoidal, la misma
    /// interpolación lineal del modelo), y un congelado (velocidad 0) no
    /// consume nada.
    var duracionEnOrigen: Int64 {
        guard let rampas = rampasDeVelocidad, !rampas.isEmpty else {
            return Int64((Double(duracion) * abs(velocidad)).rounded())
        }
        // La integral de la velocidad es trapezoidal entre keyframes, con los
        // extremos del clip anclados: un keyframe en el último frame participa
        // en su tramo, que es donde el código anterior perdía la rampa final.
        let ordenadas = rampas.sorted { $0.frame < $1.frame }
        var puntos: [(frame: Int64, velocidad: Double)] = [(0, velocidadEn(frame: 0))]
        for rampa in ordenadas where rampa.frame > 0 && rampa.frame <= duracion {
            puntos.append((rampa.frame, velocidadEn(frame: rampa.frame)))
        }
        if puntos.last?.frame ?? 0 < duracion {
            puntos.append((duracion, velocidadEn(frame: duracion)))
        }
        var consumo = 0.0
        for i in 0..<(puntos.count - 1) {
            let a = puntos[i]
            let b = puntos[i + 1]
            consumo += Double(b.frame - a.frame) * (a.velocidad + b.velocidad) / 2
        }
        return Int64(consumo.rounded())
    }

    /// Los puntos donde cambia la velocidad del clip, relativos a su inicio.
    /// Opcional por compatibilidad con los proyectos anteriores.
    var rampasDeVelocidad: [RampaDeVelocidad]? = nil

    /// Divide el clip en tramos de velocidad constante-media, uno por intervalo
    /// entre rampas, con el consumo de origen de cada tramo ya resuelto.
    ///
    /// El render no sabe interpolar velocidad: AVFoundation escala tramos
    /// enteros con `scaleTimeRange`, así que el retime con rampas se expresa
    /// como tramos contiguos, cada uno con su trozo de origen y su duración en
    /// el montaje. Un tramo congelado (consumo 0) se renderiza insertando un
    /// solo frame de origen y estirándolo a su duración.
    func piezasDeVelocidad() -> [(desde: Int64, hasta: Int64, consumo: Int64)] {
        guard let rampas = rampasDeVelocidad, !rampas.isEmpty else {
            return [(0, duracion, duracionEnOrigen)]
        }
        let ordenadas = rampas.sorted { $0.frame < $1.frame }
        var bordes: [Int64] = [0]
        for rampa in ordenadas where rampa.frame > 0 && rampa.frame < duracion {
            bordes.append(rampa.frame)
        }
        bordes.append(duracion)

        var piezas: [(desde: Int64, hasta: Int64, consumo: Int64)] = []
        var posicionDeOrigen = 0.0
        var cursor = 0.0
        for i in 0..<(bordes.count - 1) {
            let a = bordes[i]
            let b = bordes[i + 1]
            // Integral trapezoidal del tramo, idéntica a `duracionEnOrigen`.
            cursor += Double(b - a) * (velocidadEn(frame: a) + velocidadEn(frame: b)) / 2
            let siguientePosicion = cursor
            let consumo = Int64((siguientePosicion - posicionDeOrigen).rounded())
            posicionDeOrigen = siguientePosicion
            piezas.append((a, b, max(0, consumo)))
        }
        return piezas
    }

    func contiene(_ frame: Int64) -> Bool { frame >= inicio && frame < fin }

    func transformacionEn(frame relativo: Int64) -> TransformacionDeClip {
        guard let keyframes, !keyframes.isEmpty else { return transformacion }
        let ordenadas = keyframes.sorted { $0.frame < $1.frame }
        guard let anterior = ordenadas.last(where: { $0.frame <= relativo }) else {
            return ordenadas[0].transformacion
        }
        guard let siguiente = ordenadas.first(where: { $0.frame >= relativo }), siguiente.frame != anterior.frame else {
            return anterior.transformacion
        }
        let t = Double(relativo - anterior.frame) / Double(siguiente.frame - anterior.frame)
        return Self.interpolar(anterior.transformacion, siguiente.transformacion, t: t)
    }

    func gananciaEn(frame relativo: Int64) -> Double {
        guard let keyframes, !keyframes.isEmpty else { return ganancia }
        let ordenadas = keyframes.sorted { $0.frame < $1.frame }
        guard let anterior = ordenadas.last(where: { $0.frame <= relativo }) else { return ordenadas[0].ganancia }
        guard let siguiente = ordenadas.first(where: { $0.frame >= relativo }), siguiente.frame != anterior.frame else {
            return anterior.ganancia
        }
        let t = Double(relativo - anterior.frame) / Double(siguiente.frame - anterior.frame)
        return anterior.ganancia + (siguiente.ganancia - anterior.ganancia) * t
    }

    /// Copia con los keyframes reemplazados, conservando el resto del clip.
    func withKeyframes(_ claves: [ClipKeyframe]) -> Clip {
        var copia = self
        copia.keyframes = claves.isEmpty ? nil : claves.sorted { $0.frame < $1.frame }
        return copia
    }

    private static func interpolar(
        _ a: TransformacionDeClip,
        _ b: TransformacionDeClip,
        t: Double
    ) -> TransformacionDeClip {
        func mezcla(_ x: Double, _ y: Double) -> Double { x + (y - x) * t }
        return TransformacionDeClip(
            posicionX: mezcla(a.posicionX, b.posicionX),
            posicionY: mezcla(a.posicionY, b.posicionY),
            escala: mezcla(a.escala, b.escala),
            rotacion: mezcla(a.rotacion, b.rotacion),
            opacidad: mezcla(a.opacidad, b.opacidad),
            recorteIzquierda: mezcla(a.recorteIzquierda, b.recorteIzquierda),
            recorteDerecha: mezcla(a.recorteDerecha, b.recorteDerecha),
            recorteArriba: mezcla(a.recorteArriba, b.recorteArriba),
            recorteAbajo: mezcla(a.recorteAbajo, b.recorteAbajo)
        )
    }

    func solapaCon(inicio otroInicio: Int64, fin otroFin: Int64) -> Bool {
        inicio < otroFin && otroInicio < fin
    }
}

enum EtiquetaDeColor: String, Codable, CaseIterable, Hashable, Sendable {
    case ninguna, rojo, naranja, amarillo, verde, azul, morado, rosa

    var nombre: String {
        switch self {
        case .ninguna: "Sin etiqueta"
        case .rojo: "Rojo"
        case .naranja: "Naranja"
        case .amarillo: "Amarillo"
        case .verde: "Verde"
        case .azul: "Azul"
        case .morado: "Morado"
        case .rosa: "Rosa"
        }
    }
}

struct Marcador: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var frame: Int64
    var nombre: String = ""
    var nota: String = ""
    var etiqueta: EtiquetaDeColor = .azul
    /// Un marcador con duración es un rango: sirve para señalar un tramo entero.
    var duracion: Int64 = 0
}

struct Subtitulo: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var inicio: Int64
    var fin: Int64
    var texto: String
    var estilo: String = "default"

    init(id: UUID = UUID(), inicio: Int64, fin: Int64, texto: String, estilo: String = "default") {
        self.id = id
        self.inicio = max(0, inicio)
        self.fin = max(self.inicio + 1, fin)
        self.texto = texto
        self.estilo = estilo
    }
}

/// Cómo se pinta un subtítulo al quemarlo, serializado con el proyecto.
///
/// El quemado es la única verdad que llega al vídeo: el monitor puede enseñar lo
/// que quiera, pero lo que se guarda es esto. Por eso el estilo viaja en el
/// archivo `.editorcito` y no en los ajustes de la aplicación —un proyecto que se
/// abre en otro Mac tiene que salir igual—.
struct EstiloDeSubtitulo: Codable, Hashable, Sendable {
    var nombre: String
    var fuente: String = "HelveticaNeue-Bold"
    var cuerpo: Double = 42
    var color: ColorDeSubtitulo = .blanco
    /// Grosor del contorno en puntos; cero lo apaga.
    var contorno: Double = 0
    /// Color del contorno.
    var colorDeContorno: ColorDeSubtitulo = .negro
    /// Fondo con opacidad; opacidad cero lo apaga.
    var fondo: ColorDeSubtitulo = .negro
    var opacidadDeFondo: Double = 0.72
    /// Posición vertical: 0 abajo, 0.5 centro, 1 arriba.
    var posicion: Double = 0
    /// Modo de resalte palabra a palabra (ninguno, palabra, línea).
    var modoDeResalte: ModoDeResalte = .ninguno
    /// Color con el que se pinta la palabra que suena.
    var colorDeResalte: ColorDeSubtitulo = .amarillo
    /// Márgenes seguros como fracción del lado corto.
    var margen: Double = 0.08

    static let porDefecto = EstiloDeSubtitulo(nombre: "default")
}

enum ModoDeResalte: String, Codable, Sendable {
    case ninguno, palabra, linea
}

/// Colores de subtítulo en componentes, para que viajen en el JSON sin depender
/// de la plataforma.
struct ColorDeSubtitulo: Codable, Hashable, Sendable {
    var rojo: Double
    var verde: Double
    var azul: Double

    static let blanco = ColorDeSubtitulo(rojo: 1, verde: 1, azul: 1)
    static let negro = ColorDeSubtitulo(rojo: 0, verde: 0, azul: 0)
    static let amarillo = ColorDeSubtitulo(rojo: 1, verde: 0.9, azul: 0.2)
    static let cyan = ColorDeSubtitulo(rojo: 0.3, verde: 0.9, azul: 1)
}

struct MediaBin: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var nombre: String
    var color: EtiquetaDeColor = .azul
    /// Filtro de un bin inteligente: solo entran los medios que lo cumplen.
    /// `nil` es un bin normal (los medios entran a mano). «VFR» y «Audio»
    /// tienen su significado; cualquier otra cosa busca en el nombre.
    var filtro: String? = nil

    var esInteligente: Bool { filtro != nil }

    /// ¿Este medio entra en el bin?
    func contiene(_ medio: (nombre: String, esVFR: Bool, esAudio: Bool)) -> Bool {
        guard let filtro = filtro?.lowercased(), !filtro.isEmpty else { return true }
        let nombre = medio.nombre.lowercased()
        switch filtro {
        case "vfr": return medio.esVFR
        case "audio": return medio.esAudio
        default: return nombre.contains(filtro)
        }
    }
}

/// Una palabra dicha, con su sitio **en el medio** y no en el montaje.
///
/// El tiempo va en segundos de origen a propósito: el reconocedor mide en segundos
/// del archivo, y el mismo archivo puede aparecer tres veces en el montaje, a otra
/// velocidad y recortado por otro sitio. Guardar frames de montaje aquí obligaría a
/// reescribir el transcript en cada corte, que es justo el error que hace que la
/// edición por texto se desincronice.
struct Palabra: Codable, Hashable, Sendable {
    var texto: String
    /// Segundo del medio en el que empieza.
    var inicio: Double
    var duracion: Double

    var fin: Double { inicio + duracion }
}

/// Lo que se dice en un medio, palabra a palabra.
struct Transcripcion: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var mediaID: UUID
    var palabras: [Palabra] = []
    /// Idioma con el que se reconoció, para no mezclar dos pasadas distintas.
    var idioma: String = "es-ES"
}

/// Cambio de ángulo dentro de un clip multicámara.
struct CorteDeAngulo: Codable, Hashable, Sendable {
    /// Frame relativo al inicio del clip desde el que este ángulo manda.
    var frame: Int64
    var mediaID: UUID

    init(frame: Int64, mediaID: UUID) {
        self.frame = max(0, frame)
        self.mediaID = mediaID
    }
}

/// Un clip que reproduce un ángulo distinto del grupo en cada tramo.
///
/// Es la forma en que Premiere y Resolve editan la entrevista de varias
/// cámaras: un solo clip en el timeline y los cortes de ángulo dentro de él.
/// El corte no parte el clip —el montaje lo sigue tratando como una pieza—;
/// quien renderiza cambia de origen en cada corte, y el clip de audio
/// enlazado sigue al mismo ángulo para que voz e imagen nunca se separen.
struct MulticamDeClip: Codable, Hashable, Sendable {
    var grupoID: UUID
    /// Ángulo con el que arranca el clip: sin cortes todavía hay que saber
    /// qué mostrar, y no se puede deducir del grupo sin guardar su orden.
    var inicial: UUID
    var cortes: [CorteDeAngulo] = []

    /// El ángulo que manda en ese frame relativo al inicio del clip.
    func medioActivo(en relativo: Int64) -> UUID {
        cortes.last { $0.frame <= relativo }?.mediaID ?? inicial
    }

    /// Tramos contiguos de ángulo sobre [0, duracion), para quien renderiza.
    func segmentos(duracion: Int64) -> [(desde: Int64, hasta: Int64, mediaID: UUID)] {
        guard duracion > 0 else { return [] }
        let ordenados = cortes.filter { $0.frame < duracion }.sorted { $0.frame < $1.frame }
        var salida: [(Int64, Int64, UUID)] = []
        var cursor: Int64 = 0
        for corte in ordenados where corte.frame > cursor {
            salida.append((cursor, corte.frame, medioActivo(en: cursor)))
            cursor = corte.frame
        }
        if cursor < duracion { salida.append((cursor, duracion, medioActivo(en: cursor))) }
        return salida
    }

    /// Cambia al ángulo pedido desde ese frame: los cortes posteriores quedan
    /// sin efecto (el nuevo mando los sustituye), y si el ángulo ya mandaba
    /// no se añade un corte inútil.
    mutating func cambiar(en frame: Int64, a mediaID: UUID) {
        guard frame > 0 else {
            inicial = mediaID
            cortes = []
            return
        }
        guard medioActivo(en: frame) != mediaID else { return }
        cortes.removeAll { $0.frame >= frame }
        cortes.append(CorteDeAngulo(frame: frame, mediaID: mediaID))
        cortes.sort { $0.frame < $1.frame }
    }
}

struct GrupoMulticam: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var nombre: String
    var mediaIDs: [UUID]
    var sincronizadoPorAudio: Bool = false
    /// Cuándo arranca el material de cada ángulo respecto al de referencia,
    /// en frames de proyecto. Sin esto, la sincronización solo vive en las
    /// posiciones de las pistas que se crearon al alinear, y un clip
    /// multicámara no sabría dónde empieza el material de cada cámara.
    var desfases: [UUID: Int64] = [:]
}

extension GrupoMulticam {
    /// El instante del material del ángulo que corresponde al instante de
    /// grupo dado, en frames de proyecto.
    ///
    /// Es la misma cuenta del constructor (`entradaOrigen = grupoT − desfase`):
    /// el ángulo que arranca `desfase` frames después tiene su comienzo ahí. El
    /// visor multiángulo la usa para enseñar cada cámara en su tiempo exacto, y
    /// como el render usa la misma función, lo que se ve en el visor es lo que
    /// se oirá al exportar.
    func posicionDeAngulo(_ medioID: UUID, enTiempoDeGrupo grupoT: Int64) -> Int64 {
        max(0, grupoT - (desfases[medioID] ?? 0))
    }

    /// Reinterpreta los desfases en otra base de tiempo conservando el tiempo real.
    ///
    /// Cambiar la cadencia del proyecto sin convertir los desfases descuadra la
    /// sincronía del grupo en silencio: cada ángulo se insertaría con su desfase
    /// en la escala antigua.
    static func convertirDesfases(
        _ desfases: [UUID: Int64],
        de anterior: Timebase,
        a nueva: Timebase
    ) -> [UUID: Int64] {
        var resultado: [UUID: Int64] = [:]
        for (medioID, desfase) in desfases {
            resultado[medioID] = nueva.frames(segundos: anterior.segundos(desfase))
        }
        return resultado
    }
}

struct Pista: Identifiable, Codable, Hashable, Sendable {
    var id = UUID()
    var tipo: TipoDePista
    var nombre: String
    var clips: [Clip] = []
    var silenciada = false
    var solo = false
    var bloqueada = false
    var visible = true
    var altura: Double = 58
    /// Mixer por pista. Opcional para que los proyectos v2 anteriores sigan siendo
    /// válidos y se comporten como una pista a 0 dB y centro.
    var volumen: Double? = nil
    var paneo: Double? = nil
    var ducking: Bool? = nil
    /// Pista cuya señal dispara el ducking de esta (el «lado» del sidechain).
    /// `nil` usa la primera pista de audio, que es la convención de voz.
    var fuenteDeDucking: UUID? = nil
    /// Cadena de mezcla: EQ, compresor y limiter. Opcionales por compatibilidad.
    var ecualizacion: [BandaDeEQ]? = nil
    var compresor: CompresorDePista? = nil
    var limitador: LimitadorDePista? = nil
    /// DSP de la cuarta tanda: puerta de ruido, multibanda, reverb y retardo,
    /// todos opcionales por compatibilidad de los proyectos guardados.
    var puertaDeRuido: PuertaDeRuidoDePista? = nil
    var multibanda: CompresorMultibandaDePista? = nil
    var reverb: ReverbDePista? = nil
    var retardo: RetardoDePista? = nil

    /// Si esta pista lleva alguna etapa de la cadena de mezcla (y merece tap).
    var tieneProcesamientoDeAudio: Bool {
        limitador != nil || compresor != nil
            || ecualizacion?.isEmpty == false
            || puertaDeRuido != nil || multibanda != nil || reverb != nil || retardo != nil
    }

    var volumenDB: Double { volumen ?? 0 }
    var paneoNormalizado: Double { paneo ?? 0 }
    var duckingActivo: Bool { ducking ?? false }

    /// Último frame ocupado por la pista.
    var fin: Int64 { clips.map(\.fin).max() ?? 0 }

    func clip(en frame: Int64) -> Clip? { clips.first { $0.contiene(frame) } }

    func clips(entre desde: Int64, y hasta: Int64) -> [Clip] {
        clips.filter { $0.solapaCon(inicio: desde, fin: hasta) }
    }

    mutating func ordenar() {
        clips.sort { $0.inicio < $1.inicio }
    }

    /// Deja libre un tramo de la pista: recorta o elimina lo que se cruce.
    ///
    /// Vive aquí y no en la línea de tiempo por una razón concreta del lenguaje:
    /// una función que reciba `&pistas[i]` mientras lee el resto de `self` viola el
    /// acceso exclusivo y aborta en tiempo de ejecución. Como método de `Pista` hay
    /// un único acceso y el problema desaparece.
    mutating func vaciarRango(desde: Int64, hasta: Int64) {
        var resultado: [Clip] = []
        for clip in clips {
            guard clip.solapaCon(inicio: desde, fin: hasta) else { resultado.append(clip); continue }

            if clip.inicio < desde {
                var izquierda = clip
                izquierda.duracion = desde - clip.inicio
                izquierda.salidaFundido = min(izquierda.salidaFundido, izquierda.duracion)
                if izquierda.duracion > 0 { resultado.append(izquierda) }
            }
            if clip.fin > hasta {
                // Reutiliza el mismo corte que la cuchilla para que keyframes y
                // rampas también se rebajen al nuevo inicio.
                var derecha = LineaDeTiempo.partir(clip, en: hasta).1
                derecha.entradaFundido = 0
                if derecha.duracion > 0 { resultado.append(derecha) }
            }
        }
        clips = resultado.sorted { $0.inicio < $1.inicio }
    }
}

// MARK: - Línea de tiempo

/// La herramienta activa, con los atajos de siempre.
///
/// Son las teclas que tiene en los dedos cualquiera que venga de Premiere o
/// Resolve. Inventar otras no aporta nada y obliga a reaprender lo único que un
/// montador ya sabe hacer sin mirar.
enum Herramienta: String, CaseIterable, Identifiable, Sendable {
    case seleccion, cuchilla, ripple, roll, slip, slide, mano

    var id: String { rawValue }

    var atajo: Character {
        switch self {
        case .seleccion: "v"
        case .cuchilla: "c"
        case .ripple: "b"
        case .roll: "n"
        case .slip: "y"
        case .slide: "u"
        case .mano: "h"
        }
    }

    var nombre: String {
        switch self {
        case .seleccion: "Selección"
        case .cuchilla: "Cuchilla"
        case .ripple: "Ripple"
        case .roll: "Roll"
        case .slip: "Slip"
        case .slide: "Slide"
        case .mano: "Mano"
        }
    }

    var icono: String {
        switch self {
        case .seleccion: "cursorarrow"
        case .cuchilla: "scissors"
        case .ripple: "arrow.left.and.right.square"
        case .roll: "arrow.left.arrow.right.square"
        case .slip: "arrow.left.and.right.circle"
        case .slide: "arrow.up.and.down.and.arrow.left.and.right"
        case .mano: "hand.raised"
        }
    }

    var modoDeRecorte: ModoDeRecorte {
        switch self {
        case .ripple: .ripple
        case .roll: .roll
        default: .normal
        }
    }
}

/// Con qué borde de un clip se está trabajando.
enum BordeDeClip: Sendable { case entrada, salida }

/// Modo de recorte, tal y como se llaman en la industria.
enum ModoDeRecorte: Sendable {
    /// Mueve el borde y deja hueco o lo abre.
    case normal
    /// Mueve el borde y arrastra todo lo que venga detrás.
    case ripple
    /// Mueve el corte entre dos clips: uno crece lo que el otro mengua.
    case roll
}

/// El montaje completo.
///
/// Es una estructura de valor a propósito: una instantánea para deshacer es una
/// copia, y no existe la posibilidad de que el historial guarde una referencia al
/// mismo objeto que se está editando —el fallo clásico que hace que deshacer
/// «no haga nada»—.
struct LineaDeTiempo: Codable, Hashable, Sendable {
    var timebase: Timebase = .p25
    var pistas: [Pista] = []
    var marcadores: [Marcador] = []
    /// Rango de trabajo para exportar solo un tramo.
    var entradaDeTrabajo: Int64?
    var salidaDeTrabajo: Int64?
    /// Campos opcionales añadidos después del formato v2 inicial.
    var subtitulos: [Subtitulo]? = nil
    /// Estilos de subtítulo disponibles. Siempre contiene al menos el «default»,
    /// que es el que usan los subtítulos que no dicen otra cosa.
    var estilosDeSubtitulo: [EstiloDeSubtitulo] = [.porDefecto]
    var bins: [MediaBin]? = nil
    var gruposMulticam: [GrupoMulticam]? = nil
    var transcripciones: [Transcripcion]? = nil

    var duracion: Int64 { pistas.map(\.fin).max() ?? 0 }

    var pistasDeVideo: [Pista] { pistas.filter { $0.tipo == .video } }
    var pistasDeAudio: [Pista] { pistas.filter { $0.tipo == .audio } }

    /// Firma de lo que **estructura** la composición: pistas, clips, dónde y
    /// cuánto. Los atributos (color, ganancia, keyframes, fundidos, mezcla) no
    /// entran: se resuelven en las instrucciones de vídeo y en la mezcla, que se
    /// reconstruyen sin tocar las pistas. Sirve para que el preview no rehaga
    /// la composición entera cuando lo único que cambió es un ajuste.
    var firmaDeComposicion: String {
        var partes: [String] = [timebase.nombre]
        for pista in pistas {
            partes.append("\(pista.tipo.rawValue):\(pista.id):\(pista.visible):\(pista.bloqueada):\(pista.solo)")
            for clip in pista.clips {
                let multicam = clip.multicam.map {
                    "\($0.grupoID):\($0.inicial):\($0.cortes.map { "\($0.frame)@\($0.mediaID)" }.joined(separator: ";"))"
                } ?? ""
                let rampas = (clip.rampasDeVelocidad ?? []).map { "\($0.frame)@\($0.velocidad)" }.joined(separator: ";")
                partes.append("\(clip.id):\(clip.mediaID):\(clip.inicio):\(clip.duracion):\(clip.entradaEnOrigen):\(clip.velocidad):\(clip.habilitado):\(clip.esAjuste):\(clip.esTitulo):\(multicam):\(rampas)")
            }
        }
        if let grupos = gruposMulticam {
            for grupo in grupos {
                partes.append("G:\(grupo.id):\(grupo.mediaIDs.map(\.uuidString).joined(separator: ",")):\(grupo.desfases.map { "\($0.key):\($0.value)" }.sorted().joined(separator: ","))")
            }
        }
        return partes.joined(separator: "|")
    }

    /// Montaje de partida: dos pistas de vídeo y cuatro de audio, que es lo que
    /// abre cualquier NLE y cubre entrevista con cámara B, música y ambiente.
    static func nueva(timebase: Timebase = .p25) -> LineaDeTiempo {
        LineaDeTiempo(
            timebase: timebase,
            pistas: [
                Pista(tipo: .video, nombre: "V2"),
                Pista(tipo: .video, nombre: "V1"),
                Pista(tipo: .audio, nombre: "A1"),
                Pista(tipo: .audio, nombre: "A2"),
                Pista(tipo: .audio, nombre: "A3"),
                Pista(tipo: .audio, nombre: "A4"),
            ]
        )
    }

    // MARK: Consultas

    /// Genera un EDL CMX3600 del montaje: el formato de listas de decisión de
    /// edición que cualquier editor de la industria importa.    ///
    /// Cada clip de vídeo y de audio se convierte en un evento con su entrada y
    /// salida en origen y en montaje, en timecode del proyecto. Es el puente de
    /// salida con Premiere, Resolve y las mesas de edición —un montaje hecho en
    /// Editorcito se puede terminar en otro lado, y viceversa. Se escribe la
    /// pista de vídeo primero y las de audio después, en el orden del montaje.
    func edl(nombreDeProyecto: String, nombresDeMedios: (UUID) -> String) -> String {
        let tc = timebase
        var lineas: [String] = [
            "TITLE: \(nombreDeProyecto)",
            "FCM: \(timebase.dropFrame ? "DROP FRAME" : "NON-DROP FRAME")",
        ]
        var numero = 0
        let eventos = todosLosClips.sorted { a, b in
            if a.clip.inicio != b.clip.inicio { return a.clip.inicio < b.clip.inicio }
            // El vídeo va antes que el audio en el EDL.
            let esVideoA = pista(a.pista)?.tipo == .video
            let esVideoB = pista(b.pista)?.tipo == .video
            if esVideoA != esVideoB { return esVideoA }
            return a.pista.uuidString < b.pista.uuidString
        }
        for (pistaID, clip) in eventos {
            guard clip.velocidad == 1 else { continue }
            numero += 1
            let tipo = pista(pistaID)?.tipo == .video ? "V" : "A"
            let reel = clip.nombre.replacingOccurrences(of: " ", with: "_")
            let entradaOrigen = tc.timecode(clip.entradaEnOrigen)
            let salidaOrigen = tc.timecode(clip.entradaEnOrigen + clip.duracion)
            let entradaMontaje = tc.timecode(clip.inicio)
            let salidaMontaje = tc.timecode(clip.fin)
            // CMX3600 alinea el reel en 8 caracteres y el tipo en 1. El formato
            // se compone por piezas: `String(format:)` con `%s` y strings de
            // Swift es comportamiento indefinido (crash confirmado).
            let reelAlineado = reel.padding(toLength: 8, withPad: " ", startingAt: 0)
            lineas.append("\(String(format: "%03d", numero))  \(reelAlineado) \(tipo)     C        \(entradaOrigen) \(salidaOrigen) \(entradaMontaje) \(salidaMontaje)")
            lineas.append("* FROM CLIP NAME: \(clip.nombre)")
        }
        lineas.append("")
        return lineas.joined(separator: "\n")
    }

    /// Describe en una frase qué cambió entre dos versiones del montaje, para
    /// el panel de historia de deshacer.
    ///
    /// Compara clip a clip por id: añadidos, quitados, movidos, recortados y
    /// ajustes (color, ganancia, transformación, fundidos). Sin ningún cambio
    /// reconocible devuelve una descripción genérica.
    static func describirCambio(antes: LineaDeTiempo, despues: LineaDeTiempo) -> String {
        let idsAntes = Set(antes.todosLosClips.map(\.clip.id))
        let idsDespues = Set(despues.todosLosClips.map(\.clip.id))
        let anadidos = idsDespues.subtracting(idsAntes)
        let quitados = idsAntes.subtracting(idsDespues)

        if anadidos.count == 1, let id = anadidos.first,
           let clip = despues.clip(id) {
            return "Añadido «\(clip.nombre)»"
        }
        if quitados.count == 1, let id = quitados.first,
           let clip = antes.clip(id) {
            return "Quitado «\(clip.nombre)»"
        }
        if anadidos.count > 0 || quitados.count > 0 {
            return "\(anadidos.count) añadido\(anadidos.count == 1 ? "" : "s"), \(quitados.count) quitado\(quitados.count == 1 ? "" : "s")"
        }

        // Los clips que siguen están: mira qué cambió en el primero que difiere.
        let comunes = despues.todosLosClips.compactMap { antes.clip($0.clip.id) != nil ? $0.clip : nil }
        for clip in comunes {
            guard let anterior = antes.clip(clip.id) else { continue }
            if anterior.inicio != clip.inicio { return "Movido «\(clip.nombre)»" }
            if anterior.duracion != clip.duracion { return "Recortado «\(clip.nombre)»" }
            if anterior.color != clip.color || anterior.lutDeColor != clip.lutDeColor
                || anterior.color.curvas != nil || clip.color.curvas != nil
                || anterior.color.ruedas != nil || clip.color.ruedas != nil {
                return "Color de «\(clip.nombre)»"
            }
            if anterior.ganancia != clip.ganancia { return "Ganancia de «\(clip.nombre)»" }
            if anterior.transformacion != clip.transformacion { return "Transformación de «\(clip.nombre)»" }
            if anterior.entradaFundido != clip.entradaFundido || anterior.salidaFundido != clip.salidaFundido {
                return "Fundidos de «\(clip.nombre)»"
            }
            if anterior.velocidad != clip.velocidad || anterior.rampasDeVelocidad != clip.rampasDeVelocidad {
                return "Velocidad de «\(clip.nombre)»"
            }
            if anterior.modoDeFusion != clip.modoDeFusion || anterior.mascara != clip.mascara || anterior.croma != clip.croma {
                return "Composición de «\(clip.nombre)»"
            }
        }
        if antes.marcadores.count != despues.marcadores.count { return "Marcadores" }
        if antes.timebase != despues.timebase { return "Base de tiempo" }
        return "Edición"
    }

    func indiceDePista(_ id: UUID) -> Int? { pistas.firstIndex { $0.id == id } }

    /// Índices (pista, clip) de un clip, para mutarlo desde fuera del modelo.
    func indiceDeClip(_ id: UUID) -> (Int, Int)? {
        for (indicePista, pista) in pistas.enumerated() {
            if let indiceClip = pista.clips.firstIndex(where: { $0.id == id }) {
                return (indicePista, indiceClip)
            }
        }
        return nil
    }

    func pista(_ id: UUID) -> Pista? { pistas.first { $0.id == id } }

    func clip(_ id: UUID) -> Clip? {
        for pista in pistas {
            if let encontrado = pista.clips.first(where: { $0.id == id }) { return encontrado }
        }
        return nil
    }

    func pistaDe(clip id: UUID) -> UUID? {
        pistas.first { $0.clips.contains { $0.id == id } }?.id
    }

    /// Todos los clips del montaje, con la pista a la que pertenecen.
    var todosLosClips: [(pista: UUID, clip: Clip)] {
        pistas.flatMap { pista in pista.clips.map { (pista.id, $0) } }
    }

    /// Clips enlazados con este, incluido él mismo.
    func grupoEnlazado(de id: UUID) -> [UUID] {
        guard let clip = clip(id) else { return [] }
        guard let enlace = clip.enlace else { return [id] }
        return todosLosClips.filter { $0.clip.enlace == enlace }.map(\.clip.id)
    }

    // MARK: Puntos de imán

    /// Todos los frames a los que merece la pena engancharse al arrastrar.
    ///
    /// El imán es lo que separa un montaje limpio de uno con huecos de un frame
    /// que nadie ve hasta que aparece un parpadeo negro en la exportación final.
    func puntosDeIman(excluyendo clipsExcluidos: Set<UUID> = [], cabezal: Int64? = nil) -> [Int64] {
        var puntos: Set<Int64> = [0]
        for pista in pistas {
            for clip in pista.clips where !clipsExcluidos.contains(clip.id) {
                puntos.insert(clip.inicio)
                puntos.insert(clip.fin)
            }
        }
        for marcador in marcadores {
            puntos.insert(marcador.frame)
            if marcador.duracion > 0 { puntos.insert(marcador.frame + marcador.duracion) }
        }
        if let cabezal { puntos.insert(cabezal) }
        if let entrada = entradaDeTrabajo { puntos.insert(entrada) }
        if let salida = salidaDeTrabajo { puntos.insert(salida) }
        return puntos.sorted()
    }

    /// Ajusta un frame al punto de imán más cercano dentro del umbral.
    func imantar(
        _ frame: Int64,
        umbral: Int64,
        excluyendo clipsExcluidos: Set<UUID> = [],
        cabezal: Int64? = nil
    ) -> Int64 {
        guard umbral > 0 else { return frame }
        let puntos = puntosDeIman(excluyendo: clipsExcluidos, cabezal: cabezal)
        var mejor = frame
        var mejorDistancia = umbral + 1
        for punto in puntos {
            let distancia = abs(punto - frame)
            if distancia < mejorDistancia {
                mejorDistancia = distancia
                mejor = punto
            }
        }
        return mejorDistancia <= umbral ? mejor : frame
    }

    // MARK: Edición

    /// Inserta un clip abriendo hueco: todo lo que venga después se desplaza.
    ///
    /// Es la edición por inserción de toda la vida. `enTodasLasPistas` reproduce el
    /// comportamiento de Premiere y Resolve, donde insertar corre también las otras
    /// pistas para que nada pierda su sincronía con la imagen.
    mutating func insertar(_ clip: Clip, enPista pistaID: UUID, en frame: Int64, enTodasLasPistas: Bool = true) {
        guard let indice = indiceDePista(pistaID), !pistas[indice].bloqueada else { return }
        let desplazamiento = clip.duracion
        guard desplazamiento > 0 else { return }

        for i in pistas.indices where !pistas[i].bloqueada {
            if i != indice && !enTodasLasPistas { continue }
            pistas[i].clips = pistas[i].clips.flatMap { existente -> [Clip] in
                if existente.inicio >= frame {
                    var movido = existente
                    movido.inicio += desplazamiento
                    return [movido]
                }
                if existente.contiene(frame) {
                    // El clip que cruza el punto de inserción se parte: la mitad
                    // derecha se va con el resto del material.
                    let (izquierda, derecha) = Self.partir(existente, en: frame)
                    var movida = derecha
                    movida.inicio += desplazamiento
                    return [izquierda, movida]
                }
                return [existente]
            }
        }

        var nuevo = clip
        nuevo.inicio = frame
        pistas[indice].clips.append(nuevo)
        pistas[indice].ordenar()
    }

    /// Escribe encima: lo que hubiera en ese tramo desaparece y nada se desplaza.
    mutating func sobrescribir(_ clip: Clip, enPista pistaID: UUID, en frame: Int64) {
        guard let indice = indiceDePista(pistaID), !pistas[indice].bloqueada else { return }
        var nuevo = clip
        nuevo.inicio = frame
        guard nuevo.duracion > 0 else { return }

        pistas[indice].vaciarRango(desde: nuevo.inicio, hasta: nuevo.fin)
        pistas[indice].clips.append(nuevo)
        pistas[indice].ordenar()
    }

    /// Parte por el frame indicado los clips de las pistas dadas.
    /// Devuelve los identificadores nuevos que aparecieron.
    @discardableResult
    mutating func partir(en frame: Int64, pistas pistasObjetivo: Set<UUID>? = nil) -> [UUID] {
        var objetivos = pistasObjetivo
        if let objetivosIniciales = pistasObjetivo {
            var ampliados = objetivosIniciales
            // Una cuchilla sobre el vídeo también corta su audio enlazado. Sin
            // esta expansión, el siguiente ripple o trim vuelve a separar ambos.
            let enlaces = Set(pistas.filter { objetivosIniciales.contains($0.id) }.flatMap { pista in
                pista.clips.filter { $0.contiene(frame) }.compactMap(\.enlace)
            })
            for pista in pistas where pista.clips.contains(where: { clip in
                clip.contiene(frame) && clip.enlace.map(enlaces.contains) == true
            }) {
                ampliados.insert(pista.id)
            }
            objetivos = ampliados
        }
        var creados: [UUID] = []
        for i in pistas.indices where !pistas[i].bloqueada {
            if let objetivo = objetivos, !objetivo.contains(pistas[i].id) { continue }
            guard let indiceClip = pistas[i].clips.firstIndex(where: { $0.contiene(frame) }) else { continue }
            // Cortar justo en el borde no crea nada: ya hay un corte ahí.
            let clip = pistas[i].clips[indiceClip]
            guard frame > clip.inicio, frame < clip.fin else { continue }
            let (izquierda, derecha) = Self.partir(clip, en: frame)
            pistas[i].clips[indiceClip] = izquierda
            pistas[i].clips.insert(derecha, at: indiceClip + 1)
            creados.append(derecha.id)
        }
        return creados
    }

    /// Quita un clip **dejando el hueco**. Es el `lift` de la industria.
    mutating func levantar(_ clipID: UUID) {
        let ids = idsEnlazados(con: clipID)
        for i in pistas.indices where !pistas[i].bloqueada {
            pistas[i].clips.removeAll { ids.contains($0.id) }
        }
    }

    /// Quita un clip **y cierra el hueco** arrastrando lo que venga detrás.
    mutating func borrarConArrastre(_ clipID: UUID) {
        guard let clip = clip(clipID) else { return }
        let ids = idsEnlazados(con: clipID)
        let miembros = pistas.enumerated().flatMap { indice, pista in
            pista.clips.filter { ids.contains($0.id) }.map { (indice, $0) }
        }
        guard !miembros.isEmpty, miembros.allSatisfy({ !pistas[$0.0].bloqueada }) else { return }
        let hueco = clip.duracion
        for i in pistas.indices {
            pistas[i].clips.removeAll { ids.contains($0.id) }
        }
        for (indice, miembro) in miembros {
            for i in pistas[indice].clips.indices where pistas[indice].clips[i].inicio >= miembro.fin {
                pistas[indice].clips[i].inicio -= hueco
            }
        }
    }

    /// Quita un **tramo de tiempo** de todas las pistas y cierra el hueco.
    ///
    /// Es el «extract» de cualquier NLE, y el motor de la edición por transcript:
    /// seleccionar texto es seleccionar un rango, y borrarlo tiene que dejar el
    /// montaje sin ese rango y sin agujero. Al correr todas las pistas por igual, el
    /// audio enlazado se mantiene alineado con su imagen sin tratarlo aparte.
    ///
    /// Los marcadores y los subtítulos viven en frames de montaje, así que también se
    /// corren: si no, señalarían otra cosa en cuanto se borra algo por delante, y esa
    /// deriva no da ningún error, solo confunde media hora después.
    mutating func extraerRango(desde: Int64, hasta: Int64) {
        guard hasta > desde else { return }
        let hueco = hasta - desde

        for i in pistas.indices where !pistas[i].bloqueada {
            pistas[i].vaciarRango(desde: desde, hasta: hasta)
            for j in pistas[i].clips.indices where pistas[i].clips[j].inicio >= hasta {
                pistas[i].clips[j].inicio -= hueco
            }
            pistas[i].ordenar()
        }

        marcadores = marcadores.compactMap { marcador in
            if marcador.frame >= hasta {
                var corrido = marcador
                corrido.frame -= hueco
                return corrido
            }
            return marcador.frame < desde ? marcador : nil
        }

        if let cues = subtitulos {
            subtitulos = cues.compactMap { cue in
                // Un subtítulo que empieza dentro del tramo se va con lo que decía.
                if cue.inicio >= desde && cue.inicio < hasta { return nil }
                guard cue.inicio >= hasta else { return cue }
                return Subtitulo(
                    id: cue.id,
                    inicio: cue.inicio - hueco,
                    fin: cue.fin - hueco,
                    texto: cue.texto,
                    estilo: cue.estilo
                )
            }
        }
    }

    /// Quita varios tramos de una vez.
    ///
    /// **De atrás hacia delante, y no es un detalle de estilo.** Cada borrado corre
    /// todo lo que viene detrás, así que aplicar los rangos en orden de lectura deja
    /// los siguientes apuntando a un sitio que ya se movió: se come material que nadie
    /// seleccionó y no salta ningún error. Empezando por el último, lo que queda por
    /// borrar está siempre por delante y ninguno se desplaza.
    mutating func extraerRangos(_ rangos: [(desde: Int64, hasta: Int64)]) {
        for rango in rangos.sorted(by: { $0.desde > $1.desde }) {
            extraerRango(desde: rango.desde, hasta: rango.hasta)
        }
    }

    /// Cierra todos los huecos de una pista pegando los clips entre sí.
    mutating func cerrarHuecos(enPista pistaID: UUID) {
        guard let indice = indiceDePista(pistaID), !pistas[indice].bloqueada else { return }
        pistas[indice].ordenar()
        var cursor: Int64 = 0
        for i in pistas[indice].clips.indices {
            pistas[indice].clips[i].inicio = cursor
            cursor += pistas[indice].clips[i].duracion
        }
    }

    /// El hueco que contiene ese frame en una pista, si lo hay.
    func hueco(enPista pistaID: UUID, en frame: Int64) -> (inicio: Int64, fin: Int64)? {
        guard let pista = pista(pistaID) else { return nil }
        let ordenados = pista.clips.sorted { $0.inicio < $1.inicio }
        guard !ordenados.isEmpty else { return nil }
        if ordenados.contains(where: { $0.contiene(frame) }) { return nil }

        var anterior: Int64 = 0
        for clip in ordenados {
            if frame < clip.inicio { return (anterior, clip.inicio) }
            anterior = max(anterior, clip.fin)
        }
        return nil
    }

    /// Mueve un clip a otro sitio, y si hace falta a otra pista.
    ///
    /// Sobrescribe lo que encuentre, que es lo que hace cualquier NLE al soltar un
    /// clip encima de otro. La alternativa —rechazar el movimiento— obligaría al
    /// usuario a hacer sitio a mano antes de cada arrastre.
    mutating func mover(_ clipID: UUID, aPista destinoID: UUID, en frame: Int64) {
        guard let clipInicial = clip(clipID),
              let origenIndice = indiceDePista(pistaDe(clip: clipID) ?? destinoID),
              let destinoIndice = indiceDePista(destinoID),
              !pistas[origenIndice].bloqueada, !pistas[destinoIndice].bloqueada,
              pistas[origenIndice].tipo == pistas[destinoIndice].tipo
        else { return }

        let ids = idsEnlazados(con: clipID)
        let desplazamiento = max(0, frame) - clipInicial.inicio
        var movimientos: [(indice: Int, clip: Clip)] = []

        for indice in pistas.indices {
            for original in pistas[indice].clips where ids.contains(original.id) {
                let indiceDestino = original.id == clipID ? destinoIndice : indice
                guard !pistas[indiceDestino].bloqueada,
                      pistas[indiceDestino].tipo == pistas[indice].tipo else { return }
                var movido = original
                movido.inicio = max(0, original.inicio + desplazamiento)
                movimientos.append((indiceDestino, movido))
            }
        }

        for indice in pistas.indices {
            pistas[indice].clips.removeAll { ids.contains($0.id) }
        }
        for movimiento in movimientos {
            pistas[movimiento.indice].vaciarRango(
                desde: movimiento.clip.inicio,
                hasta: movimiento.clip.fin
            )
            pistas[movimiento.indice].clips.append(movimiento.clip)
            pistas[movimiento.indice].ordenar()
        }
    }

    /// Segmentos del mismo enlace que representan el mismo tramo temporal.
    /// Tras dividir un clip, las dos mitades conservan el enlace pero tienen
    /// distinto inicio y duración, por eso no se mueven como una sola unidad.
    func idsEnlazados(con clipID: UUID) -> Set<UUID> {
        guard let objetivo = clip(clipID), let enlace = objetivo.enlace else { return [clipID] }
        return Set(pistas.flatMap { $0.clips }.filter {
            $0.enlace == enlace &&
            $0.inicio == objetivo.inicio &&
            $0.duracion == objetivo.duracion
        }.map(\.id))
    }

    /// Convierte un clip multicámara en clips normales, uno por tramo de
    /// ángulo. Es el «flatten» de Premiere o el «commit» de Resolve: cada
    /// tramo se convierte en un clip con su propio medio, su punto de entrada
    /// en origen calculado con el desfase del grupo, y sus propios enlaces de
    /// vídeo/audio. Devuelve los clips de vídeo creados.
    @discardableResult
    mutating func aplanarMulticam(clipID: UUID) -> [Clip] {
        guard let clip = clip(clipID), let multicam = clip.multicam,
              let grupo = gruposMulticam?.first(where: { $0.id == multicam.grupoID }),
              let pistaVideoID = pistaDe(clip: clipID) else { return [] }

        // El clip multicámara vive en el tiempo del grupo: su ventana es el
        // tramo [inicio, fin) de la línea de tiempo, y cada ángulo muestra en
        // el instante de grupo `t` su material en `t − desfase`. Por eso el
        // clip aplanado hereda `inicio + tramo.desde` y `entradaEnOrigen =
        // inicio + tramo.desde − desfase` —la misma cuenta del constructor—.
        let tramos = multicam.segmentos(duracion: clip.duracion)
        guard !tramos.isEmpty else { return [] }
        var creados: [Clip] = []

        // El audio enlazado del clip multicámara: misma instancia de
        // `multicam` (grupo y cortes) en una pista de audio. Se reparte por
        // los mismos tramos que el vídeo para que voz e imagen sigan juntas.
        var audioEnlazado: (indice: Int, clip: Clip)?
        for indice in pistas.indices where pistas[indice].tipo == .audio {
            if let audio = pistas[indice].clips.first(where: { $0.multicam == multicam }) {
                audioEnlazado = (indice, audio)
                break
            }
        }

        // Borra el clip multicámara y su audio enlazado.
        for indice in pistas.indices {
            pistas[indice].clips.removeAll { $0.id == clipID }
        }
        if let audio = audioEnlazado {
            pistas[audio.indice].clips.removeAll { $0.id == audio.clip.id }
        }

        for tramo in tramos {
            let desfase = grupo.desfases[tramo.mediaID] ?? 0
            let entradaEnOrigen = clip.inicio + tramo.desde - desfase
            // Sin material (desfase que deja el tramo antes del arranque del
            // ángulo) el tramo no se puede aplanar: el constructor ya lo
            // habría marcado como crítico al renderizar.
            guard entradaEnOrigen >= 0 else { continue }

            let enlace = UUID()
            var video = Clip(
                mediaID: tramo.mediaID,
                nombre: clip.nombre,
                inicio: clip.inicio + tramo.desde,
                duracion: tramo.hasta - tramo.desde,
                entradaEnOrigen: entradaEnOrigen,
                enlace: enlace
            )
            video.transformacion = clip.transformacion
            video.color = clip.color
            video.ganancia = clip.ganancia
            video.etiqueta = clip.etiqueta
            video.entradaFundido = min(clip.entradaFundido, video.duracion)
            video.salidaFundido = min(clip.salidaFundido, video.duracion)
            // Los keyframes viven en frames relativos al clip: al partir el
            // tramo hay que rebasarlos al inicio del tramo o animarían en el
            // sitio equivocado.
            video.keyframes = clip.keyframes?.compactMap { clave in
                guard clave.frame >= tramo.desde, clave.frame < tramo.hasta else { return nil }
                var rebasada = clave
                rebasada.frame -= tramo.desde
                return rebasada
            }
            video.multicam = nil

            if let indice = indiceDePista(pistaVideoID) {
                pistas[indice].clips.append(video)
                pistas[indice].ordenar()
            }
            creados.append(video)

            if let audio = audioEnlazado {
                var audioTramo = Clip(
                    mediaID: tramo.mediaID,
                    nombre: audio.clip.nombre,
                    inicio: video.inicio,
                    duracion: video.duracion,
                    entradaEnOrigen: entradaEnOrigen,
                    enlace: enlace
                )
                audioTramo.ganancia = audio.clip.ganancia
                audioTramo.etiqueta = audio.clip.etiqueta
                audioTramo.entradaFundido = min(audio.clip.entradaFundido, audioTramo.duracion)
                audioTramo.salidaFundido = min(audio.clip.salidaFundido, audioTramo.duracion)
                audioTramo.velocidad = audio.clip.velocidad
                pistas[audio.indice].clips.append(audioTramo)
                pistas[audio.indice].ordenar()
            }
        }
        return creados
    }

    /// Corrige a mano la sincronía de un ángulo del grupo, en frames.
    ///
    /// La sincronización automática usa el onset de la forma de onda, que
    /// falla con material sin audio o con música en vez de voz. Este ajuste
    /// manual de ±1 frame (o ±10 con ⌥) es el volante de sincronía de
    /// cualquier multicámara real. El desfase nunca baja de cero: pedir
    /// material antes del arranque de un ángulo es un aviso crítico.
    mutating func ajustarDesfase(grupoID: UUID, medioID: UUID, delta: Int64) -> Int64 {
        guard let indice = gruposMulticam?.firstIndex(where: { $0.id == grupoID }) else { return 0 }
        let actual = gruposMulticam?[indice].desfases[medioID] ?? 0
        let nuevo = max(0, actual + delta)
        gruposMulticam?[indice].desfases[medioID] = nuevo
        return nuevo
    }

    /// Recorta un borde del clip. Devuelve cuántos frames se movió de verdad.
    ///
    /// El movimiento se acota por el material disponible en el medio: no se puede
    /// alargar un clip más allá de donde acaba el archivo. Devolver lo aplicado en
    /// vez de un booleano permite que la interfaz enseñe el arrastre pegado al
    /// límite real en lugar de dejar de responder.
    @discardableResult
    mutating func recortar(
        _ clipID: UUID,
        borde: BordeDeClip,
        delta: Int64,
        modo: ModoDeRecorte,
        duracionDelMedio: Int64
    ) -> Int64 {
        guard let pistaID = pistaDe(clip: clipID),
              let p = indiceDePista(pistaID), !pistas[p].bloqueada,
              let c = pistas[p].clips.firstIndex(where: { $0.id == clipID }) else { return 0 }

        var clip = pistas[p].clips[c]
        var aplicado = delta

        switch borde {
        case .entrada:
            // No se puede entrar antes del principio del archivo ni pasarse del
            // final del propio clip: siempre tiene que quedar al menos un frame.
            aplicado = max(aplicado, -clip.entradaEnOrigen)
            aplicado = min(aplicado, clip.duracion - 1)
            if modo != .ripple { aplicado = max(aplicado, -clip.inicio) }
            guard aplicado != 0 else { return 0 }
            clip.entradaEnOrigen += Int64((Double(aplicado) * abs(clip.velocidad)).rounded())
            clip.inicio += aplicado
            clip.duracion -= aplicado

        case .salida:
            let disponible = duracionDelMedio > 0
                ? duracionDelMedio - clip.entradaEnOrigen - clip.duracionEnOrigen
                : Int64.max / 4
            aplicado = min(aplicado, disponible)
            aplicado = max(aplicado, -(clip.duracion - 1))
            guard aplicado != 0 else { return 0 }
            clip.duracion += aplicado
        }

        pistas[p].clips[c] = clip

        switch modo {
        case .normal:
            break
        case .ripple:
            // El montaje se acorta o se alarga justo lo recortado y no queda hueco.
            // Recortar por la entrada no mueve el clip —sigue pegado a su vecino de
            // la izquierda—, así que lo que se desplaza es toda la cola hacia atrás.
            if borde == .entrada {
                clip.inicio -= aplicado
                pistas[p].clips[c] = clip
            }
            let arrastre = borde == .entrada ? -aplicado : aplicado
            for i in pistas[p].clips.indices where i != c && pistas[p].clips[i].inicio >= clip.fin {
                pistas[p].clips[i].inicio += arrastre
            }
        case .roll:
            // El vecino del otro lado cede o gana justo lo mismo, así que la
            // duración total del montaje no cambia: solo se mueve el corte.
            ajustarVecino(enPista: p, deClip: c, borde: borde, delta: aplicado, duracionDelMedio: duracionDelMedio)
        }

        pistas[p].ordenar()
        return aplicado
    }

    /// Cambia qué trozo del medio se ve, sin mover el clip ni cambiar su duración.
    @discardableResult
    mutating func deslizarContenido(_ clipID: UUID, delta: Int64, duracionDelMedio: Int64) -> Int64 {
        guard let pistaID = pistaDe(clip: clipID),
              let p = indiceDePista(pistaID), !pistas[p].bloqueada,
              let c = pistas[p].clips.firstIndex(where: { $0.id == clipID }) else { return 0 }
        var clip = pistas[p].clips[c]
        let maximo = max(0, duracionDelMedio - clip.duracionEnOrigen)
        let destino = min(max(0, clip.entradaEnOrigen + delta), maximo)
        let aplicado = destino - clip.entradaEnOrigen
        guard aplicado != 0 else { return 0 }
        clip.entradaEnOrigen = destino
        pistas[p].clips[c] = clip
        return aplicado
    }

    /// Mueve el clip por la línea de tiempo comiéndose a sus vecinos: el de la
    /// izquierda se alarga o acorta lo mismo que el de la derecha al revés.
    @discardableResult
    mutating func deslizarPosicion(_ clipID: UUID, delta: Int64, duracionDelMedio: Int64) -> Int64 {
        guard let pistaID = pistaDe(clip: clipID),
              let p = indiceDePista(pistaID), !pistas[p].bloqueada,
              let c = pistas[p].clips.firstIndex(where: { $0.id == clipID }) else { return 0 }
        pistas[p].ordenar()
        guard let indice = pistas[p].clips.firstIndex(where: { $0.id == clipID }) else { return 0 }
        _ = c

        let anterior = indice > 0 ? indice - 1 : nil
        let siguiente = indice + 1 < pistas[p].clips.count ? indice + 1 : nil
        var aplicado = delta

        if let anterior {
            // El de la izquierda no puede quedarse sin frames.
            aplicado = max(aplicado, -(pistas[p].clips[anterior].duracion - 1))
        } else {
            aplicado = max(aplicado, -pistas[p].clips[indice].inicio)
        }
        if let siguiente {
            aplicado = min(aplicado, pistas[p].clips[siguiente].duracion - 1)
        }
        guard aplicado != 0 else { return 0 }

        pistas[p].clips[indice].inicio += aplicado
        if let anterior { pistas[p].clips[anterior].duracion += aplicado }
        if let siguiente {
            pistas[p].clips[siguiente].inicio += aplicado
            pistas[p].clips[siguiente].duracion -= aplicado
            let consumido = Int64((Double(aplicado) * abs(pistas[p].clips[siguiente].velocidad)).rounded())
            pistas[p].clips[siguiente].entradaEnOrigen += consumido
        }
        return aplicado
    }

    // MARK: Navegación

    /// Corte anterior o siguiente, contando todas las pistas visibles.
    /// Es la navegación con la que se monta de verdad: saltar de corte en corte.
    func corte(desde frame: Int64, haciaDelante: Bool) -> Int64? {
        var cortes: Set<Int64> = []
        for pista in pistas where pista.visible {
            for clip in pista.clips {
                cortes.insert(clip.inicio)
                cortes.insert(clip.fin)
            }
        }
        let ordenados = cortes.sorted()
        return haciaDelante
            ? ordenados.first { $0 > frame }
            : ordenados.last { $0 < frame }
    }

    func marcador(desde frame: Int64, haciaDelante: Bool) -> Marcador? {
        let ordenados = marcadores.sorted { $0.frame < $1.frame }
        return haciaDelante
            ? ordenados.first { $0.frame > frame }
            : ordenados.last { $0.frame < frame }
    }

    // MARK: Auxiliares

    private mutating func ajustarVecino(
        enPista p: Int,
        deClip c: Int,
        borde: BordeDeClip,
        delta: Int64,
        duracionDelMedio: Int64
    ) {
        let clip = pistas[p].clips[c]
        switch borde {
        case .entrada:
            guard let vecino = pistas[p].clips.firstIndex(where: { $0.fin == clip.inicio - delta }) else { return }
            pistas[p].clips[vecino].duracion += delta
        case .salida:
            guard let vecino = pistas[p].clips.firstIndex(where: { $0.inicio == clip.fin - delta }) else { return }
            pistas[p].clips[vecino].inicio += delta
            pistas[p].clips[vecino].duracion -= delta
            let consumido = Int64((Double(delta) * abs(pistas[p].clips[vecino].velocidad)).rounded())
            pistas[p].clips[vecino].entradaEnOrigen += consumido
        }
    }

    /// Parte un clip en dos por un frame absoluto de la línea de tiempo.
    static func partir(_ clip: Clip, en frame: Int64) -> (Clip, Clip) {
        var izquierda = clip
        var derecha = clip
        let recorrido = frame - clip.inicio

        izquierda.duracion = recorrido
        izquierda.salidaFundido = 0

        derecha.id = UUID()
        derecha.inicio = frame
        derecha.duracion = clip.fin - frame
        derecha.entradaEnOrigen += Int64((Double(recorrido) * abs(clip.velocidad)).rounded())
        derecha.entradaFundido = 0
        if let keyframes = clip.keyframes {
            izquierda.keyframes = keyframes.filter { $0.frame < recorrido }
            let estadoEnElCorte = ClipKeyframe(
                frame: 0,
                transformacion: clip.transformacionEn(frame: recorrido),
                ganancia: clip.gananciaEn(frame: recorrido)
            )
            derecha.keyframes = [estadoEnElCorte] + keyframes.compactMap { keyframe in
                guard keyframe.frame > recorrido else { return nil }
                var rebasado = keyframe
                rebasado.frame -= recorrido
                return rebasado
            }
        }
        if let rampas = clip.rampasDeVelocidad {
            izquierda.rampasDeVelocidad = rampas.filter { $0.frame < recorrido }
            let velocidadEnElCorte = RampaDeVelocidad(frame: 0, velocidad: clip.velocidadEn(frame: recorrido))
            derecha.rampasDeVelocidad = [velocidadEnElCorte] + rampas.compactMap { rampa in
                guard rampa.frame > recorrido else { return nil }
                var rebasada = rampa
                rebasada.frame -= recorrido
                return rebasada
            }
        }
        return (izquierda, derecha)
    }
}
