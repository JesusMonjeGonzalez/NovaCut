//! Mate de pista, como «Track Matte Key» de Premiere: un clip se ve solo
//! donde hay imagen en otra pista (por su alfa o por su luminancia). Es lo
//! que hace el vídeo dentro de un texto o los revelados con formas. La
//! pista que hace de mate se compone aparte y deja de verse por sí sola.

use super::RoughClip;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModoMate {
    /// Se ve donde la pista mate es opaca: textos, formas, PNG con alfa.
    #[default]
    Alfa,
    /// Se ve donde la pista mate es clara: vídeo o mates en blanco y negro.
    Luma,
}

impl ModoMate {
    pub const TODOS: [ModoMate; 2] = [ModoMate::Alfa, ModoMate::Luma];

    pub fn nombre(self) -> &'static str {
        match self {
            ModoMate::Alfa => "Alfa",
            ModoMate::Luma => "Luminancia",
        }
    }

    pub fn ayuda(self) -> &'static str {
        match self {
            ModoMate::Alfa => "El clip se ve donde la pista mate tiene imagen (texto, forma, PNG con transparencia)",
            ModoMate::Luma => "El clip se ve donde la pista mate es clara: blanco opaco, negro transparente",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatePista {
    /// Pista de vídeo (0 = V1) cuya imagen recorta este clip.
    pub pista: usize,
    pub modo: ModoMate,
    /// Se ve donde la pista mate NO tiene imagen.
    pub invertir: bool,
}

/// Qué clips hacen de mate (y por tanto no se componen solos) y qué clips
/// usan cada pista mate. Los índices son los de la lista recibida.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// Pista mate → clips que la usan, en orden.
    pub pistas: BTreeMap<usize, Vec<usize>>,
    /// Pista mate → clips que la forman, por orden de inicio.
    pub miembros: BTreeMap<usize, Vec<usize>>,
}

impl Plan {
    pub fn oculto(&self, indice: usize) -> bool {
        self.miembros.values().any(|miembros| miembros.contains(&indice))
    }
}

/// Un clip puede formar parte de un mate si aporta imagen propia.
fn aporta_imagen(clip: &RoughClip) -> bool {
    clip.has_video && !clip.is_adjustment
}

pub fn planificar(clips: &[&RoughClip]) -> Plan {
    let mut plan = Plan::default();
    let pistas_con_imagen: Vec<usize> = clips.iter().filter(|c| aporta_imagen(c)).map(|c| c.track).collect();
    let pedidas: Vec<usize> = clips
        .iter()
        .filter(|clip| aporta_imagen(clip))
        .filter_map(|clip| clip.fx.mate.filter(|mate| mate.pista != clip.track).map(|mate| mate.pista))
        .filter(|pista| pistas_con_imagen.contains(pista))
        .collect();
    for (indice, clip) in clips.iter().enumerate() {
        // Un clip que está en una pista mate no puede usar otra (no se
        // encadenan mates): se ve solo a través de quien lo usa.
        if !aporta_imagen(clip) || pedidas.contains(&clip.track) {
            continue;
        }
        if let Some(mate) = clip.fx.mate.filter(|mate| pedidas.contains(&mate.pista)) {
            plan.pistas.entry(mate.pista).or_default().push(indice);
        }
    }
    for (pista, _) in plan.pistas.iter() {
        let mut miembros: Vec<usize> = clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| clip.track == *pista && aporta_imagen(clip))
            .map(|(indice, _)| indice)
            .collect();
        miembros.sort_by(|a, b| clips[*a].timeline_start.total_cmp(&clips[*b].timeline_start));
        plan.miembros.insert(*pista, miembros);
    }
    plan
}

/// Lienzo transparente del tamaño de la salida.
pub fn lienzo(etiqueta: &str, tamano: (u32, u32), cadencia: &str, duracion: f64) -> String {
    format!(
        "color=c=black@0.0:s={}x{}:r={cadencia}:d={duracion:.6},format=rgba[{etiqueta}]",
        tamano.0, tamano.1
    )
}

/// Del compuesto de la pista mate a una máscara en gris (blanco = se ve).
pub fn mascara(entrada: &str, mate: &MatePista, salida: &str) -> String {
    let extraer = match mate.modo {
        ModoMate::Alfa => "format=rgba,alphaextract",
        // Lo transparente del compuesto es negro: cuenta como «no se ve».
        ModoMate::Luma => "format=rgba,format=gray",
    };
    let invertir = if mate.invertir { ",negate" } else { "" };
    format!("[{entrada}]{extraer}{invertir}[{salida}]")
}

/// Multiplica el alfa de `capa` (ya colocada en un lienzo del tamaño de la
/// salida) por `mascara` y la compone sobre `anterior`.
pub fn aplicar(anterior: &str, capa: &str, mascara: &str, etiqueta: &str, salida: &str) -> Vec<String> {
    vec![
        format!("[{capa}]format=rgba,split=2[{etiqueta}c][{etiqueta}a]"),
        format!("[{etiqueta}a]alphaextract[{etiqueta}al]"),
        format!("[{etiqueta}al][{mascara}]blend=all_mode=multiply:shortest=0:repeatlast=1[{etiqueta}m]"),
        format!("[{etiqueta}c][{etiqueta}m]alphamerge[{etiqueta}r]"),
        format!("[{anterior}][{etiqueta}r]overlay=x=0:y=0:eof_action=pass:shortest=0:format=auto[{salida}]"),
    ]
}

/// Reparte un compuesto entre `n` consumidores.
pub fn repartir(entrada: &str, n: usize, prefijo: &str) -> (String, Vec<String>) {
    let salidas: Vec<String> = (0..n).map(|k| format!("{prefijo}{k}")).collect();
    let filtro = if n == 1 {
        format!("[{entrada}]null[{}]", salidas[0])
    } else {
        let etiquetas: String = salidas.iter().map(|s| format!("[{s}]")).collect();
        format!("[{entrada}]split={n}{etiquetas}")
    };
    (filtro, salidas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(track: usize, start: f64, mate: Option<MatePista>) -> RoughClip {
        let mut clip = RoughClip {
            track,
            timeline_start: start,
            out_seconds: 2.0,
            ..Default::default()
        };
        clip.fx.mate = mate;
        clip
    }

    #[test]
    fn la_pista_mate_se_oculta_y_la_usa_quien_la_pide() {
        let mate = MatePista { pista: 1, modo: ModoMate::Alfa, invertir: false };
        let clips = [clip(0, 0.0, Some(mate)), clip(1, 3.0, None), clip(1, 0.0, None), clip(2, 0.0, None)];
        let plan = planificar(&clips.iter().collect::<Vec<_>>());
        assert_eq!(plan.pistas.get(&1), Some(&vec![0]));
        assert_eq!(plan.miembros.get(&1), Some(&vec![2, 1]), "por orden de inicio");
        assert!(plan.oculto(1) && plan.oculto(2) && !plan.oculto(0) && !plan.oculto(3));
    }

    #[test]
    fn sin_imagen_en_la_pista_pedida_no_hay_mate() {
        let mate = MatePista { pista: 4, modo: ModoMate::Luma, invertir: true };
        let clips = [clip(0, 0.0, Some(mate)), clip(1, 0.0, None)];
        assert_eq!(planificar(&clips.iter().collect::<Vec<_>>()), Plan::default());
        // Su propia pista tampoco vale.
        let propio = MatePista { pista: 0, ..mate };
        let clips = [clip(0, 0.0, Some(propio))];
        assert_eq!(planificar(&clips.iter().collect::<Vec<_>>()), Plan::default());
    }
}
