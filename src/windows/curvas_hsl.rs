//! Curvas HSL dibujables, como las de Lumetri: tono frente a saturación,
//! tono frente a tono, tono frente a luminancia y luminancia frente a
//! saturación. Cada curva son puntos `(x, y)` con `y` = 0 sin cambio.
//!
//! FFmpeg no tiene un filtro así: las cuatro curvas se convierten en una LUT
//! 3D (.cube) que aplica `lut3d`. El monitor y la exportación usan el mismo
//! archivo, así que se ven igual. La LUT se nombra por su contenido y se
//! reutiliza; la caché se poda para no crecer sin límite.

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Un punto de curva: `x` de 0 a 1 (tono o luminancia), `y` de −1 a 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Punto {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CurvasHsl {
    /// Saturación según el tono: +1 dobla la saturación, −1 la quita.
    pub tono_saturacion: Vec<Punto>,
    /// Giro de tono según el tono: ±1 son ±90°.
    pub tono_tono: Vec<Punto>,
    /// Luminosidad según el tono: ±1 sube o baja medio rango los colores
    /// saturados (los grises no se tocan).
    pub tono_luminancia: Vec<Punto>,
    /// Saturación según la luminancia: sombras o luces más o menos vivas.
    pub luminancia_saturacion: Vec<Punto>,
}

/// Qué curva se está editando.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curva {
    TonoSaturacion,
    TonoTono,
    TonoLuminancia,
    LuminanciaSaturacion,
}

impl Curva {
    pub const TODAS: [Curva; 4] = [
        Curva::TonoSaturacion,
        Curva::TonoTono,
        Curva::TonoLuminancia,
        Curva::LuminanciaSaturacion,
    ];

    pub fn nombre(self) -> &'static str {
        match self {
            Curva::TonoSaturacion => "Tono / Sat.",
            Curva::TonoTono => "Tono / Tono",
            Curva::TonoLuminancia => "Tono / Lum.",
            Curva::LuminanciaSaturacion => "Lum. / Sat.",
        }
    }

    pub fn ayuda(self) -> &'static str {
        match self {
            Curva::TonoSaturacion => "Sube o baja la saturación de un color concreto: apagar un cielo, avivar una camiseta",
            Curva::TonoTono => "Gira un color hacia otro: verdes más amarillos, rojos más naranjas",
            Curva::TonoLuminancia => "Aclara u oscurece un color concreto: cielos más densos, piel más luminosa",
            Curva::LuminanciaSaturacion => "Saturación según el brillo: quitar color a las sombras o a las luces quemadas",
        }
    }

    /// Las curvas de tono dan la vuelta: el rojo de la izquierda es el de la derecha.
    fn ciclica(self) -> bool {
        self != Curva::LuminanciaSaturacion
    }
}

impl CurvasHsl {
    pub fn puntos(&self, curva: Curva) -> &Vec<Punto> {
        match curva {
            Curva::TonoSaturacion => &self.tono_saturacion,
            Curva::TonoTono => &self.tono_tono,
            Curva::TonoLuminancia => &self.tono_luminancia,
            Curva::LuminanciaSaturacion => &self.luminancia_saturacion,
        }
    }

    fn puntos_mut(&mut self, curva: Curva) -> &mut Vec<Punto> {
        match curva {
            Curva::TonoSaturacion => &mut self.tono_saturacion,
            Curva::TonoTono => &mut self.tono_tono,
            Curva::TonoLuminancia => &mut self.tono_luminancia,
            Curva::LuminanciaSaturacion => &mut self.luminancia_saturacion,
        }
    }

    /// Sin ningún punto que se aparte de cero no hay nada que aplicar.
    pub fn es_neutra(&self) -> bool {
        Curva::TODAS
            .iter()
            .all(|curva| self.puntos(*curva).iter().all(|punto| punto.y.abs() < 1e-4))
    }

    /// Color transformado; entra y sale RGB de 0 a 1.
    pub fn aplicar(&self, rgb: [f64; 3]) -> [f64; 3] {
        let (h, s, l) = rgb_a_hsl(rgb);
        let sat_tono = 1.0 + valor(&self.tono_saturacion, h, true);
        let giro = valor(&self.tono_tono, h, true) * 0.25;
        let luz = valor(&self.tono_luminancia, h, true) * 0.5 * s;
        let sat_luz = 1.0 + valor(&self.luminancia_saturacion, l, false);
        let h = (h + giro).rem_euclid(1.0);
        let s = (s * sat_tono * sat_luz).clamp(0.0, 1.0);
        let l = (l + luz).clamp(0.0, 1.0);
        hsl_a_rgb(h, s, l)
    }

    /// Texto .cube de lado `n` (rojo varía más rápido, como pide el formato).
    pub fn cube(&self, n: usize) -> String {
        let mut texto = format!("TITLE \"NovaCut curvas HSL\"\nLUT_3D_SIZE {n}\n");
        texto.reserve(n * n * n * 27);
        let paso = 1.0 / (n - 1) as f64;
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let [ro, go, bo] = self.aplicar([r as f64 * paso, g as f64 * paso, b as f64 * paso]);
                    texto.push_str(&format!("{ro:.6} {go:.6} {bo:.6}\n"));
                }
            }
        }
        texto
    }

    /// Archivo .cube de estas curvas, escrito si aún no existe.
    pub fn archivo_lut(&self) -> Option<PathBuf> {
        if self.es_neutra() {
            return None;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(self).ok()?.hash(&mut hasher);
        LADO.hash(&mut hasher);
        let carpeta = carpeta_cache();
        let ruta = carpeta.join(format!("hsl-{:016x}.cube", hasher.finish()));
        if !ruta.is_file() {
            std::fs::create_dir_all(&carpeta).ok()?;
            podar(&carpeta);
            // Se escribe aparte y se renombra: FFmpeg nunca lee uno a medias.
            let parcial = ruta.with_extension(format!("{}.part", std::process::id()));
            std::fs::write(&parcial, self.cube(LADO)).ok()?;
            std::fs::rename(&parcial, &ruta).ok()?;
        }
        Some(ruta)
    }

    /// Filtro para la cadena de color; vacío si las curvas son neutras.
    pub fn filtro(&self) -> String {
        self.archivo_lut()
            .map(|ruta| format!(",lut3d=file={}:interp=tetrahedral", super::escape_lut_path(&ruta)))
            .unwrap_or_default()
    }
}

/// 33 es lo habitual en LUT de etalonaje: suave en giros de tono y ligera.
const LADO: usize = 33;
/// Curvas distintas que se guardan; las más viejas se borran.
const MAXIMO_EN_CACHE: usize = 64;

fn carpeta_cache() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|local| PathBuf::from(local).join("NovaCut").join("curvas"))
        .unwrap_or_else(|| std::env::temp_dir().join("novacut-curvas"))
}

/// Al arrastrar un punto se generan muchas LUT seguidas: se quedan las
/// `MAXIMO_EN_CACHE` más recientes.
fn podar(carpeta: &std::path::Path) {
    let Ok(entradas) = std::fs::read_dir(carpeta) else {
        return;
    };
    let mut archivos: Vec<(std::time::SystemTime, PathBuf)> = entradas
        .filter_map(Result::ok)
        .filter(|entrada| entrada.path().extension().is_some_and(|ext| ext == "cube"))
        .filter_map(|entrada| Some((entrada.metadata().ok()?.modified().ok()?, entrada.path())))
        .collect();
    if archivos.len() < MAXIMO_EN_CACHE {
        return;
    }
    archivos.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, ruta) in archivos.iter().take(archivos.len() + 1 - MAXIMO_EN_CACHE) {
        let _ = std::fs::remove_file(ruta);
    }
}

/// Valor de la curva en `x`: entre puntos, curva suave (smoothstep); en las
/// cíclicas el último punto enlaza con el primero dando la vuelta.
pub fn valor(puntos: &[Punto], x: f64, ciclica: bool) -> f64 {
    let mut ordenados: Vec<Punto> = puntos
        .iter()
        .copied()
        .filter(|punto| punto.x.is_finite() && punto.y.is_finite())
        .collect();
    if ordenados.is_empty() {
        return 0.0;
    }
    ordenados.sort_by(|a, b| a.x.total_cmp(&b.x));
    let suave = |a: Punto, b: Punto, x: f64| {
        let tramo = (b.x - a.x).max(1e-9);
        let f = ((x - a.x) / tramo).clamp(0.0, 1.0);
        a.y + (b.y - a.y) * f * f * (3.0 - 2.0 * f)
    };
    let primero = ordenados[0];
    let ultimo = ordenados[ordenados.len() - 1];
    if ciclica {
        let x = x.rem_euclid(1.0);
        if x < primero.x {
            return suave(Punto { x: ultimo.x - 1.0, y: ultimo.y }, primero, x);
        }
        if x >= ultimo.x {
            return suave(ultimo, Punto { x: primero.x + 1.0, y: primero.y }, x);
        }
    } else {
        if x <= primero.x {
            return primero.y;
        }
        if x >= ultimo.x {
            return ultimo.y;
        }
    }
    for par in ordenados.windows(2) {
        if x >= par[0].x && x <= par[1].x {
            return suave(par[0], par[1], x);
        }
    }
    ultimo.y
}

fn rgb_a_hsl([r, g, b]: [f64; 3]) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-9 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-9);
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s.clamp(0.0, 1.0), l)
}

fn hsl_a_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(1.0) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [(r + m).clamp(0.0, 1.0), (g + m).clamp(0.0, 1.0), (b + m).clamp(0.0, 1.0)]
}

/// Añade un punto y devuelve su índice. El primero de una curva vacía llega
/// con dos vecinos neutros: un punto solo movería la curva entera y
/// cambiaría todos los colores, no el que se señaló.
pub fn anadir(puntos: &mut Vec<Punto>, nuevo: Punto, ciclica: bool) -> usize {
    if puntos.is_empty() {
        for x in [nuevo.x - 0.12, nuevo.x + 0.12] {
            let x = if ciclica { x.rem_euclid(1.0) } else { x.clamp(0.0, 1.0) };
            if (x - nuevo.x).abs() > 0.01 {
                puntos.push(Punto { x, y: 0.0 });
            }
        }
    }
    puntos.push(nuevo);
    puntos.len() - 1
}

/// Editor de una curva. Clic en vacío añade un punto, arrastrar lo mueve,
/// clic derecho lo quita. Devuelve si algo cambió.
pub fn editor(ui: &mut egui::Ui, curvas: &mut CurvasHsl, curva: &mut Curva) -> bool {
    let mut cambiado = false;
    ui.horizontal_wrapped(|ui| {
        for opcion in Curva::TODAS {
            let activa = *curva == opcion;
            let marcada = !curvas.puntos(opcion).iter().all(|p| p.y.abs() < 1e-4);
            let texto = if marcada { format!("{} •", opcion.nombre()) } else { opcion.nombre().to_owned() };
            if ui.selectable_label(activa, texto).on_hover_text(opcion.ayuda()).clicked() {
                *curva = opcion;
            }
        }
    });
    let ancho = (ui.available_width() - 10.0).clamp(160.0, 420.0);
    let (rect, respuesta) = ui.allocate_exact_size(egui::vec2(ancho, 110.0), egui::Sense::click_and_drag());
    let pintor = ui.painter_at(rect);
    // Fondo: el degradado de tono (o de negro a blanco) que se edita.
    // Malla con color por vértice: sin costuras entre franjas.
    let franjas = 48;
    let mut malla = egui::Mesh::default();
    for i in 0..=franjas {
        let t = i as f64 / franjas as f64;
        let x = rect.left() + rect.width() * t as f32;
        let [r, g, b] = if curva.ciclica() { hsl_a_rgb(t, 0.75, 0.42) } else { [t, t, t] };
        let color = egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
            .gamma_multiply(0.55);
        malla.colored_vertex(egui::pos2(x, rect.top()), color);
        malla.colored_vertex(egui::pos2(x, rect.bottom()), color);
        if i > 0 {
            let base = (i as u32 - 1) * 2;
            malla.add_triangle(base, base + 1, base + 2);
            malla.add_triangle(base + 1, base + 3, base + 2);
        }
    }
    pintor.add(egui::Shape::mesh(malla));
    pintor.line_segment(
        [egui::pos2(rect.left(), rect.center().y), egui::pos2(rect.right(), rect.center().y)],
        egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(70)),
    );
    let a_pantalla = |p: Punto| {
        egui::pos2(
            rect.left() + rect.width() * p.x as f32,
            rect.center().y - rect.height() * 0.45 * p.y as f32,
        )
    };
    let desde_pantalla = |pos: egui::Pos2| Punto {
        x: (((pos.x - rect.left()) / rect.width()) as f64).clamp(0.0, 1.0),
        y: (((rect.center().y - pos.y) / (rect.height() * 0.45)) as f64).clamp(-1.0, 1.0),
    };
    let ciclica = curva.ciclica();
    let linea: Vec<egui::Pos2> = (0..=96)
        .map(|i| {
            let x = i as f64 / 96.0;
            a_pantalla(Punto { x, y: valor(curvas.puntos(*curva), x, ciclica) })
        })
        .collect();
    pintor.add(egui::Shape::line(linea, egui::Stroke::new(2.0_f32, egui::Color32::WHITE)));
    // Interacción: el punto más cercano a menos de 10 px se arrastra.
    let id_arrastre = respuesta.id.with("punto");
    let cercano = |puntos: &[Punto], pos: egui::Pos2| {
        puntos
            .iter()
            .enumerate()
            .map(|(i, p)| (i, a_pantalla(*p).distance(pos)))
            .filter(|(_, d)| *d < 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    if respuesta.drag_started() {
        if let Some(pos) = respuesta.interact_pointer_pos() {
            let puntos = curvas.puntos_mut(*curva);
            let indice = match cercano(puntos, pos) {
                Some(i) => i,
                None => {
                    cambiado = true;
                    anadir(puntos, desde_pantalla(pos), ciclica)
                }
            };
            ui.ctx().data_mut(|d| d.insert_temp(id_arrastre, indice));
        }
    }
    if respuesta.dragged() {
        if let (Some(pos), Some(indice)) = (
            respuesta.interact_pointer_pos(),
            ui.ctx().data(|d| d.get_temp::<usize>(id_arrastre)),
        ) {
            if let Some(punto) = curvas.puntos_mut(*curva).get_mut(indice) {
                *punto = desde_pantalla(pos);
                cambiado = true;
            }
        }
    }
    if respuesta.clicked() {
        if let Some(pos) = respuesta.interact_pointer_pos() {
            let puntos = curvas.puntos_mut(*curva);
            if cercano(puntos, pos).is_none() {
                anadir(puntos, desde_pantalla(pos), ciclica);
                cambiado = true;
            }
        }
    }
    if respuesta.secondary_clicked() {
        if let Some(pos) = respuesta.interact_pointer_pos() {
            let puntos = curvas.puntos_mut(*curva);
            if let Some(i) = cercano(puntos, pos) {
                puntos.remove(i);
                cambiado = true;
            }
        }
    }
    for punto in curvas.puntos(*curva) {
        pintor.circle(a_pantalla(*punto), 4.5, egui::Color32::WHITE, egui::Stroke::new(1.5_f32, egui::Color32::BLACK));
    }
    respuesta.on_hover_text("Clic o arrastre: añade y mueve puntos · clic derecho en un punto: lo quita");
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!curvas.puntos(*curva).is_empty(), egui::Button::new("Restablecer esta curva"))
            .clicked()
        {
            curvas.puntos_mut(*curva).clear();
            cambiado = true;
        }
    });
    cambiado
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sat([r, g, b]: [f64; 3]) -> f64 {
        rgb_a_hsl([r, g, b]).1
    }

    #[test]
    fn sin_puntos_es_la_identidad() {
        let curvas = CurvasHsl::default();
        assert!(curvas.es_neutra());
        for color in [[0.2, 0.4, 0.9], [1.0, 0.0, 0.0], [0.5, 0.5, 0.5], [0.0, 0.0, 0.0]] {
            let salida = curvas.aplicar(color);
            for (a, b) in color.iter().zip(salida) {
                assert!((a - b).abs() < 1e-9, "{color:?} → {salida:?}");
            }
        }
        assert!(curvas.archivo_lut().is_none());
    }

    #[test]
    fn bajar_los_azules_no_toca_los_rojos_ni_los_grises() {
        let curvas = CurvasHsl {
            tono_saturacion: vec![Punto { x: 0.0, y: 0.0 }, Punto { x: 0.667, y: -1.0 }, Punto { x: 0.9, y: 0.0 }],
            ..Default::default()
        };
        let azul = [0.1, 0.2, 0.9];
        let rojo = [0.9, 0.1, 0.1];
        let gris = [0.5, 0.5, 0.5];
        assert!(sat(curvas.aplicar(azul)) < 0.05);
        assert!((sat(curvas.aplicar(rojo)) - sat(rojo)).abs() < 0.02);
        assert_eq!(curvas.aplicar(gris), gris);
    }

    #[test]
    fn el_primer_punto_solo_cambia_su_zona() {
        let mut puntos = Vec::new();
        anadir(&mut puntos, Punto { x: 0.667, y: -1.0 }, true);
        assert_eq!(puntos.len(), 3);
        assert_eq!(valor(&puntos, 0.667, true), -1.0);
        assert_eq!(valor(&puntos, 0.0, true), 0.0);
        assert_eq!(valor(&puntos, 0.3, true), 0.0);
        let mut luz = Vec::new();
        anadir(&mut luz, Punto { x: 0.0, y: 0.5 }, false);
        assert_eq!(luz.len(), 2);
        assert_eq!(valor(&luz, 0.5, false), 0.0);
    }

    #[test]
    fn las_curvas_de_tono_dan_la_vuelta() {
        let puntos = vec![Punto { x: 0.1, y: 0.0 }, Punto { x: 0.9, y: 1.0 }];
        // Entre 0,9 y 1,1 (=0,1) se vuelve suave al valor del primero.
        assert!((valor(&puntos, 0.0, true) - 0.5).abs() < 1e-9);
        assert_eq!(valor(&puntos, 0.0, false), 0.0);
        assert_eq!(valor(&puntos, 0.95, false), 1.0);
    }

    #[test]
    fn la_lut_tiene_el_tamano_y_el_orden_del_formato_cube() {
        let curvas = CurvasHsl {
            luminancia_saturacion: vec![Punto { x: 0.0, y: -1.0 }, Punto { x: 0.2, y: 0.0 }],
            ..Default::default()
        };
        let cube = curvas.cube(3);
        let filas: Vec<&str> = cube.lines().filter(|l| l.starts_with(|c: char| c.is_ascii_digit())).collect();
        assert_eq!(filas.len(), 27);
        assert_eq!(filas[0], "0.000000 0.000000 0.000000");
        assert_eq!(filas[1], "0.500000 0.000000 0.000000");
        assert_eq!(filas[26], "1.000000 1.000000 1.000000");
    }
}
