//! Selection editing deliberately writes only explicitly chosen static fields.
use super::{egui, theme, NovaCutWindows, RoughClip, RoughProject};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Field {
    X,
    Y,
    Scale,
    Rotation,
    Opacity,
    Exposure,
    Contrast,
    Saturation,
    Vignette,
    Blur,
    Gain,
    Pan,
    Mute,
    FadeIn,
    FadeOut,
}

impl Field {
    const ALL: [Self; 15] = [
        Self::X,
        Self::Y,
        Self::Scale,
        Self::Rotation,
        Self::Opacity,
        Self::Exposure,
        Self::Contrast,
        Self::Saturation,
        Self::Vignette,
        Self::Blur,
        Self::Gain,
        Self::Pan,
        Self::Mute,
        Self::FadeIn,
        Self::FadeOut,
    ];

    fn spec(self) -> (&'static str, f64, f64) {
        match self {
            Self::X => ("X (px)", -7680.0, 7680.0),
            Self::Y => ("Y (px)", -4320.0, 4320.0),
            Self::Scale => ("Escala (%)", 1.0, 800.0),
            Self::Rotation => ("Rotacion", -3600.0, 3600.0),
            Self::Opacity => ("Opacidad (%)", 0.0, 100.0),
            Self::Exposure => ("Exposicion", -1.0, 1.0),
            Self::Contrast => ("Contraste", -1.0, 1.0),
            Self::Saturation => ("Saturacion", -1.0, 1.0),
            Self::Vignette => ("Vineta", -1.0, 1.0),
            Self::Blur => ("Desenfoque", 0.0, 1.0),
            Self::Gain => ("Ganancia (dB)", -96.0, 24.0),
            Self::Pan => ("Balance L/R", -1.0, 1.0),
            Self::Mute => ("Silencio (0/1)", 0.0, 1.0),
            Self::FadeIn => ("Fundido entrada (s)", 0.0, 3600.0),
            Self::FadeOut => ("Fundido salida (s)", 0.0, 3600.0),
        }
    }

    fn category(self) -> usize {
        match self {
            Self::X | Self::Y | Self::Scale | Self::Rotation | Self::Opacity => 0,
            Self::Exposure | Self::Contrast | Self::Saturation | Self::Vignette | Self::Blur => 1,
            Self::Gain | Self::Pan | Self::Mute => 2,
            Self::FadeIn | Self::FadeOut => 3,
        }
    }

    fn get(self, c: &RoughClip) -> f64 {
        match self {
            Self::X => c.position_x,
            Self::Y => c.position_y,
            Self::Scale => c.scale_percent,
            Self::Rotation => c.rotation,
            Self::Opacity => c.opacity,
            Self::Exposure => c.exposure,
            Self::Contrast => c.contrast,
            Self::Saturation => c.saturation,
            Self::Vignette => c.vignette,
            Self::Blur => c.blur,
            Self::Gain => c.gain_db,
            Self::Pan => c.pan,
            Self::Mute => {
                if c.muted {
                    1.0
                } else {
                    0.0
                }
            }
            Self::FadeIn => c.fade_in_seconds,
            Self::FadeOut => c.fade_out_seconds,
        }
    }

    fn set(self, c: &mut RoughClip, value: f64) {
        if !value.is_finite() {
            return;
        }
        let (_, min, max) = self.spec();
        let value = value.clamp(min, max);
        let half = (c.duration() / 2.0).max(0.0);
        match self {
            Self::X => c.position_x = value,
            Self::Y => c.position_y = value,
            Self::Scale => c.scale_percent = value,
            Self::Rotation => c.rotation = value,
            Self::Opacity => c.opacity = value,
            Self::Exposure => c.exposure = value,
            Self::Contrast => c.contrast = value,
            Self::Saturation => c.saturation = value,
            Self::Vignette => c.vignette = value,
            Self::Blur => c.blur = value,
            Self::Gain => c.gain_db = value,
            Self::Pan => c.pan = value,
            Self::Mute => c.muted = value >= 0.5,
            Self::FadeIn => c.fade_in_seconds = value.min(half),
            Self::FadeOut => c.fade_out_seconds = value.min(half),
        }
    }
}

const CATEGORIES: [&str; 5] = [
    "Transformacion",
    "Color y efectos",
    "Audio",
    "Fundidos",
    "Etiqueta",
];

fn supports(c: &RoughClip, category: usize) -> bool {
    if c.nested.is_some() || c.title.is_some() {
        return false;
    }
    match category {
        0 => c.has_video && !c.is_adjustment,
        1 => c.has_video,
        2 => c.has_audio && !c.is_adjustment,
        3 => (c.has_video || c.has_audio) && !c.is_adjustment,
        4 => true,
        _ => false,
    }
}

#[derive(Default)]
pub(super) struct State {
    selection: Vec<usize>,
    generation: u64,
    values: [Option<f64>; 15],
    categories: [bool; 5],
    pub paste: Option<(Vec<usize>, u64, Box<RoughClip>)>,
}

pub(super) enum Operation {
    Fields(Vec<(Field, f64)>),
    Paste(Box<RoughClip>, [bool; 5]),
    Reset([bool; 5]),
}

#[derive(Default, Debug, PartialEq)]
struct Report {
    changed: usize,
    unchanged: usize,
    locked: usize,
    incompatible: usize,
    partial: usize,
}

/// Returns the common value only when all eligible clips agree.
fn common(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let mut values = values.into_iter();
    let first = values.next()?;
    (first.is_finite() && values.all(|v| v == first)).then_some(first)
}

fn apply(project: &mut RoughProject, indices: &[usize], operation: &Operation) -> Report {
    let mut report = Report::default();
    let defaults = RoughClip::default();
    let mut unique = indices.to_vec();
    unique.sort_unstable();
    unique.dedup();
    for index in unique {
        let Some(c) = project.clips.get(index) else {
            continue;
        };
        if project.clip_locked(c) {
            report.locked += 1;
            continue;
        }
        let mut next = c.clone();
        let mut accepted = false;
        let mut rejected = false;
        match operation {
            Operation::Fields(fields) => {
                for &(field, value) in fields {
                    if supports(c, field.category()) && value.is_finite() {
                        field.set(&mut next, value);
                        accepted = true;
                    } else {
                        rejected = true;
                    }
                }
            }
            Operation::Paste(_, categories) | Operation::Reset(categories) => {
                let source = match operation {
                    Operation::Paste(source, _) => source.as_ref(),
                    _ => &defaults,
                };
                for (category, enabled) in categories.iter().enumerate() {
                    if !enabled {
                        continue;
                    }
                    if !supports(c, category) || !supports(source, category) {
                        rejected = true;
                        continue;
                    }
                    accepted = true;
                    for field in Field::ALL {
                        if field.category() == category {
                            field.set(&mut next, field.get(source));
                        }
                    }
                    match category {
                        1 => {
                            next.wheels = source.wheels;
                            next.curves = source.curves.clone();
                            next.chroma = source.chroma;
                            next.lut = source.lut.clone();
                            next.mask = source.mask;
                            next.fusion = source.fusion;
                        }
                        4 => next.label = source.label,
                        _ => {}
                    }
                }
            }
        }
        if !accepted && rejected {
            report.incompatible += 1;
            continue;
        }
        if rejected {
            report.partial += 1;
        }
        // Compare the whole candidate, not just scalar fields: effects may be the only change.
        if serde_json::to_value(c).ok() != serde_json::to_value(&next).ok() {
            project.clips[index] = next;
            report.changed += 1;
        } else {
            report.unchanged += 1;
        }
    }
    report
}

impl NovaCutWindows {
    pub(super) fn run_batch(&mut self, indices: Vec<usize>, operation: Operation) {
        let before = self.project.clone();
        let report = apply(&mut self.project, &indices, &operation);
        // finish_edit also commits any pending single-clip inspector baseline separately.
        if report.changed > 0 {
            self.finish_edit(before);
        }
        self.status = format!(
            "Lote: {} cambiados, {} sin cambios, {} bloqueados, {} incompatibles, {} parciales",
            report.changed, report.unchanged, report.locked, report.incompatible, report.partial
        );
    }

    pub(super) fn batch_inspector(&mut self, ui: &mut egui::Ui) {
        let indices = self.selected_indices();
        if self.batch_state.selection != indices
            || self.batch_state.generation != self.document_generation
        {
            self.batch_state.selection = indices.clone();
            self.batch_state.generation = self.document_generation;
            self.batch_state.values = [None; 15];
        }
        ui.colored_label(
            theme::ACCENT,
            format!("{} clips seleccionados", indices.len()),
        );
        let locked = indices
            .iter()
            .filter(|&&i| self.project.clip_locked(&self.project.clips[i]))
            .count();
        ui.small(format!(
            "{locked} bloqueados. Marca los campos que quieras igualar."
        ));
        ui.small("Valores actuales de clips compatibles desbloqueados. Aplicar fija valores absolutos; no modifica keyframes.");
        let mut previous_category = usize::MAX;
        for (slot, field) in Field::ALL.into_iter().enumerate() {
            if previous_category != field.category() {
                theme::section_label(ui, CATEGORIES[field.category()]);
                previous_category = field.category();
            }
            let values: Vec<f64> = indices
                .iter()
                .filter_map(|&i| {
                    let c = &self.project.clips[i];
                    (!self.project.clip_locked(c) && supports(c, field.category()))
                        .then(|| field.get(c))
                })
                .collect();
            let current = common(values.iter().copied());
            let (label, min, max) = field.spec();
            ui.push_id(slot, |ui| {
                ui.add_enabled_ui(!values.is_empty(), |ui| {
                    let mut enabled = self.batch_state.values[slot].is_some();
                    if ui.checkbox(&mut enabled, label).changed() {
                        self.batch_state.values[slot] =
                            enabled.then(|| current.unwrap_or(field.get(&RoughClip::default())));
                    }
                    ui.horizontal(|ui| {
                        ui.small(if values.is_empty() {
                            "No compatible".to_owned()
                        } else {
                            current
                                .map_or_else(|| "Mixto".to_owned(), |v| format!("Actual: {v:.2}"))
                        });
                        if let Some(value) = &mut self.batch_state.values[slot] {
                            if field == Field::Mute {
                                let mut muted = *value >= 0.5;
                                ui.checkbox(&mut muted, "Silenciar")
                                    .on_hover_text("Silencia todos los clips seleccionados");
                                *value = if muted { 1.0 } else { 0.0 };
                            } else {
                                ui.add(egui::DragValue::new(value).range(min..=max).speed(0.1));
                            }
                        }
                    });
                });
            });
        }
        let fields: Vec<_> = Field::ALL
            .into_iter()
            .zip(self.batch_state.values)
            .filter_map(|(field, value)| value.map(|v| (field, v)))
            .collect();
        if ui
            .add_enabled(
                !fields.is_empty(),
                egui::Button::new("Aplicar campos marcados"),
            )
            .on_hover_text("Aplica a todos los seleccionados los campos marcados")
            .on_disabled_hover_text("Aplica a todos los seleccionados los campos marcados")
            .clicked()
        {
            self.run_batch(indices.clone(), Operation::Fields(fields));
            self.batch_state.values = [None; 15];
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Copiar atributos del principal")
                .on_hover_text("Copia los atributos del clip principal para pegarlos en otros")
                .clicked()
            {
                self.copy_attributes();
            }
            if ui
                .button("Activar / desactivar")
                .on_hover_text("Activa o desactiva todos los seleccionados")
                .clicked()
            {
                self.toggle_selection_enabled();
            }
        });
        if ui
            .button("Fundidos de 1 s (limitados por clip)")
            .on_hover_text("Pone fundidos de entrada y salida de 1 s")
            .clicked()
        {
            self.quick_fade();
        }
        if ui
            .add_enabled(
                self.attribute_clipboard.is_some(),
                egui::Button::new("Pegar atributos..."),
            )
            .on_hover_text("Elige qué atributos copiados pegar en los seleccionados")
            .on_disabled_hover_text("Elige qué atributos copiados pegar en los seleccionados")
            .clicked()
        {
            self.paste_attributes();
        }
        ui.collapsing("Restablecer categorias", |ui| {
            for (category, label) in CATEGORIES.iter().enumerate() {
                ui.checkbox(&mut self.batch_state.categories[category], *label);
            }
            ui.small("Color incluye ruedas, curvas, chroma, LUT, mascara y fusion. No borra animacion ni transiciones.");
            if ui.add_enabled(self.batch_state.categories.iter().any(|v| *v), egui::Button::new("Restablecer seleccionadas")).on_hover_text("Vuelve esos atributos a sus valores por defecto").on_disabled_hover_text("Vuelve esos atributos a sus valores por defecto").clicked() {
                self.run_batch(indices, Operation::Reset(self.batch_state.categories));
            }
        });
        ui.small("Titulos y nidos se omiten. Capas de ajuste: solo color y etiqueta. Los keyframes existentes pueden prevalecer sobre los valores estaticos.");
    }

    pub(super) fn batch_paste_dialog(&mut self, context: &egui::Context) {
        let Some((indices, generation, source)) = self.batch_state.paste.take() else {
            return;
        };
        if generation != self.document_generation || indices != self.selected_indices() {
            self.status = "Pegado cancelado: cambio el documento o la seleccion".to_owned();
            return;
        }
        let mut open = true;
        let mut apply_now = false;
        let mut cancel = false;
        egui::Window::new("Pegar atributos a la seleccion")
            .id(egui::Id::new("batch-paste"))
            .collapsible(false).resizable(false).open(&mut open).show(context, |ui| {
                ui.colored_label(theme::ACCENT, format!("Destino: {} clips", indices.len()));
                ui.small(format!("Origen: {}", source.name()));
                for (category, label) in CATEGORIES.iter().enumerate() {
                    ui.add_enabled_ui(supports(&source, category), |ui| {
                        ui.checkbox(&mut self.batch_state.categories[category], *label);
                    });
                }
                ui.small("Solo valores estaticos. Se conservan tiempos, rutas de medios, retiming y keyframes.");
                ui.small("Color incluye ruedas, curvas, chroma, LUT, mascara y fusion. Fundidos limitados a media duracion por clip.");
                ui.horizontal(|ui| {
                    apply_now = ui.add_enabled(self.batch_state.categories.iter().enumerate()
                        .any(|(i, v)| *v && supports(&source, i)), egui::Button::new("Pegar")).on_hover_text("Pega los atributos marcados").on_disabled_hover_text("Pega los atributos marcados").clicked();
                    cancel = ui.button("Cancelar").on_hover_text("Cierra sin pegar").clicked();
                });
            });
        if apply_now {
            self.run_batch(
                indices,
                Operation::Paste(source, self.batch_state.categories),
            );
        } else if open && !cancel {
            self.batch_state.paste = Some((indices, generation, source));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(clips: Vec<RoughClip>) -> RoughProject {
        RoughProject {
            clips,
            ..Default::default()
        }
    }

    #[test]
    fn mixed_values_and_empty_selection() {
        assert_eq!(common([]), None);
        assert_eq!(common([2.0, 2.0]), Some(2.0));
        assert_eq!(common([2.0, 3.0]), None);
        assert_eq!(common([f64::NAN]), None);
    }

    #[test]
    fn batch_changes_only_requested_fields_and_deduplicates() {
        let mut p = project(vec![RoughClip::default(); 2]);
        let report = apply(
            &mut p,
            &[0, 0, 1, 99],
            &Operation::Fields(vec![(Field::X, 12.0)]),
        );
        assert_eq!(report.changed, 2);
        assert_eq!(p.clips[1].position_x, 12.0);
        assert_eq!(p.clips[1].scale_percent, 100.0);
        assert_eq!(
            apply(&mut p, &[0, 1], &Operation::Fields(vec![(Field::X, 12.0)])).unchanged,
            2
        );
    }

    #[test]
    fn fades_clamp_independently_to_retimed_duration() {
        let mut p = project(vec![
            RoughClip {
                out_seconds: 8.0,
                speed: 2.0,
                ..Default::default()
            },
            RoughClip::default(),
        ]);
        apply(
            &mut p,
            &[0, 1],
            &Operation::Fields(vec![(Field::FadeIn, 3.0), (Field::FadeOut, 3.0)]),
        );
        assert_eq!(p.clips[0].fade_in_seconds, 2.0);
        assert_eq!(p.clips[1].fade_out_seconds, 0.5);
    }

    #[test]
    fn locked_incompatible_and_partial_are_reported() {
        let mut p = project(vec![
            RoughClip::default(),
            RoughClip {
                has_video: false,
                ..Default::default()
            },
            RoughClip {
                nested: Some(vec![]),
                track: 1,
                ..Default::default()
            },
        ]);
        p.video_locked = vec![true, false];
        let report = apply(
            &mut p,
            &[0, 1, 2],
            &Operation::Fields(vec![(Field::Gain, 4.0), (Field::X, 12.0)]),
        );
        assert_eq!(
            report,
            Report {
                changed: 1,
                locked: 1,
                incompatible: 1,
                partial: 1,
                ..Default::default()
            }
        );
        assert_eq!(p.clips[0].gain_db, 0.0);
        assert_eq!(p.clips[1].gain_db, 4.0);
    }

    #[test]
    fn selective_paste_and_reset_preserve_every_unselected_property() {
        let mut clip = RoughClip {
            path: "original.mov".into(),
            proxy: Some("proxy.mov".into()),
            in_seconds: 2.0,
            out_seconds: 9.0,
            timeline_start: 20.0,
            speed: 2.0,
            gain_db: 7.0,
            transition: Some("fade".into()),
            keyframes: Some(vec![super::super::TransformKeyframe {
                t: 0.0,
                x: 4.0,
                y: 5.0,
                scale: 90.0,
                opacity: 60.0,
            }]),
            speed_ramp: Some(vec![super::super::SpeedPoint {
                source_t: 0.0,
                speed: 2.0,
            }]),
            ..Default::default()
        };
        let mut p = project(vec![clip.clone()]);
        let source = RoughClip {
            exposure: 0.5,
            muted: true,
            gain_db: -5.0,
            ..Default::default()
        };
        apply(
            &mut p,
            &[0],
            &Operation::Paste(Box::new(source), [false, true, false, false, false]),
        );
        clip.exposure = 0.5;
        assert_eq!(
            serde_json::to_value(&p.clips[0]).unwrap(),
            serde_json::to_value(&clip).unwrap()
        );
        apply(
            &mut p,
            &[0],
            &Operation::Reset([false, true, false, false, false]),
        );
        clip.exposure = 0.0;
        assert_eq!(
            serde_json::to_value(&p.clips[0]).unwrap(),
            serde_json::to_value(&clip).unwrap()
        );
        apply(
            &mut p,
            &[0],
            &Operation::Fields(vec![(Field::X, 25.0), (Field::Scale, 150.0)]),
        );
        clip.position_x = 25.0;
        clip.scale_percent = 150.0;
        assert_eq!(
            serde_json::to_value(&p.clips[0]).unwrap(),
            serde_json::to_value(&clip).unwrap()
        );
        apply(
            &mut p,
            &[0],
            &Operation::Reset([true, false, false, false, false]),
        );
        clip.position_x = 0.0;
        clip.scale_percent = 100.0;
        assert_eq!(
            serde_json::to_value(&p.clips[0]).unwrap(),
            serde_json::to_value(&clip).unwrap()
        );
    }

    #[test]
    fn audio_paste_includes_mute_and_reset_restores_defaults() {
        let mut p = project(vec![RoughClip::default()]);
        let source = RoughClip {
            gain_db: 12.0,
            pan: -0.5,
            muted: true,
            ..Default::default()
        };
        apply(
            &mut p,
            &[0],
            &Operation::Paste(Box::new(source), [false, false, true, false, false]),
        );
        assert_eq!(
            (p.clips[0].gain_db, p.clips[0].pan, p.clips[0].muted),
            (12.0, -0.5, true)
        );
        apply(
            &mut p,
            &[0],
            &Operation::Reset([false, false, true, false, false]),
        );
        assert_eq!(
            (p.clips[0].gain_db, p.clips[0].pan, p.clips[0].muted),
            (0.0, 0.0, false)
        );
    }

    #[test]
    fn invalid_values_and_empty_categories_are_noops() {
        let mut p = project(vec![RoughClip::default()]);
        assert_eq!(
            apply(
                &mut p,
                &[0],
                &Operation::Fields(vec![(Field::Gain, f64::INFINITY)])
            )
            .changed,
            0
        );
        assert_eq!(
            apply(&mut p, &[0], &Operation::Reset([false; 5])).changed,
            0
        );
        apply(
            &mut p,
            &[0],
            &Operation::Fields(vec![(Field::Pan, 99.0), (Field::Scale, -1.0)]),
        );
        assert_eq!(p.clips[0].pan, 1.0);
        assert_eq!(p.clips[0].scale_percent, 1.0);
    }

    #[test]
    fn generated_clips_are_skipped_except_adjustment_color_and_label() {
        let mut p = project(vec![
            RoughClip {
                title: Some(super::super::Titulo::default()),
                ..Default::default()
            },
            RoughClip {
                is_adjustment: true,
                ..Default::default()
            },
            RoughClip {
                has_video: false,
                ..Default::default()
            },
        ]);
        let source = RoughClip {
            exposure: 0.3,
            position_x: 10.0,
            ..Default::default()
        };
        let report = apply(
            &mut p,
            &[0, 1, 2],
            &Operation::Paste(Box::new(source), [true, true, false, false, false]),
        );
        assert_eq!(report.changed, 1);
        assert_eq!(report.partial, 1);
        assert_eq!(report.incompatible, 2);
        assert_eq!(p.clips[1].exposure, 0.3);
        assert_eq!(p.clips[1].position_x, 0.0);
    }

    #[test]
    fn effects_only_paste_and_reset_are_real_changes() {
        let mut p = project(vec![RoughClip::default()]);
        let source = RoughClip {
            lut: Some("look.cube".into()),
            fusion: super::super::Fusion::Multiply,
            ..Default::default()
        };
        let operation = Operation::Paste(Box::new(source), [false, true, false, false, false]);
        assert_eq!(apply(&mut p, &[0], &operation).changed, 1);
        assert_eq!(apply(&mut p, &[0], &operation).unchanged, 1);
        assert_eq!(
            apply(
                &mut p,
                &[0],
                &Operation::Reset([false, true, false, false, false])
            )
            .changed,
            1
        );
        assert!(p.clips[0].lut.is_none());
        assert!(p.clips[0].fusion == super::super::Fusion::Normal);
    }

    #[test]
    fn incompatible_source_never_erases_destination_attributes() {
        let mut p = project(vec![RoughClip {
            exposure: 0.5,
            ..Default::default()
        }]);
        let source = RoughClip {
            has_video: false,
            ..Default::default()
        };
        let report = apply(
            &mut p,
            &[0],
            &Operation::Paste(Box::new(source), [false, true, false, false, false]),
        );
        assert_eq!(report.incompatible, 1);
        assert_eq!(report.changed, 0);
        assert_eq!(p.clips[0].exposure, 0.5);
    }
}
