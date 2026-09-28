//! Keyframes de cualquier parámetro de color, efecto o transformación, y su
//! traducción a FFmpeg para que la animación sea continua fotograma a
//! fotograma (no por tramos constantes).
//!
//! Dos mecanismos, según lo que admita cada filtro:
//! - expresiones en `t` evaluadas por fotograma (`eq`, `vignette`, `rotate`,
//!   `scale`, `overlay`);
//! - órdenes `sendcmd` con el valor interpolado en cada fotograma de las
//!   rampas (`colortemperature`, `colorchannelmixer`, `vibrance`, `gblur`).
//!
//! Los tiempos de los keyframes son locales al clip, en segundos de
//! timeline, igual que la banda de volumen y los de transformación.

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Param {
    Exposure,
    Contrast,
    Saturation,
    Vignette,
    Blur,
    Temperature,
    Tint,
    Vibrance,
    Rotation,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub t: f64,
    pub v: f64,
}

/// Pistas de animación de un clip: solo existen las de parámetros animados.
pub type Tracks = BTreeMap<Param, Vec<Key>>;

/// Dos keyframes a menos de esto se consideran el mismo (medio fotograma a
/// 25 fps).
pub const SAME_KEY: f64 = 0.02;

/// Valor en `t`: interpolación lineal; fuera de rango, el keyframe más
/// cercano. `None` si no hay keyframes.
pub fn value_at(keys: &[Key], t: f64) -> Option<f64> {
    let first = keys.first()?;
    if t <= first.t {
        return Some(first.v);
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.t {
            let span = (b.t - a.t).max(1e-9);
            return Some(a.v + (b.v - a.v) * (t - a.t) / span);
        }
    }
    keys.last().map(|key| key.v)
}

/// Añade o sustituye el keyframe en `t`, manteniendo el orden.
pub fn set_key(keys: &mut Vec<Key>, t: f64, v: f64) {
    match keys.iter_mut().find(|key| (key.t - t).abs() < SAME_KEY) {
        Some(existing) => existing.v = v,
        None => {
            keys.push(Key { t, v });
            keys.sort_by(|left, right| left.t.total_cmp(&right.t));
        }
    }
}

pub fn key_index_near(keys: &[Key], t: f64) -> Option<usize> {
    keys.iter().position(|key| (key.t - t).abs() < SAME_KEY)
}

/// ¿Cambia el valor a lo largo del clip? Con un solo keyframe (o todos
/// iguales) el parámetro es constante y basta el filtro estático.
pub fn varies(keys: &[Key]) -> bool {
    keys.windows(2).any(|pair| (pair[0].v - pair[1].v).abs() > 1e-9)
}

/// Desplaza todos los tiempos `delta` segundos (cuando cambia el inicio del
/// clip sin mover su contenido).
pub fn shift(tracks: &mut Tracks, delta: f64) {
    for keys in tracks.values_mut() {
        for key in keys.iter_mut() {
            key.t += delta;
        }
    }
}

/// Keyframes con el valor ya convertido a la unidad del filtro. Todas las
/// conversiones son lineales (con límites), así que convertir los extremos
/// e interpolar da lo mismo que interpolar y convertir.
pub fn mapped(keys: &[Key], map: impl Fn(f64) -> f64) -> Vec<Key> {
    keys.iter()
        .map(|key| Key {
            t: key.t,
            v: map(key.v),
        })
        .collect()
}

/// Expresión de FFmpeg con la curva lineal a tramos sobre `time` (por
/// ejemplo `t` o `(t-3.5)`). Antes del primero y tras el último keyframe se
/// mantiene su valor.
pub fn expression(keys: &[Key], time: &str) -> String {
    let Some(last) = keys.last() else {
        return "0".to_owned();
    };
    let mut expression = format!("{:.6}", last.v);
    for pair in keys.windows(2).rev() {
        let (a, b) = (pair[0], pair[1]);
        let span = (b.t - a.t).max(1e-6);
        expression = format!(
            "if(lt({time},{bt:.6}),{av:.6}+({dv:.6})*clip(({time}-{at:.6})/{span:.6},0,1),{expression})",
            bt = b.t,
            av = a.v,
            dv = b.v - a.v,
            at = a.t,
        );
    }
    expression
}

/// Órdenes `sendcmd` para `target` (instancia con nombre, `filtro@id`):
/// una por fotograma dentro de cada rampa, más una en cada keyframe. `shift`
/// suma a los tiempos (0 en cadenas locales al clip). Solo se emite dentro
/// de `[0, duration]`.
pub fn commands(
    target: &str,
    options: &[(&str, &dyn Fn(f64) -> f64)],
    keys: &[Key],
    frame: f64,
    duration: f64,
    shift: f64,
) -> Vec<String> {
    let mut times: Vec<f64> = Vec::new();
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if (a.v - b.v).abs() < 1e-9 {
            times.push(b.t);
            continue;
        }
        let mut t = a.t.max(0.0);
        while t < b.t.min(duration) {
            times.push(t);
            t += frame;
        }
        times.push(b.t);
    }
    times.retain(|t| *t >= 0.0 && *t <= duration + 1e-6);
    times.sort_by(f64::total_cmp);
    times.dedup_by(|later, earlier| (*later - *earlier).abs() < 1e-6);
    let mut out = Vec::new();
    for t in times {
        let Some(value) = value_at(keys, t) else {
            continue;
        };
        for (option, map) in options {
            out.push(format!("{:.4} {target} {option} {:.6}", t + shift, map(value)));
        }
    }
    out
}

/// Filtro `sendcmd` con todas las órdenes, o vacío si no hay ninguna.
pub fn sendcmd(commands: &[String]) -> String {
    if commands.is_empty() {
        return String::new();
    }
    format!(",sendcmd=c='{}'", commands.join(";"))
}

/// Pistas de posición, escala y opacidad a partir de los keyframes de
/// transformación clásicos del clip.
pub fn transform_tracks(keyframes: &[super::TransformKeyframe]) -> [Vec<Key>; 4] {
    let mut sorted = keyframes.to_vec();
    sorted.sort_by(|left, right| left.t.total_cmp(&right.t));
    let track = |pick: &dyn Fn(&super::TransformKeyframe) -> f64| {
        sorted
            .iter()
            .map(|keyframe| Key {
                t: keyframe.t,
                v: pick(keyframe),
            })
            .collect::<Vec<_>>()
    };
    [
        track(&|keyframe| keyframe.x),
        track(&|keyframe| keyframe.y),
        track(&|keyframe| keyframe.scale),
        track(&|keyframe| keyframe.opacity),
    ]
}

/// Qué pidió el usuario con los controles de keyframe de un parámetro.
#[derive(Clone, Copy, PartialEq)]
pub enum KeyAction {
    None,
    /// Llevar el cabezal a este tiempo local del clip.
    Seek(f64),
    Changed,
}

/// Deslizador animable, como los de Premiere: ◇ activa la animación con un
/// keyframe en el cabezal; con animación, ◆ añade o quita el keyframe del
/// cabezal, ◀ ▶ saltan entre keyframes, y mover el deslizador crea o
/// actualiza el keyframe del cabezal. Clic derecho en ◆ quita la animación.
#[allow(clippy::too_many_arguments)]
pub fn animated_slider(
    ui: &mut egui::Ui,
    tracks: &mut Tracks,
    param: Param,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    text: &str,
    local_t: f64,
    reset_to: f64,
) -> KeyAction {
    animated_value(ui, tracks, param, value, local_t, |ui, value| {
        let slider = ui.add(egui::Slider::new(value, range).text(text));
        let reset = ui.small_button("↺").on_hover_text("Restablecer").clicked();
        if reset {
            *value = reset_to;
        }
        slider.changed() || reset
    })
}

/// Cualquier control numérico con los botones de keyframe al lado. `widget`
/// dibuja el control y devuelve si el usuario cambió el valor.
pub fn animated_value(
    ui: &mut egui::Ui,
    tracks: &mut Tracks,
    param: Param,
    value: &mut f64,
    local_t: f64,
    widget: impl FnOnce(&mut egui::Ui, &mut f64) -> bool,
) -> KeyAction {
    let mut action = KeyAction::None;
    ui.horizontal(|ui| {
        let animated = tracks.get(&param).is_some_and(|keys| !keys.is_empty());
        if animated {
            // El control muestra el valor animado en el cabezal.
            if let Some(current) = tracks.get(&param).and_then(|keys| value_at(keys, local_t)) {
                *value = current;
            }
        }
        if widget(ui, value) {
            if animated {
                if let Some(keys) = tracks.get_mut(&param) {
                    set_key(keys, local_t, *value);
                }
            }
            action = KeyAction::Changed;
        }
        if !animated {
            let start = ui
                .small_button("◇")
                .on_hover_text("Animar este parámetro: crea un keyframe en el cabezal");
            if start.clicked() {
                tracks.insert(param, vec![Key { t: local_t, v: *value }]);
                action = KeyAction::Changed;
            }
            return;
        }
        let keys = tracks.get_mut(&param).expect("animado");
        let times: Vec<f64> = keys.iter().map(|key| key.t).collect();
        match key_buttons(ui, &times, local_t) {
            KeyButton::Seek(t) => action = KeyAction::Seek(t),
            KeyButton::Toggle => {
                match key_index_near(keys, local_t) {
                    Some(index) => {
                        keys.remove(index);
                    }
                    None => set_key(keys, local_t, *value),
                }
                action = KeyAction::Changed;
            }
            KeyButton::Clear => {
                keys.clear();
            }
            KeyButton::None => {}
        }
        if keys.is_empty() {
            let keep = *value;
            tracks.remove(&param);
            *value = keep;
            action = KeyAction::Changed;
        }
    });
    action
}

/// Resultado de los botones ◀ ◆ ▶.
#[derive(Clone, Copy, PartialEq)]
pub enum KeyButton {
    None,
    Seek(f64),
    /// Añadir o quitar el keyframe del cabezal.
    Toggle,
    /// Quitar toda la animación (clic derecho en ◆).
    Clear,
}

/// ◀ ◆ ▶ para una lista de tiempos de keyframe.
pub fn key_buttons(ui: &mut egui::Ui, times: &[f64], local_t: f64) -> KeyButton {
    let mut result = KeyButton::None;
    let here = times.iter().any(|t| (t - local_t).abs() < SAME_KEY);
    let previous = times
        .iter()
        .copied()
        .filter(|t| *t < local_t - SAME_KEY)
        .fold(None, |best: Option<f64>, t| Some(best.map_or(t, |b| b.max(t))));
    let next = times
        .iter()
        .copied()
        .filter(|t| *t > local_t + SAME_KEY)
        .fold(None, |best: Option<f64>, t| Some(best.map_or(t, |b| b.min(t))));
    if ui
        .add_enabled(previous.is_some(), egui::Button::new("◀").small())
        .on_hover_text("Keyframe anterior")
        .clicked()
    {
        result = KeyButton::Seek(previous.unwrap_or(local_t));
    }
    let diamond = ui
        .add(
            egui::Button::new(
                egui::RichText::new(if here { "◆" } else { "◇" })
                    .color(egui::Color32::from_rgb(90, 190, 255)),
            )
            .small(),
        )
        .on_hover_text(
            "Clic: añadir o quitar el keyframe del cabezal · Clic derecho: quitar la animación",
        );
    if diamond.clicked() {
        result = KeyButton::Toggle;
    }
    if diamond.secondary_clicked() {
        result = KeyButton::Clear;
    }
    if ui
        .add_enabled(next.is_some(), egui::Button::new("▶").small())
        .on_hover_text("Keyframe siguiente")
        .clicked()
    {
        result = KeyButton::Seek(next.unwrap_or(local_t));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(points: &[(f64, f64)]) -> Vec<Key> {
        points.iter().map(|&(t, v)| Key { t, v }).collect()
    }

    /// Evalúa la expresión generada con un intérprete mínimo de las tres
    /// funciones que usa (if, lt, clip), para comprobar que coincide con
    /// `value_at` sin lanzar FFmpeg.
    fn eval(expression: &str, t: f64) -> f64 {
        fn parse(input: &mut &str, t: f64) -> f64 {
            let text = input.trim_start();
            *input = text;
            let mut value = term(input, t);
            loop {
                let text = input.trim_start();
                if let Some(rest) = text.strip_prefix('+') {
                    *input = rest;
                    value += term(input, t);
                } else if let Some(rest) = text.strip_prefix('-') {
                    *input = rest;
                    value -= term(input, t);
                } else {
                    *input = text;
                    return value;
                }
            }
        }
        fn term(input: &mut &str, t: f64) -> f64 {
            let mut value = factor(input, t);
            loop {
                if let Some(rest) = input.strip_prefix('*') {
                    *input = rest;
                    value *= factor(input, t);
                } else if let Some(rest) = input.strip_prefix('/') {
                    *input = rest;
                    value /= factor(input, t);
                } else {
                    return value;
                }
            }
        }
        fn args(input: &mut &str, t: f64, count: usize) -> Vec<f64> {
            let mut values = Vec::new();
            for index in 0..count {
                values.push(parse(input, t));
                let separator = if index + 1 == count { ')' } else { ',' };
                *input = input.strip_prefix(separator).expect("separador");
            }
            values
        }
        fn factor(input: &mut &str, t: f64) -> f64 {
            if let Some(rest) = input.strip_prefix("if(") {
                *input = rest;
                let values = args(input, t, 3);
                return if values[0] != 0.0 { values[1] } else { values[2] };
            }
            if let Some(rest) = input.strip_prefix("lt(") {
                *input = rest;
                let values = args(input, t, 2);
                return (values[0] < values[1]) as i32 as f64;
            }
            if let Some(rest) = input.strip_prefix("clip(") {
                *input = rest;
                let values = args(input, t, 3);
                return values[0].clamp(values[1], values[2]);
            }
            if let Some(rest) = input.strip_prefix('(') {
                *input = rest;
                let value = parse(input, t);
                *input = input.strip_prefix(')').expect("paréntesis");
                return value;
            }
            if let Some(rest) = input.strip_prefix('t') {
                *input = rest;
                return t;
            }
            if let Some(rest) = input.strip_prefix('-') {
                *input = rest;
                return -factor(input, t);
            }
            let end = input
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(input.len());
            let (number, rest) = input.split_at(end);
            *input = rest;
            number.parse().expect("número")
        }
        let mut input = expression;
        parse(&mut input, t)
    }

    #[test]
    fn expression_matches_value_at_everywhere() {
        let curve = keys(&[(0.5, 0.0), (1.5, 1.0), (3.0, -0.5)]);
        let expression = expression(&curve, "t");
        for step in 0..40 {
            let t = step as f64 * 0.1;
            let expected = value_at(&curve, t).unwrap();
            assert!(
                (eval(&expression, t) - expected).abs() < 1e-5,
                "t={t}: {} vs {expected}",
                eval(&expression, t)
            );
        }
    }

    #[test]
    fn commands_ramp_every_frame_and_hold_flat_stretches() {
        let curve = keys(&[(0.0, 0.0), (0.2, 1.0), (5.0, 1.0)]);
        let lines = commands("gblur@b1", &[("sigma", &|v| v * 10.0)], &curve, 0.04, 6.0, 0.0);
        // 5 fotogramas de rampa + el final de la rampa + el del último keyframe.
        assert_eq!(lines.len(), 7, "{lines:?}");
        assert_eq!(lines[0], "0.0000 gblur@b1 sigma 0.000000");
        assert!(lines[2].starts_with("0.0800 gblur@b1 sigma 4.0"));
        assert!(lines.last().unwrap().starts_with("5.0000"));
    }

    #[test]
    fn set_key_replaces_nearby_and_keeps_order() {
        let mut curve = keys(&[(1.0, 0.0)]);
        set_key(&mut curve, 0.5, 2.0);
        set_key(&mut curve, 1.01, 3.0);
        assert_eq!(curve, keys(&[(0.5, 2.0), (1.0, 3.0)]));
        assert!(varies(&curve));
        assert!(!varies(&keys(&[(0.0, 1.0), (2.0, 1.0)])));
    }
}
