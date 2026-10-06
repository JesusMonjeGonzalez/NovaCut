//! Importación de FCPXML (Final Cut Pro, DaVinci Resolve): el camino de
//! vuelta del exportador de `mejoras.rs`.
//!
//! FCPXML es XML bien formado y aquí solo importan elementos y atributos,
//! así que basta un analizador mínimo sin dependencias. Del montaje se
//! traen cortes, pistas (carriles), entradas en origen y clips
//! desactivados; lo que el modelo de NovaCut no representa (títulos,
//! multicámara, cambios de velocidad, efectos) se cuenta en `avisos`.

use super::RoughClip;
use editorcito::Timebase;
use std::collections::HashMap;
use std::path::PathBuf;

/// Un elemento XML con sus atributos (ya decodificados) y sus hijos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nodo {
    pub nombre: String,
    pub atributos: Vec<(String, String)>,
    pub hijos: Vec<Nodo>,
}

impl Nodo {
    fn nuevo(nombre: &str) -> Self {
        Self {
            nombre: nombre.to_owned(),
            ..Default::default()
        }
    }

    pub fn atributo(&self, nombre: &str) -> Option<&str> {
        self.atributos
            .iter()
            .find(|(clave, _)| clave == nombre)
            .map(|(_, valor)| valor.as_str())
    }

    fn hijo(&self, nombre: &str) -> Option<&Nodo> {
        self.hijos.iter().find(|hijo| hijo.nombre == nombre)
    }

    /// Primer descendiente (en profundidad) con ese nombre.
    fn buscar(&self, nombre: &str) -> Option<&Nodo> {
        for hijo in &self.hijos {
            if hijo.nombre == nombre {
                return Some(hijo);
            }
            if let Some(encontrado) = hijo.buscar(nombre) {
                return Some(encontrado);
            }
        }
        None
    }
}

fn sin_cierre(que: &str) -> String {
    format!("XML incompleto: falta cerrar {que}")
}

/// Árbol del documento: un nodo raíz `#documento` con los elementos de
/// primer nivel. Ignora declaraciones, comentarios, DOCTYPE, CDATA y texto.
pub fn analizar_xml(texto: &str) -> Result<Nodo, String> {
    let bytes = texto.as_bytes();
    let mut pila: Vec<Nodo> = vec![Nodo::nuevo("#documento")];
    let mut i = 0;
    while let Some(desplazamiento) = texto[i..].find('<') {
        i += desplazamiento;
        let resto = &texto[i..];
        if resto.starts_with("<?") {
            i += resto.find("?>").ok_or_else(|| sin_cierre("una declaración"))? + 2;
            continue;
        }
        if resto.starts_with("<!--") {
            i += resto.find("-->").ok_or_else(|| sin_cierre("un comentario"))? + 3;
            continue;
        }
        if resto.starts_with("<![CDATA[") {
            i += resto.find("]]>").ok_or_else(|| sin_cierre("un bloque CDATA"))? + 3;
            continue;
        }
        if resto.starts_with("<!") {
            i += resto.find('>').ok_or_else(|| sin_cierre("el DOCTYPE"))? + 1;
            continue;
        }
        if resto.starts_with("</") {
            let fin = resto.find('>').ok_or_else(|| sin_cierre("una etiqueta"))?;
            let nombre = resto[2..fin].trim();
            let nodo = pila.pop().ok_or_else(|| format!("</{nombre}> sobra"))?;
            if nodo.nombre != nombre {
                return Err(format!("se esperaba </{}> y llegó </{nombre}>", nodo.nombre));
            }
            pila.last_mut()
                .ok_or_else(|| format!("</{nombre}> sobra"))?
                .hijos
                .push(nodo);
            i += fin + 1;
            continue;
        }
        // Etiqueta de apertura: nombre y atributos entre comillas.
        let mut j = i + 1;
        while j < bytes.len()
            && !bytes[j].is_ascii_whitespace()
            && bytes[j] != b'/'
            && bytes[j] != b'>'
        {
            j += 1;
        }
        let mut nodo = Nodo::nuevo(&texto[i + 1..j]);
        let mut vacia = false;
        loop {
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let Some(&actual) = bytes.get(j) else {
                return Err(sin_cierre(&format!("<{}>", nodo.nombre)));
            };
            if actual == b'/' {
                vacia = true;
                j += 1;
                continue;
            }
            if actual == b'>' {
                j += 1;
                break;
            }
            let inicio_nombre = j;
            while j < bytes.len()
                && bytes[j] != b'='
                && !bytes[j].is_ascii_whitespace()
                && bytes[j] != b'>'
            {
                j += 1;
            }
            let nombre = texto[inicio_nombre..j].to_owned();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if bytes.get(j) != Some(&b'=') {
                return Err(format!("el atributo «{nombre}» no tiene valor"));
            }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let comilla = match bytes.get(j) {
                Some(&c) if c == b'"' || c == b'\'' => c,
                _ => return Err(format!("el atributo «{nombre}» no va entre comillas")),
            };
            j += 1;
            let inicio_valor = j;
            while j < bytes.len() && bytes[j] != comilla {
                j += 1;
            }
            if j >= bytes.len() {
                return Err(sin_cierre(&format!("el atributo «{nombre}»")));
            }
            nodo.atributos
                .push((nombre, decodificar_entidades(&texto[inicio_valor..j])));
            j += 1;
        }
        i = j;
        if vacia {
            pila.last_mut()
                .ok_or_else(|| sin_cierre("el documento"))?
                .hijos
                .push(nodo);
        } else {
            pila.push(nodo);
        }
    }
    if pila.len() != 1 {
        let abierta = pila.last().map(|nodo| nodo.nombre.clone()).unwrap_or_default();
        return Err(sin_cierre(&format!("<{abierta}>")));
    }
    pila.pop().ok_or_else(|| "documento vacío".to_owned())
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;` y referencias numéricas.
fn decodificar_entidades(texto: &str) -> String {
    let mut salida = String::with_capacity(texto.len());
    let mut resto = texto;
    while let Some(posicion) = resto.find('&') {
        salida.push_str(&resto[..posicion]);
        let tras = &resto[posicion..];
        let Some(fin) = tras.find(';').filter(|fin| *fin <= 10) else {
            salida.push('&');
            resto = &tras[1..];
            continue;
        };
        let entidad = &tras[1..fin];
        let caracter = match entidad {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entidad
                .strip_prefix("#x")
                .or_else(|| entidad.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entidad.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match caracter {
            Some(caracter) => {
                salida.push(caracter);
                resto = &tras[fin + 1..];
            }
            None => {
                salida.push('&');
                resto = &tras[1..];
            }
        }
    }
    salida.push_str(resto);
    salida
}

/// Tiempo de FCPXML: `"1001/30000s"`, `"5s"` o `"0s"`.
pub fn tiempo(valor: &str) -> Option<f64> {
    let valor = valor.trim().strip_suffix('s')?;
    match valor.split_once('/') {
        Some((numerador, denominador)) => {
            let numerador: f64 = numerador.parse().ok()?;
            let denominador: f64 = denominador.parse().ok()?;
            (denominador != 0.0).then(|| numerador / denominador)
        }
        None => valor.parse().ok(),
    }
}

fn tiempo_de(nodo: &Nodo, atributo: &str) -> Option<f64> {
    nodo.atributo(atributo).and_then(tiempo)
}

/// Ruta local de una URL `file://` con los caracteres `%XX` decodificados.
fn ruta_de_url(url: &str) -> PathBuf {
    let sin_esquema = url.strip_prefix("file://").unwrap_or(url);
    let sin_host = sin_esquema.strip_prefix("localhost").unwrap_or(sin_esquema);
    let bytes = sin_host.as_bytes();
    let hexadecimal = |byte: u8| (byte as char).to_digit(16).map(|valor| valor as u8);
    let mut decodificado = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(alto), Some(bajo)) = (hexadecimal(bytes[i + 1]), hexadecimal(bytes[i + 2])) {
                decodificado.push(alto * 16 + bajo);
                i += 3;
                continue;
            }
        }
        decodificado.push(bytes[i]);
        i += 1;
    }
    let texto = String::from_utf8_lossy(&decodificado).into_owned();
    // `file:///C:/...` deja `/C:/...`: la barra inicial sobra en Windows.
    let b = texto.as_bytes();
    if b.len() > 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        PathBuf::from(&texto[1..])
    } else {
        PathBuf::from(texto)
    }
}

/// Un medio de `<resources>`.
#[derive(Clone, Debug)]
struct Medio {
    ruta: PathBuf,
    /// Instante de origen donde empieza el medio (timecode de cámara).
    inicio: f64,
    duracion: Option<f64>,
    video: bool,
    audio: bool,
}

/// Resultado de importar un FCPXML.
pub struct Importado {
    pub nombre: String,
    pub cadencia: Option<Timebase>,
    pub clips: Vec<RoughClip>,
    pub avisos: Vec<String>,
}

/// Un clip colocado en la timeline con su carril de FCPXML (0 la historia
/// principal, positivos encima, negativos audio debajo).
struct Colocado {
    clip: RoughClip,
    carril: i64,
}

#[derive(Default)]
struct Recuento {
    titulos: usize,
    transiciones: usize,
    velocidad: usize,
    compuestos: usize,
    sin_medio: usize,
}

pub fn importar_fcpxml(texto: &str) -> Result<Importado, String> {
    let documento = analizar_xml(texto)?;
    let raiz = documento
        .hijo("fcpxml")
        .ok_or_else(|| "no es un documento FCPXML".to_owned())?;
    let recursos = raiz.hijo("resources");
    let mut formatos: HashMap<String, Timebase> = HashMap::new();
    let mut medios: HashMap<String, Medio> = HashMap::new();
    for recurso in recursos.map(|nodo| nodo.hijos.as_slice()).unwrap_or(&[]) {
        let Some(id) = recurso.atributo("id") else {
            continue;
        };
        match recurso.nombre.as_str() {
            "format" => {
                if let Some(cadencia) = recurso.atributo("frameDuration").and_then(cadencia_de) {
                    formatos.insert(id.to_owned(), cadencia);
                }
            }
            "asset" => {
                let src = recurso
                    .atributo("src")
                    .or_else(|| recurso.hijo("media-rep").and_then(|rep| rep.atributo("src")));
                let Some(src) = src else {
                    continue;
                };
                medios.insert(
                    id.to_owned(),
                    Medio {
                        ruta: ruta_de_url(src),
                        inicio: tiempo_de(recurso, "start").unwrap_or(0.0),
                        duracion: tiempo_de(recurso, "duration").filter(|d| *d > 0.0),
                        video: recurso.atributo("hasVideo").is_some_and(|v| v == "1"),
                        audio: recurso.atributo("hasAudio").is_some_and(|v| v == "1"),
                    },
                );
            }
            _ => {}
        }
    }
    let proyecto = raiz.buscar("project");
    let secuencia = proyecto
        .and_then(|proyecto| proyecto.hijo("sequence"))
        .or_else(|| raiz.buscar("sequence"))
        .ok_or_else(|| "el documento no trae ninguna secuencia".to_owned())?;
    let nombre = proyecto
        .and_then(|proyecto| proyecto.atributo("name"))
        .filter(|nombre| !nombre.trim().is_empty())
        .unwrap_or("Secuencia importada")
        .to_owned();
    let mut cadencia = secuencia
        .atributo("format")
        .and_then(|id| formatos.get(id).copied());
    if secuencia.atributo("tcFormat") == Some("DF") {
        cadencia = cadencia.map(|base| {
            Timebase::new(base.numerator, base.denominator, true).unwrap_or(base)
        });
    }
    let inicio_tc = tiempo_de(secuencia, "tcStart").unwrap_or(0.0);
    let espina = secuencia
        .hijo("spine")
        .ok_or_else(|| "la secuencia no tiene historia principal (spine)".to_owned())?;
    let mut colocados = Vec::new();
    let mut recuento = Recuento::default();
    // La historia principal usa el tiempo de la secuencia, que empieza en
    // su timecode inicial (Resolve suele empezar en 01:00:00:00).
    for elemento in &espina.hijos {
        let posicion = tiempo_de(elemento, "offset").unwrap_or(0.0) - inicio_tc;
        colocar(elemento, posicion, 0, &medios, &mut colocados, &mut recuento);
    }
    let clips = asignar_pistas(colocados);
    let mut avisos = Vec::new();
    for (cuantos, que) in [
        (recuento.titulos, "título(s) no importados"),
        (recuento.transiciones, "transición(es) no importadas"),
        (recuento.velocidad, "cambio(s) de velocidad importados a velocidad normal"),
        (recuento.compuestos, "clip(s) compuestos o multicámara no importados"),
        (recuento.sin_medio, "clip(s) sin medio localizable"),
    ] {
        if cuantos > 0 {
            avisos.push(format!("{cuantos} {que}"));
        }
    }
    Ok(Importado {
        nombre,
        cadencia,
        clips,
        avisos,
    })
}

/// `frameDuration` («1001/30000s») como cadencia.
fn cadencia_de(duracion: &str) -> Option<Timebase> {
    let valor = duracion.trim().strip_suffix('s')?;
    let (numerador, denominador) = valor.split_once('/').unwrap_or((valor, "1"));
    let numerador: u32 = numerador.parse().ok()?;
    let denominador: u32 = denominador.parse().ok()?;
    // Un fotograma dura numerador/denominador: la cadencia es la inversa.
    Timebase::new(denominador, numerador, false).ok()
}

/// Coloca `elemento` (que empieza en `posicion` de la timeline) y sus clips
/// conectados. Un hijo conectado cae en `posicion + (offset − start)` del
/// elemento que lo lleva.
fn colocar(
    elemento: &Nodo,
    posicion: f64,
    carril_heredado: i64,
    medios: &HashMap<String, Medio>,
    colocados: &mut Vec<Colocado>,
    recuento: &mut Recuento,
) {
    let carril = elemento
        .atributo("lane")
        .and_then(|carril| carril.parse::<i64>().ok())
        .unwrap_or(carril_heredado);
    let inicio_local = tiempo_de(elemento, "start").unwrap_or(0.0);
    match elemento.nombre.as_str() {
        "asset-clip" => {
            let medio = elemento.atributo("ref").and_then(|id| medios.get(id));
            match medio {
                Some(medio) => {
                    let entrada = tiempo_de(elemento, "start").unwrap_or(medio.inicio);
                    anadir(elemento, medio, posicion, entrada, carril, colocados, recuento);
                }
                None => recuento.sin_medio += 1,
            }
        }
        "clip" => {
            // Clip de Final Cut: el medio va en un hijo `video` o `audio`.
            let interior = elemento
                .hijos
                .iter()
                .find(|hijo| (hijo.nombre == "video" || hijo.nombre == "audio") && hijo.atributo("ref").is_some());
            match interior.and_then(|hijo| hijo.atributo("ref").and_then(|id| medios.get(id)).map(|m| (hijo, m))) {
                Some((hijo, medio)) => {
                    let entrada_hijo = tiempo_de(hijo, "start").unwrap_or(medio.inicio);
                    let desfase = tiempo_de(hijo, "offset").unwrap_or(0.0);
                    let entrada = entrada_hijo + (inicio_local - desfase);
                    anadir(elemento, medio, posicion, entrada, carril, colocados, recuento);
                }
                None => recuento.sin_medio += 1,
            }
        }
        "gap" | "spine" => {}
        "title" => {
            recuento.titulos += 1;
            return;
        }
        "transition" => {
            recuento.transiciones += 1;
            return;
        }
        "mc-clip" | "ref-clip" | "sync-clip" => {
            recuento.compuestos += 1;
            return;
        }
        _ => return,
    }
    // Clips conectados y historias secundarias.
    for hijo in &elemento.hijos {
        match hijo.nombre.as_str() {
            "asset-clip" | "clip" | "gap" | "title" | "transition" | "mc-clip" | "ref-clip"
            | "sync-clip" => {
                let offset = tiempo_de(hijo, "offset").unwrap_or(inicio_local);
                let hijo_carril = if elemento.nombre == "spine" { carril } else { 0 };
                colocar(
                    hijo,
                    posicion + (offset - inicio_local),
                    hijo_carril,
                    medios,
                    colocados,
                    recuento,
                );
            }
            "spine" => {
                colocar(hijo, posicion, carril, medios, colocados, recuento);
            }
            _ => {}
        }
    }
}

fn anadir(
    elemento: &Nodo,
    medio: &Medio,
    posicion: f64,
    entrada_absoluta: f64,
    carril: i64,
    colocados: &mut Vec<Colocado>,
    recuento: &mut Recuento,
) {
    let Some(duracion) = tiempo_de(elemento, "duration").filter(|d| *d > 0.0) else {
        return;
    };
    if elemento.hijo("timeMap").is_some() {
        recuento.velocidad += 1;
    }
    let fuente = elemento.atributo("srcEnable").unwrap_or("all");
    let has_video = medio.video && fuente != "audio";
    let has_audio = medio.audio && fuente != "video";
    if !has_video && !has_audio {
        return;
    }
    let entrada = (entrada_absoluta - medio.inicio).max(0.0);
    colocados.push(Colocado {
        clip: RoughClip {
            path: medio.ruta.clone(),
            in_seconds: entrada,
            out_seconds: entrada + duracion,
            source_duration_seconds: medio.duracion,
            has_video,
            has_audio,
            timeline_start: posicion.max(0.0),
            enabled: elemento.atributo("enabled") != Some("0"),
            ..Default::default()
        },
        carril,
    });
}

/// Carriles → pistas. Vídeo: la historia principal es V1 y cada carril
/// positivo, una pista más; si la historia principal solo tiene huecos
/// (como en el FCPXML que exporta NovaCut), el carril 1 pasa a V1. Audio
/// suelto en carril −1 es A1, −2 es A2…
fn asignar_pistas(colocados: Vec<Colocado>) -> Vec<RoughClip> {
    let hay_video_principal = colocados
        .iter()
        .any(|colocado| colocado.clip.has_video && colocado.carril == 0);
    let desplazamiento = if hay_video_principal { 0 } else { 1 };
    colocados
        .into_iter()
        .map(|Colocado { mut clip, carril }| {
            let pista = if clip.has_video {
                (carril - desplazamiento).max(0)
            } else if carril < 0 {
                -carril - 1
            } else {
                carril
            };
            clip.track = pista.clamp(0, 15) as usize;
            clip
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_analizador_lee_atributos_hijos_y_entidades() {
        let documento = analizar_xml(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fcpxml>\n<!-- nota -->\n<a x=\"1 &amp; 2\" y='&lt;b&gt;'>\n  <b/>\n  <c z=\"&#233;&#x41;\">texto</c>\n</a>",
        )
        .unwrap();
        let a = &documento.hijos[0];
        assert_eq!(a.nombre, "a");
        assert_eq!(a.atributo("x"), Some("1 & 2"));
        assert_eq!(a.atributo("y"), Some("<b>"));
        assert_eq!(a.hijos.len(), 2);
        assert_eq!(a.hijos[1].atributo("z"), Some("éA"));
        assert!(analizar_xml("<a><b></a>").is_err());
        assert!(analizar_xml("<a x=1/>").is_err());
        assert!(analizar_xml("<a>").is_err());
    }

    #[test]
    fn tiempos_y_rutas_de_fcpxml() {
        assert_eq!(tiempo("1001/30000s"), Some(1001.0 / 30000.0));
        assert_eq!(tiempo("5s"), Some(5.0));
        assert_eq!(tiempo("0s"), Some(0.0));
        assert_eq!(tiempo("5"), None);
        assert_eq!(cadencia_de("1001/30000s"), Some(Timebase::new(30000, 1001, false).unwrap()));
        assert_eq!(cadencia_de("1/25s"), Some(Timebase::P25));
        assert_eq!(
            ruta_de_url("file:///C:/Medios/Entrevista%201.mov"),
            PathBuf::from("C:/Medios/Entrevista 1.mov")
        );
        assert_eq!(
            ruta_de_url("file:///Users/ana/M%C3%BAsica.wav"),
            PathBuf::from("/Users/ana/Música.wav")
        );
        assert_eq!(ruta_de_url("file://localhost/Volumes/X/a.mov"), PathBuf::from("/Volumes/X/a.mov"));
        assert_eq!(ruta_de_url("file:///C:/100%.mov"), PathBuf::from("C:/100%.mov"));
    }

    /// Como lo exporta DaVinci Resolve: timecode en 01:00:00:00, medios con
    /// timecode de cámara y un clip conectado encima.
    #[test]
    fn un_montaje_de_resolve_se_importa_con_sus_pistas_y_entradas() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE fcpxml>
<fcpxml version="1.10">
  <resources>
    <format id="r0" frameDuration="1/25s" width="1920" height="1080"/>
    <asset id="r1" name="A" start="3600s" duration="120s" hasVideo="1" hasAudio="1" format="r0">
      <media-rep kind="original-media" src="file:///D:/Rodaje/A.mov"/>
    </asset>
    <asset id="r2" name="B" start="0s" duration="60s" hasVideo="1" hasAudio="0">
      <media-rep kind="original-media" src="file:///D:/Rodaje/B.mov"/>
    </asset>
  </resources>
  <library>
    <event name="Rodaje">
      <project name="Entrevista">
        <sequence format="r0" tcStart="3600s" tcFormat="NDF">
          <spine>
            <asset-clip ref="r1" offset="3600s" start="3610s" duration="5s" name="A">
              <asset-clip ref="r2" lane="1" offset="3612s" start="4s" duration="2s" name="B"/>
            </asset-clip>
            <transition offset="3604s" duration="1s"/>
            <asset-clip ref="r1" offset="3605s" start="3630s" duration="3s" enabled="0"/>
            <title offset="3605s" duration="2s" lane="2"/>
          </spine>
        </sequence>
      </project>
    </event>
  </library>
</fcpxml>"#;
        let importado = importar_fcpxml(xml).unwrap();
        assert_eq!(importado.nombre, "Entrevista");
        assert_eq!(importado.cadencia, Some(Timebase::P25));
        assert_eq!(importado.clips.len(), 3);
        let a = &importado.clips[0];
        assert_eq!(a.path, PathBuf::from("D:/Rodaje/A.mov"));
        assert!((a.timeline_start - 0.0).abs() < 1e-9);
        assert!((a.in_seconds - 10.0).abs() < 1e-9);
        assert!((a.duration() - 5.0).abs() < 1e-9);
        assert_eq!(a.track, 0);
        assert!(a.has_video && a.has_audio);
        let b = &importado.clips[1];
        assert!((b.timeline_start - 2.0).abs() < 1e-9, "{}", b.timeline_start);
        assert!((b.in_seconds - 4.0).abs() < 1e-9);
        assert_eq!(b.track, 1);
        assert!(!b.has_audio);
        let desactivado = &importado.clips[2];
        assert!((desactivado.timeline_start - 5.0).abs() < 1e-9);
        assert!((desactivado.in_seconds - 30.0).abs() < 1e-9);
        assert!(!desactivado.enabled);
        assert_eq!(importado.avisos.len(), 2, "{:?}", importado.avisos);
    }

    #[test]
    fn un_clip_de_final_cut_con_video_interior_se_importa() {
        let xml = r#"<fcpxml version="1.9">
  <resources>
    <format id="f" frameDuration="1001/30000s"/>
    <asset id="a" start="0s" duration="20s" hasVideo="1" hasAudio="1" src="file:///Users/ana/plano.mov"/>
  </resources>
  <library><event><project name="Corto"><sequence format="f" tcFormat="DF"><spine>
    <clip offset="0s" start="2s" duration="3s"><video ref="a" offset="0s" start="1s" duration="20s"/></clip>
  </spine></sequence></project></event></library>
</fcpxml>"#;
        let importado = importar_fcpxml(xml).unwrap();
        assert_eq!(importado.cadencia, Some(Timebase::NTSC30));
        assert_eq!(importado.clips.len(), 1);
        // Entrada: el vídeo interior empieza en 1 s y el clip toma desde su 2 s.
        assert!((importado.clips[0].in_seconds - 3.0).abs() < 1e-9);
        assert!((importado.clips[0].duration() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn un_documento_que_no_es_fcpxml_se_rechaza() {
        assert!(importar_fcpxml("<xmeml/>").is_err());
        assert!(importar_fcpxml("<fcpxml><resources/></fcpxml>").is_err());
    }
}
