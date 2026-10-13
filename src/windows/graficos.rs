//! Capas gráficas para redes y podcast: audiograma (la onda de la voz,
//! animada), barra de progreso y formas (rectángulo y marco). Viven en el
//! estilo de un título, así que se mueven, recortan y exportan como
//! cualquier capa de texto; el texto, si lo hay, va encima.

use super::RoughClip;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstiloOnda {
    /// Onda rellena y simétrica: la clásica de los audiogramas.
    #[default]
    Onda,
    /// Una línea fina.
    Linea,
    /// Barras de frecuencias, como un ecualizador.
    Barras,
}

impl EstiloOnda {
    pub const TODOS: [EstiloOnda; 3] = [EstiloOnda::Onda, EstiloOnda::Linea, EstiloOnda::Barras];

    pub fn nombre(self) -> &'static str {
        match self {
            EstiloOnda::Onda => "Onda",
            EstiloOnda::Linea => "Línea",
            EstiloOnda::Barras => "Barras",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TipoForma {
    #[default]
    Rectangulo,
    /// Solo el borde.
    Marco,
}

/// De dónde sale el audio del audiograma: se resuelve al componer con el
/// audio que suena bajo la capa, así que sigue al montaje si se mueve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FuenteAudio {
    pub ruta: PathBuf,
    /// Segundo del archivo que coincide con el inicio de la capa.
    pub desde: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Grafico {
    Audiograma {
        estilo: EstiloOnda,
        color: [f64; 3],
        /// Alto de la onda como fracción del lienzo.
        alto: f64,
        /// Centro vertical como fracción del lienzo.
        centro_y: f64,
        #[serde(default)]
        fuente: Option<FuenteAudio>,
    },
    BarraProgreso {
        color: [f64; 3],
        /// Grosor en píxeles a 1080p.
        grosor: f64,
        arriba: bool,
    },
    Forma {
        tipo: TipoForma,
        color: [f64; 3],
        opacidad: f64,
        /// Caja como fracciones del lienzo: x, y, ancho, alto.
        caja: [f64; 4],
        /// Grosor del marco en píxeles a 1080p.
        grosor: f64,
    },
}

impl Grafico {
    pub fn audiograma() -> Self {
        Grafico::Audiograma {
            estilo: EstiloOnda::Onda,
            color: [1.0, 1.0, 1.0],
            alto: 0.22,
            centro_y: 0.62,
            fuente: None,
        }
    }

    pub fn barra_de_progreso() -> Self {
        Grafico::BarraProgreso { color: [0.0, 0.78, 0.86], grosor: 14.0, arriba: false }
    }

    pub fn rectangulo() -> Self {
        Grafico::Forma {
            tipo: TipoForma::Rectangulo,
            color: [0.08, 0.08, 0.1],
            opacidad: 0.7,
            caja: [0.1, 0.7, 0.8, 0.2],
            grosor: 8.0,
        }
    }

    pub fn marco() -> Self {
        Grafico::Forma {
            tipo: TipoForma::Marco,
            color: [1.0, 1.0, 1.0],
            opacidad: 1.0,
            caja: [0.05, 0.05, 0.9, 0.9],
            grosor: 10.0,
        }
    }

    pub fn nombre(&self) -> &'static str {
        match self {
            Grafico::Audiograma { .. } => "Audiograma",
            Grafico::BarraProgreso { .. } => "Barra de progreso",
            Grafico::Forma { tipo: TipoForma::Rectangulo, .. } => "Rectángulo",
            Grafico::Forma { tipo: TipoForma::Marco, .. } => "Marco",
        }
    }
}

fn hex([r, g, b]: [f64; 3]) -> String {
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02X}{:02X}{:02X}", c(r), c(g), c(b))
}

/// Etiquetas únicas dentro de un grafo: cada capa gráfica abre su propia
/// rama con `;`.
fn etiqueta() -> u64 {
    static SIGUIENTE: AtomicU64 = AtomicU64::new(0);
    SIGUIENTE.fetch_add(1, Ordering::Relaxed)
}

/// Filtros que dibujan el gráfico sobre el lienzo transparente del título.
/// Puede contener ramas propias (`;`) que vuelven a unirse con `overlay`:
/// el resultado sigue siendo una cadena que entra y sale por la del título.
/// `local` es el tiempo del título en el monitor; `None` al exportar.
pub fn filtros(grafico: &Grafico, lienzo: (u32, u32), duracion: f64, local: Option<f64>, fps: f64) -> String {
    let (ancho, alto_lienzo) = (lienzo.0 as f64, lienzo.1 as f64);
    let escala = alto_lienzo / 1080.0;
    let id = etiqueta();
    match grafico {
        Grafico::Forma { tipo, color, opacidad, caja, grosor } => {
            let [x, y, w, h] = caja.map(|v| v.clamp(0.0, 1.0));
            let relleno = match tipo {
                TipoForma::Rectangulo => "fill".to_owned(),
                TipoForma::Marco => format!("{:.0}", (grosor * escala).max(1.0)),
            };
            format!(
                "drawbox=x={:.1}:y={:.1}:w={:.1}:h={:.1}:color=0x{}@{:.3}:t={relleno}:replace=1",
                x * ancho,
                y * alto_lienzo,
                (w * ancho).max(1.0),
                (h * alto_lienzo).max(1.0),
                hex(*color),
                opacidad.clamp(0.0, 1.0)
            )
        }
        Grafico::BarraProgreso { color, grosor, arriba } => {
            // drawbox no evalúa `t` en cada fotograma: la barra es un color
            // que entra deslizándose con `overlay`, que sí lo hace.
            let alto = (grosor * escala).round().max(2.0);
            let y = if *arriba { 0.0 } else { alto_lienzo - alto };
            let duracion = duracion.max(0.04);
            let x = match local {
                Some(t) => format!("{:.1}", -ancho + ancho * (t / duracion).clamp(0.0, 1.0)),
                None => format!("'-w+W*clip(t/{duracion:.4},0,1)'"),
            };
            format!(
                "null[gb{id}];color=c=0x{}:s={}x{}:r={fps:.3},format=rgba[gc{id}];[gb{id}][gc{id}]overlay=x={x}:y={y:.0}:shortest=1",
                hex(*color),
                ancho as u32,
                alto as u32,
            )
        }
        Grafico::Audiograma { estilo, color, alto, centro_y, fuente } => {
            let Some(fuente) = fuente else {
                return "null".to_owned();
            };
            let alto = ((alto.clamp(0.02, 1.0) * alto_lienzo) as u32).max(8) & !1;
            let y = (centro_y.clamp(0.0, 1.0) * alto_lienzo - alto as f64 / 2.0).max(0.0);
            // amovie busca al keyframe anterior del vídeo (en un MP4 de
            // cámara, hasta varios segundos antes) y deja los tiempos del
            // archivo: `atrim` corta exactamente en `desde`.
            let desde = (fuente.desde + local.unwrap_or(0.0)).max(0.0);
            let dibujo = match estilo {
                EstiloOnda::Onda => format!(
                    "showwaves=s={}x{alto}:mode=cline:scale=sqrt:draw=full:rate={fps:.3}:colors=0x{}",
                    ancho as u32,
                    hex(*color)
                ),
                EstiloOnda::Linea => format!(
                    "showwaves=s={}x{alto}:mode=line:scale=sqrt:draw=full:rate={fps:.3}:colors=0x{}",
                    ancho as u32,
                    hex(*color)
                ),
                // showfreqs pinta sobre negro: se recorta con colorkey.
                EstiloOnda::Barras => format!(
                    "showfreqs=s={}x{alto}:mode=bar:ascale=sqrt:fscale=log:win_size=1024:rate={fps:.3}:colors=0x{},format=rgba,colorkey=black:0.01:0",
                    ancho as u32,
                    hex(*color)
                ),
            };
            // showwaves sella su primer fotograma al final de la ventana
            // (pts 1, no 0): sin `setpts` el overlay compone el primero
            // vacío y el monitor, que lee solo uno, no veía la onda.
            format!(
                "null[ga{id}];amovie=filename={}:seek_point={desde:.4},atrim=start={desde:.4},asetpts=PTS-STARTPTS,aformat=channel_layouts=mono,apad,{dibujo},setpts=PTS-STARTPTS,format=rgba[gw{id}];[ga{id}][gw{id}]overlay=x=0:y={y:.0}:shortest=1",
                super::escape_filter_value(&fuente.ruta.to_string_lossy()),
            )
        }
    }
}

/// Pone a cada audiograma el audio que suena bajo él: el clip audible que
/// más tiempo comparte con la capa. Sin audio debajo, el audiograma queda
/// vacío en vez de romper el render.
pub fn resolver_fuentes(clips: &mut [RoughClip]) {
    let audibles: Vec<(PathBuf, f64, f64, f64, f64)> = clips
        .iter()
        .filter(|clip| {
            clip.has_audio && !clip.muted && clip.enabled && clip.title.is_none() && !clip.path.as_os_str().is_empty()
        })
        .map(|clip| {
            (
                clip.path.clone(),
                clip.timeline_start,
                clip.timeline_start + clip.duration(),
                clip.in_seconds,
                clip.speed.clamp(0.1, 8.0),
            )
        })
        .collect();
    for clip in clips.iter_mut() {
        let (inicio, fin) = (clip.timeline_start, clip.timeline_start + clip.duration());
        let Some(Grafico::Audiograma { fuente, .. }) = clip.title.as_mut().and_then(|t| t.style.grafico.as_mut()) else {
            continue;
        };
        *fuente = audibles
            .iter()
            .map(|(ruta, a, b, entrada, velocidad)| (ruta, a, entrada, velocidad, b.min(fin) - a.max(inicio)))
            .filter(|(.., solape)| *solape > 0.01)
            .max_by(|x, y| x.4.total_cmp(&y.4))
            .map(|(ruta, a, entrada, velocidad, _)| FuenteAudio {
                ruta: ruta.clone(),
                // Si el audio empezó antes que la capa, se salta lo previo;
                // si empieza después, la onda arranca en su principio.
                desde: entrada + (inicio - a).max(0.0) * velocidad,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_barra_avanza_con_el_tiempo_y_llena_al_final() {
        let barra = Grafico::barra_de_progreso();
        let mitad = filtros(&barra, (1920, 1080), 10.0, Some(5.0), 25.0);
        assert!(mitad.contains("overlay=x=-960.0:y=1066"), "{mitad}");
        let final_ = filtros(&barra, (1920, 1080), 10.0, Some(10.0), 25.0);
        assert!(final_.contains("overlay=x=0.0"), "{final_}");
        let exportacion = filtros(&barra, (1920, 1080), 10.0, None, 25.0);
        assert!(exportacion.contains("x='-w+W*clip(t/10.0000,0,1)'"), "{exportacion}");
    }

    #[test]
    fn el_audiograma_toma_el_audio_que_suena_debajo() {
        let voz = RoughClip {
            path: PathBuf::from("voz.wav"),
            timeline_start: 2.0,
            in_seconds: 10.0,
            out_seconds: 30.0,
            has_audio: true,
            has_video: false,
            ..Default::default()
        };
        let capa = RoughClip {
            timeline_start: 5.0,
            out_seconds: 4.0,
            has_audio: false,
            title: Some(super::super::Titulo {
                style: super::super::efectos::TitleStyle {
                    grafico: Some(Grafico::audiograma()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut clips = vec![voz, capa];
        resolver_fuentes(&mut clips);
        let Some(Grafico::Audiograma { fuente: Some(fuente), .. }) =
            clips[1].title.as_ref().and_then(|t| t.style.grafico.clone())
        else {
            panic!("sin fuente");
        };
        assert_eq!((fuente.ruta, fuente.desde), (PathBuf::from("voz.wav"), 13.0));
        // Sin audio debajo, el audiograma no dibuja nada.
        let mut sola = vec![clips[1].clone()];
        resolver_fuentes(&mut sola);
        let grafico = sola[0].title.as_ref().and_then(|t| t.style.grafico.clone()).unwrap();
        assert_eq!(filtros(&grafico, (640, 360), 4.0, None, 25.0), "null");
    }
}
