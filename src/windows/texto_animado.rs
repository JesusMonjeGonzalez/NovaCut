//! Animación de títulos letra a letra (o palabra a palabra), como los
//! textos animados de Premiere y CapCut: máquina de escribir, aparecer,
//! subir. Cada letra es un `drawtext` propio colocado con las medidas reales
//! de la fuente; en exportación la opacidad y la altura son expresiones en
//! `t` (tiempo local del título) y en el monitor se calculan para el
//! instante que se ve.

use super::subtitulos_animados::{load_font, text_width};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimacionTexto {
    #[default]
    Ninguna,
    /// Cada letra aparece de golpe, una tras otra.
    MaquinaDeEscribir,
    /// Cada letra se funde desde transparente.
    AparecerPorLetras,
    /// Cada letra sube un poco mientras aparece.
    SubirPorLetras,
    /// Palabra a palabra, fundiéndose.
    AparecerPorPalabras,
}

impl AnimacionTexto {
    pub const TODAS: [AnimacionTexto; 5] = [
        AnimacionTexto::Ninguna,
        AnimacionTexto::MaquinaDeEscribir,
        AnimacionTexto::AparecerPorLetras,
        AnimacionTexto::SubirPorLetras,
        AnimacionTexto::AparecerPorPalabras,
    ];

    pub fn nombre(self) -> &'static str {
        match self {
            AnimacionTexto::Ninguna => "Sin animación",
            AnimacionTexto::MaquinaDeEscribir => "Máquina de escribir",
            AnimacionTexto::AparecerPorLetras => "Aparecer por letras",
            AnimacionTexto::SubirPorLetras => "Subir por letras",
            AnimacionTexto::AparecerPorPalabras => "Aparecer por palabras",
        }
    }
}

/// Lo que necesita cada letra para dibujarse con el estilo del título.
pub struct Estilo<'a> {
    pub fuente: &'a Path,
    /// Ruta de la fuente ya escapada para el grafo.
    pub fuente_escapada: &'a str,
    pub escapar: &'a dyn Fn(&str) -> String,
    pub tamano: f64,
    pub color: &'a str,
    /// Centro del bloque como fracción del lienzo, como `drawtext` normal.
    pub posicion: (f64, f64),
    pub lienzo: (u32, u32),
    /// Contorno, sombra… en el formato `:clave=valor` de `drawtext`.
    pub extra: String,
    /// Duración del título para la salida; `None` si no hay salida.
    pub salida: Option<f64>,
}

/// Unidad que aparece de una vez: una letra o una palabra, ya colocada.
#[derive(Clone, Debug, PartialEq)]
pub struct Pieza {
    pub texto: String,
    pub x: f64,
    pub y: f64,
    pub orden: usize,
}

const FUNDIDO: f64 = 0.25;
const SALIDA: f64 = 0.4;

/// Retardo entre piezas: rápido en textos cortos, y la entrada entera
/// nunca dura más de ~1,2 s aunque el texto sea largo.
fn escalon(piezas: usize, animacion: AnimacionTexto) -> f64 {
    let base: f64 = if animacion == AnimacionTexto::AparecerPorPalabras { 0.18 } else { 0.05 };
    base.min(1.2 / piezas.max(1) as f64)
}

/// Coloca letras (o palabras) del texto centrado en `posicion`, con la
/// altura de línea de `drawtext` (1,2 em).
pub fn colocar(texto: &str, animacion: AnimacionTexto, medir: &dyn Fn(&str) -> f64, tamano: f64, centro: (f64, f64)) -> Vec<Pieza> {
    let lineas: Vec<&str> = texto.lines().collect();
    let alto_linea = tamano * 1.2;
    let alto = alto_linea * lineas.len().max(1) as f64;
    let mut piezas = Vec::new();
    let mut orden = 0;
    for (n, linea) in lineas.iter().enumerate() {
        let izquierda = centro.0 - medir(linea) / 2.0;
        let y = centro.1 - alto / 2.0 + n as f64 * alto_linea;
        if animacion == AnimacionTexto::AparecerPorPalabras {
            let mut desde = 0;
            for palabra in linea.split(' ') {
                if !palabra.is_empty() {
                    piezas.push(Pieza {
                        texto: palabra.to_owned(),
                        x: izquierda + medir(&linea[..desde]),
                        y,
                        orden,
                    });
                    orden += 1;
                }
                desde += palabra.len() + 1;
            }
        } else {
            for (byte, letra) in linea.char_indices() {
                if letra.is_whitespace() {
                    continue;
                }
                piezas.push(Pieza {
                    texto: letra.to_string(),
                    x: izquierda + medir(&linea[..byte]),
                    y,
                    orden,
                });
                orden += 1;
            }
        }
    }
    piezas
}

/// Opacidad de la pieza `i` en el instante `t` (para el monitor) y su
/// expresión en `t` (para exportar); igual fórmula en los dos.
fn opacidad(animacion: AnimacionTexto, i: usize, total: usize, salida: Option<f64>, t: Option<f64>) -> String {
    let paso = escalon(total, animacion);
    let inicio = i as f64 * paso;
    let entrada = |t: f64| match animacion {
        AnimacionTexto::MaquinaDeEscribir => f64::from(t >= inicio),
        _ => ((t - inicio) / FUNDIDO).clamp(0.0, 1.0),
    };
    // La salida es una ola como la entrada: la primera en llegar se va primero.
    let fin = salida.map(|duracion| duracion - SALIDA - (total - 1 - i) as f64 * paso * 0.5);
    match t {
        Some(t) => {
            let mut valor = entrada(t);
            if let Some(fin) = fin {
                valor *= ((fin + SALIDA - t) / SALIDA).clamp(0.0, 1.0);
            }
            format!("{valor:.4}")
        }
        None => {
            let mut expresion = match animacion {
                AnimacionTexto::MaquinaDeEscribir => format!("gte(t\\,{inicio:.4})"),
                _ => format!("clip((t-{inicio:.4})/{FUNDIDO}\\,0\\,1)"),
            };
            if let Some(fin) = fin {
                expresion = format!("{expresion}*clip(({:.4}-t)/{SALIDA}\\,0\\,1)", fin + SALIDA);
            }
            expresion
        }
    }
}

/// Altura de la pieza: en «Subir», parte medio cuerpo más abajo.
fn altura(animacion: AnimacionTexto, pieza: &Pieza, total: usize, tamano: f64, t: Option<f64>) -> String {
    if animacion != AnimacionTexto::SubirPorLetras {
        return format!("{:.1}", pieza.y);
    }
    let inicio = pieza.orden as f64 * escalon(total, animacion);
    let caida = tamano * 0.45;
    match t {
        Some(t) => format!("{:.1}", pieza.y + (1.0 - ((t - inicio) / 0.3).clamp(0.0, 1.0)) * caida),
        None => format!("{:.1}+(1-clip((t-{inicio:.4})/0.3\\,0\\,1))*{caida:.1}", pieza.y),
    }
}

/// ¿Admite este FFmpeg `y_align`? (6.1+). Sin él, cada letra se alinea por
/// su propia caja y las bajas («g», «y») bailarían; se usa si está.
fn admite_y_align() -> bool {
    static ADMITE: OnceLock<bool> = OnceLock::new();
    *ADMITE.get_or_init(|| {
        #[cfg(windows)]
        use std::os::windows::process::CommandExt;
        #[cfg(not(windows))]
        use super::CommandExt;
        std::process::Command::new(super::tool_path("ffmpeg.exe"))
            .args(["-hide_banner", "-h", "filter=drawtext"])
            .creation_flags(super::CREATE_NO_WINDOW)
            .output()
            .map(|salida| String::from_utf8_lossy(&salida.stdout).contains("y_align"))
            .unwrap_or(false)
    })
}

/// Opacidad de la caja de fondo: entra con la primera pieza y se va con
/// la última, para no quedarse vacía antes o después del texto.
pub fn alfa_caja(animacion: AnimacionTexto, texto: &str, salida: Option<f64>, instante: Option<f64>) -> String {
    let total = colocar(texto, animacion, &|s: &str| s.chars().count() as f64, 1.0, (0.0, 0.0)).len().max(1);
    let entrada = opacidad(animacion, 0, total, None, instante);
    let Some(duracion) = salida else {
        return entrada;
    };
    let fin = duracion - SALIDA;
    match instante {
        Some(t) => format!(
            "{:.4}",
            entrada.parse::<f64>().unwrap_or(1.0) * ((fin + SALIDA - t) / SALIDA).clamp(0.0, 1.0)
        ),
        None => format!("{entrada}*clip(({:.4}-t)/{SALIDA}\\,0\\,1)", fin + SALIDA),
    }
}

/// Filtros de las letras animadas, o `None` si no hay animación.
/// `instante` es el tiempo local del título en el monitor; `None` al exportar.
pub fn filtros(animacion: AnimacionTexto, texto: &str, estilo: &Estilo, instante: Option<f64>) -> Option<String> {
    if animacion == AnimacionTexto::Ninguna || texto.trim().is_empty() {
        return None;
    }
    let fuente = load_font(estilo.fuente)?;
    let medir = |s: &str| text_width(&fuente, estilo.tamano, s);
    let centro = (
        estilo.lienzo.0 as f64 * estilo.posicion.0.clamp(0.0, 1.0),
        estilo.lienzo.1 as f64 * estilo.posicion.1.clamp(0.0, 1.0),
    );
    let piezas = colocar(texto, animacion, &medir, estilo.tamano, centro);
    let total = piezas.len();
    let y_align = if admite_y_align() { ":y_align=font" } else { "" };
    let mut partes = Vec::new();
    for pieza in &piezas {
        let alfa = opacidad(animacion, pieza.orden, total, estilo.salida, instante);
        if instante.is_some() && alfa.parse::<f64>().is_ok_and(|valor| valor <= 0.0) {
            continue;
        }
        partes.push(format!(
            "drawtext=fontfile={}:text={}:expansion=none:fontsize={:.1}:fontcolor=0x{}:x={:.1}:y={}{y_align}:alpha={}{}",
            estilo.fuente_escapada,
            (estilo.escapar)(&pieza.texto),
            estilo.tamano,
            estilo.color,
            pieza.x,
            if instante.is_some() {
                altura(animacion, pieza, total, estilo.tamano, instante)
            } else {
                format!("'{}'", altura(animacion, pieza, total, estilo.tamano, None))
            },
            if instante.is_some() { alfa } else { format!("'{alfa}'") },
            estilo.extra,
        ));
    }
    Some(if partes.is_empty() { "null".to_owned() } else { partes.join(",") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn medir(s: &str) -> f64 {
        s.chars().count() as f64 * 10.0
    }

    #[test]
    fn las_letras_se_colocan_en_orden_y_centradas() {
        let piezas = colocar("Hola tú", AnimacionTexto::AparecerPorLetras, &medir, 20.0, (100.0, 50.0));
        assert_eq!(piezas.len(), 6);
        assert_eq!(piezas[0].x, 100.0 - 35.0);
        assert_eq!(piezas[4].texto, "t");
        assert_eq!(piezas[4].x, 65.0 + 50.0);
        assert_eq!(piezas[5].texto, "ú");
        let palabras = colocar("Hola tú", AnimacionTexto::AparecerPorPalabras, &medir, 20.0, (100.0, 50.0));
        assert_eq!(palabras.iter().map(|p| p.texto.as_str()).collect::<Vec<_>>(), ["Hola", "tú"]);
        let dos = colocar("a\nb", AnimacionTexto::SubirPorLetras, &medir, 20.0, (100.0, 50.0));
        assert_eq!((dos[0].y, dos[1].y), (26.0, 50.0));
    }

    #[test]
    fn la_opacidad_del_monitor_sigue_a_la_de_la_exportacion() {
        let a = |i, t| opacidad(AnimacionTexto::AparecerPorLetras, i, 10, Some(3.0), Some(t)).parse::<f64>().unwrap();
        assert_eq!(a(0, 0.0), 0.0);
        assert_eq!(a(0, 1.0), 1.0);
        assert!(a(9, 0.3) < a(0, 0.3), "las últimas letras llegan después");
        assert_eq!(a(5, 2.99), 0.0, "al final ya se fueron");
        let m = |t| opacidad(AnimacionTexto::MaquinaDeEscribir, 3, 10, None, Some(t)).parse::<f64>().unwrap();
        assert_eq!((m(0.14), m(0.16)), (0.0, 1.0));
        let expresion = opacidad(AnimacionTexto::AparecerPorLetras, 2, 10, Some(3.0), None);
        assert!(expresion.starts_with("clip((t-0.1000)/0.25\\,0\\,1)*clip(("), "{expresion}");
    }
}
