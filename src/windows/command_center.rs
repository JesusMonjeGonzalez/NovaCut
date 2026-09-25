use super::{egui, navigation, parse_timecode, theme, timecode, BottomTab, NovaCutWindows};

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    query: String,
    cursor: usize,
    focus: bool,
}
impl State {
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.cursor = 0;
        self.focus = true;
    }
}

#[derive(Clone, Copy)]
enum Action {
    Save,
    Undo,
    Redo,
    Import,
    Export,
    CopyAttributes,
    PasteAttributes,
    Fade,
    FitSelection,
    RangeSelection,
    InvertSelection,
    SelectDisabled,
    PreviousMarker,
    NextMarker,
    PreviousGap,
    NextGap,
    Mixer,
    Subtitles,
    Markers,
    Clips,
    Shortcuts,
    Clip(usize),
    Marker(f64),
    Subtitle(f64),
    Time(f64),
}
struct Entry {
    title: String,
    detail: String,
    action: Action,
    unavailable: Option<&'static str>,
}

impl NovaCutWindows {
    fn selection_span(&self) -> Option<(f64, f64)> {
        navigation::span(self.selected_indices().into_iter().map(|i| {
            let c = &self.project.clips[i];
            (c.timeline_start, c.timeline_start + c.duration())
        }))
    }

    fn command_entries(&self) -> Vec<Entry> {
        let mut entries = Vec::new();
        let selected = !self.selected_indices().is_empty();
        let editable = self
            .selected_indices()
            .into_iter()
            .any(|i| !self.project.clip_locked(&self.project.clips[i]));
        for (title, detail, action, unavailable) in [
            ("Guardar proyecto", "Documento | Ctrl+S", Action::Save, None),
            (
                "Deshacer",
                "Edicion | Ctrl+Z",
                Action::Undo,
                (self.undo_stack.is_empty() && self.pending_edit.is_none())
                    .then_some("No hay cambios para deshacer"),
            ),
            (
                "Rehacer",
                "Edicion | Ctrl+Y",
                Action::Redo,
                self.redo_stack
                    .is_empty()
                    .then_some("No hay cambios para rehacer"),
            ),
            (
                "Importar medios",
                "Documento | Ctrl+I",
                Action::Import,
                (!self.ffmpeg_ready || self.import_result.is_some())
                    .then_some("Requiere FFmpeg y ninguna importacion en curso"),
            ),
            (
                "Exportar montaje",
                "Entrega | Ctrl+Mayus+E",
                Action::Export,
                (!self.ffmpeg_ready
                    || self.export_result.is_some()
                    || self.project.clips.is_empty())
                .then_some("Requiere medios, FFmpeg y ninguna exportacion en curso"),
            ),
            (
                "Copiar atributos",
                "Seleccion | Ctrl+Alt+C",
                Action::CopyAttributes,
                (!selected).then_some("Selecciona un clip"),
            ),
            (
                "Pegar atributos selectivos",
                "Seleccion | Ctrl+Alt+V",
                Action::PasteAttributes,
                (!editable || self.attribute_clipboard.is_none())
                    .then_some("Copia atributos y selecciona destinos editables"),
            ),
            (
                "Fundidos rapidos en seleccion",
                "Seleccion | Ambos bordes, hasta 1 s",
                Action::Fade,
                (!editable).then_some("Selecciona clips en pistas desbloqueadas"),
            ),
            (
                "Encuadrar seleccion",
                "Vista | Ajusta zoom y desplazamiento",
                Action::FitSelection,
                (!selected).then_some("Selecciona al menos un clip"),
            ),
            (
                "Rango de trabajo desde seleccion",
                "Rango | Entrada y salida sin activar exportacion parcial",
                Action::RangeSelection,
                (!selected).then_some("Selecciona al menos un clip"),
            ),
            (
                "Invertir seleccion",
                "Seleccion | Intercambia seleccionados y no seleccionados",
                Action::InvertSelection,
                self.project
                    .clips
                    .is_empty()
                    .then_some("El montaje esta vacio"),
            ),
            (
                "Seleccionar clips desactivados",
                "Revision | Material excluido del montaje",
                Action::SelectDisabled,
                (!self.project.clips.iter().any(|c| !c.enabled))
                    .then_some("No hay clips desactivados"),
            ),
            (
                "Marcador anterior",
                "Navegacion | Marcador previo",
                Action::PreviousMarker,
                self.project
                    .markers
                    .is_empty()
                    .then_some("No hay marcadores"),
            ),
            (
                "Marcador siguiente",
                "Navegacion | Proximo marcador",
                Action::NextMarker,
                self.project
                    .markers
                    .is_empty()
                    .then_some("No hay marcadores"),
            ),
            (
                "Hueco anterior en pista",
                "Revision | Pista del clip principal; cuenta clips activos",
                Action::PreviousGap,
                (!selected).then_some("Selecciona un clip de la pista a revisar"),
            ),
            (
                "Hueco siguiente en pista",
                "Revision | Pista del clip principal; cuenta clips activos",
                Action::NextGap,
                (!selected).then_some("Selecciona un clip de la pista a revisar"),
            ),
            ("Abrir mezclador", "Panel | Audio", Action::Mixer, None),
            (
                "Abrir subtitulos",
                "Panel | Texto y sincronizacion",
                Action::Subtitles,
                None,
            ),
            (
                "Abrir marcadores",
                "Panel | Notas de montaje",
                Action::Markers,
                None,
            ),
            (
                "Abrir planos",
                "Panel | Lista de clips",
                Action::Clips,
                None,
            ),
            (
                "Consultar atajos",
                "Ayuda | Teclado",
                Action::Shortcuts,
                None,
            ),
        ] {
            entries.push(Entry {
                title: title.into(),
                detail: detail.into(),
                action,
                unavailable,
            });
        }
        for (index, clip) in self.project.clips.iter().enumerate() {
            entries.push(Entry {
                title: clip.name(),
                detail: format!(
                    "Clip | {}{} | {} | {}",
                    if clip.has_video { "V" } else { "A" },
                    clip.track + 1,
                    timecode(clip.timeline_start, self.project.fps),
                    clip.path.display()
                ),
                action: Action::Clip(index),
                unavailable: None,
            });
        }
        for marker in &self.project.markers {
            entries.push(Entry {
                title: marker.name.clone(),
                detail: format!("Marcador | {}", timecode(marker.time, self.project.fps)),
                action: Action::Marker(marker.time),
                unavailable: None,
            });
        }
        for subtitle in &self.project.subtitles {
            entries.push(Entry {
                title: subtitle.text.replace('\n', " "),
                detail: format!("Subtitulo | {}", timecode(subtitle.start, self.project.fps)),
                action: Action::Subtitle(subtitle.start),
                unavailable: None,
            });
        }
        entries
    }

    pub(super) fn show_command_center(&mut self, context: &egui::Context) {
        if !self.command_center.open {
            return;
        }
        let mut chosen = None;
        let response = egui::Modal::new(egui::Id::new("command-center")).show(context, |ui| {
            ui.set_width((context.screen_rect().width() - 48.0).clamp(240.0, 680.0));
            ui.heading("Centro de comandos");
            ui.label(
                egui::RichText::new("Comandos, clips, marcadores y subtitulos. Filtros: > @ # =")
                    .small()
                    .color(theme::TEXT_DIM),
            );
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.command_center.query)
                    .desired_width(f32::INFINITY)
                    .hint_text("Buscar... o :00:01:12:00 para ir a timecode"),
            );
            if self.command_center.focus {
                search.request_focus();
                self.command_center.focus = false;
            }
            if search.changed() {
                self.command_center.cursor = 0;
            }
            let query = self.command_center.query.trim();
            let mut entries = self.command_entries();
            if let Some(text) = query.strip_prefix(':') {
                entries.clear();
                if let Some(seconds) = parse_timecode(text, self.project.timebase())
                    .filter(|t| t.is_finite() && *t >= 0.0 && *t <= self.project.duration())
                {
                    entries.push(Entry {
                        title: format!("Ir a {}", timecode(seconds, self.project.fps)),
                        detail: "Posicion del cabezal".into(),
                        action: Action::Time(seconds),
                        unavailable: None,
                    });
                }
            }
            let (scope, needle) = match query.chars().next() {
                Some(c @ ('>' | '@' | '#' | '=' | ':')) => (Some(c), query[1..].trim()),
                _ => (None, query),
            };
            let mut matches: Vec<_> = entries
                .into_iter()
                .filter_map(|entry| {
                    let in_scope = match scope {
                        Some('@') => matches!(entry.action, Action::Clip(_)),
                        Some('#') => matches!(entry.action, Action::Marker(_)),
                        Some('=') => matches!(entry.action, Action::Subtitle(_)),
                        Some('>') => !matches!(
                            entry.action,
                            Action::Clip(_)
                                | Action::Marker(_)
                                | Action::Subtitle(_)
                                | Action::Time(_)
                        ),
                        _ => true,
                    };
                    if !in_scope {
                        return None;
                    }
                    navigation::search_score(
                        if scope == Some(':') { "" } else { needle },
                        &entry.title,
                        &entry.detail,
                    )
                    .map(|score| (score, entry))
                })
                .collect();
            matches.sort_by_key(|(score, _)| *score);
            if matches.is_empty() {
                ui.label(if scope == Some(':') {
                    "Introduce segundos o HH:MM:SS:FF dentro del montaje."
                } else {
                    "Sin resultados. Prueba menos palabras o cambia el filtro."
                });
            }
            self.command_center.cursor = self
                .command_center
                .cursor
                .min(matches.len().saturating_sub(1));
            let down =
                context.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
            let up =
                context.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
            if down {
                self.command_center.cursor =
                    (self.command_center.cursor + 1).min(matches.len().saturating_sub(1));
            }
            if up {
                self.command_center.cursor = self.command_center.cursor.saturating_sub(1);
            }
            let enter =
                context.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui.label(
                egui::RichText::new(format!(
                    "{} resultados | Flechas: navegar | Enter: ejecutar | Esc: cerrar",
                    matches.len()
                ))
                .small(),
            );
            egui::ScrollArea::vertical()
                .max_height((context.screen_rect().height() - 250.0).clamp(100.0, 420.0))
                .show(ui, |ui| {
                    for (index, (_, entry)) in matches.iter().enumerate() {
                        let selected = index == self.command_center.cursor;
                        let text = format!(
                            "{}\n{}",
                            entry.title,
                            entry.unavailable.unwrap_or(&entry.detail)
                        );
                        let row = ui.add_enabled(
                            entry.unavailable.is_none(),
                            egui::Button::new(text)
                                .selected(selected)
                                .min_size(egui::vec2(ui.available_width(), 44.0)),
                        );
                        if selected && (up || down || search.changed()) {
                            row.scroll_to_me(Some(egui::Align::Center));
                        }
                        if row.clicked() || (selected && enter && entry.unavailable.is_none()) {
                            chosen = Some(entry.action);
                        }
                    }
                });
        });
        if response.should_close() {
            self.command_center.open = false;
        }
        if let Some(action) = chosen {
            self.command_center.open = false;
            self.run_center_action(action);
        }
    }

    fn run_center_action(&mut self, action: Action) {
        match action {
            Action::Save => self.save_project(false),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Import => self.import_media(),
            Action::Export => self.export(),
            Action::CopyAttributes => self.copy_attributes(),
            Action::PasteAttributes => self.paste_attributes(),
            Action::Fade => self.quick_fade(),
            Action::FitSelection => {
                if let Some((start, end)) = self.selection_span() {
                    (self.zoom, self.hscroll) =
                        navigation::fit_span(start, end, self.project.duration());
                    self.status = "Vista ajustada a la seleccion".into();
                }
            }
            Action::RangeSelection => {
                if let Some((start, end)) = self.selection_span() {
                    self.work_in = Some(start);
                    self.work_out = Some(end);
                    self.status =
                        "Rango marcado desde la seleccion; el modo de exportacion no cambia".into();
                }
            }
            Action::InvertSelection | Action::SelectDisabled => {
                let current: std::collections::BTreeSet<_> =
                    self.selected_indices().into_iter().collect();
                self.selection = self
                    .project
                    .clips
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| {
                        if matches!(action, Action::InvertSelection) {
                            !current.contains(i)
                        } else {
                            !c.enabled
                        }
                    })
                    .map(|(i, _)| i)
                    .collect();
                self.selected = self.selection.iter().next().copied();
                self.status = format!("{} clips seleccionados", self.selection.len());
            }
            Action::PreviousMarker | Action::NextMarker => {
                if let Some(t) = navigation::next_point(
                    self.project.markers.iter().map(|m| m.time),
                    self.playhead,
                    matches!(action, Action::NextMarker),
                    self.frame_duration() / 2.0,
                ) {
                    self.stop_playback();
                    self.seek(t);
                } else {
                    self.status = "No hay otro marcador en esa direccion".into();
                }
            }
            Action::PreviousGap | Action::NextGap => {
                if let Some(clip) = self.selected.and_then(|i| self.project.clips.get(i)) {
                    let intervals = self
                        .project
                        .clips
                        .iter()
                        .filter(|c| {
                            c.enabled && c.track == clip.track && c.has_video == clip.has_video
                        })
                        .map(|c| (c.timeline_start, c.timeline_start + c.duration()))
                        .collect();
                    let gaps = navigation::gaps(
                        intervals,
                        self.project.duration(),
                        self.frame_duration() * 0.99,
                    );
                    if let Some(t) = navigation::next_point(
                        gaps.iter().map(|g| g.0),
                        self.playhead,
                        matches!(action, Action::NextGap),
                        self.frame_duration() / 2.0,
                    ) {
                        self.stop_playback();
                        self.seek(t);
                        self.status = format!("Hueco en pista: {}", timecode(t, self.project.fps));
                    } else {
                        self.status =
                            "No hay otro hueco en esa direccion en la pista elegida".into();
                    }
                }
            }
            Action::Mixer | Action::Subtitles | Action::Markers | Action::Clips => {
                self.bottom_open = true;
                self.bottom_tab = match action {
                    Action::Mixer => BottomTab::Mixer,
                    Action::Subtitles => BottomTab::Subtitles,
                    Action::Markers => BottomTab::Markers,
                    _ => BottomTab::Clips,
                };
            }
            Action::Shortcuts => self.show_shortcuts = true,
            Action::Clip(i) => {
                if let Some(clip) = self.project.clips.get(i) {
                    let start = clip.timeline_start;
                    self.select_only(i);
                    self.stop_playback();
                    self.seek(start);
                }
            }
            Action::Marker(t) | Action::Subtitle(t) | Action::Time(t) => {
                if matches!(action, Action::Marker(_)) {
                    self.bottom_open = true;
                    self.bottom_tab = BottomTab::Markers;
                }
                if matches!(action, Action::Subtitle(_)) {
                    self.bottom_open = true;
                    self.bottom_tab = BottomTab::Subtitles;
                }
                self.stop_playback();
                self.seek(t);
            }
        }
    }
}
