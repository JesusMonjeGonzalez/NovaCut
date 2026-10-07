//! Recorrido guiado de la primera visita.
//!
//! Ilumina cada zona real de la interfaz (los rectángulos se registran
//! mientras se dibuja) y, en los pasos clave, pide hacer la acción y la
//! marca como hecha al detectarla. El velo solo se pinta: no captura el
//! ratón, así que se puede seguir usando la app durante el recorrido.

use std::collections::HashMap;

use eframe::egui;

use super::{theme, NovaCutWindows};

/// Lo que un paso pide hacer para darse por hecho.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Condicion {
    HayClips,
    FuenteAbierta,
    ClipSeleccionado,
    HaReproducido,
    HayTexto,
}

pub(super) struct Paso {
    /// Zona que se ilumina (clave de `Tutorial::zonas`); `None`, centrado.
    pub zona: Option<&'static str>,
    pub titulo: &'static str,
    pub texto: &'static str,
    /// Acción que se propone y cómo se reconoce.
    pub accion: Option<(&'static str, Condicion)>,
}

pub(super) const PASOS: &[Paso] = &[
    Paso {
        zona: None,
        titulo: "Bienvenida a NovaCut",
        texto: "En dos minutos te enseño dónde está todo. Cada paso ilumina una zona; en algunos te pediré que lo hagas tú para que lo pruebes de verdad. Puedes seguir usando la app mientras tanto.",
        accion: None,
    },
    Paso {
        zona: Some("importar"),
        titulo: "1 · Trae tu material",
        texto: "Pulsa «+ Importar» o arrastra vídeos, audio o imágenes desde el Explorador a cualquier parte de la ventana.",
        accion: Some(("Importa al menos un vídeo", Condicion::HayClips)),
    },
    Paso {
        zona: Some("medios"),
        titulo: "2 · Vista previa antes de usarlo",
        texto: "Aquí quedan tus archivos. Un clic abre su vista previa: reprodúcelo, marca entrada y salida (I y O) y pulsa «Insertar en el cabezal» para meter solo ese trozo. También puedes arrastrarlo a la línea de tiempo.",
        accion: Some(("Haz clic en un medio para verlo", Condicion::FuenteAbierta)),
    },
    Paso {
        zona: Some("timeline"),
        titulo: "3 · La línea de tiempo",
        texto: "Haz clic en un clip para seleccionarlo (se marca en blanco). Arrástralo para moverlo, arrastra sus bordes para recortarlo y los círculos de arriba para hacer fundidos. Clic derecho: partir, duplicar, separar el audio y más. Clic en la regla: mover el cabezal.",
        accion: Some(("Selecciona un clip", Condicion::ClipSeleccionado)),
    },
    Paso {
        zona: Some("herramientas"),
        titulo: "4 · Deshacer, partir y herramientas",
        texto: "↶ y ↷ deshacen y rehacen (Ctrl+Z, Ctrl+Mayús+Z). «Partir» corta en el cabezal (S). La flecha es la herramienta normal; las demás son para recortes avanzados. La línea de debajo dice siempre qué hará el ratón.",
        accion: None,
    },
    Paso {
        zona: Some("monitor"),
        titulo: "5 · El monitor",
        texto: "Espacio reproduce y pausa. Con un clip o un texto seleccionado, arrástralo aquí para moverlo y usa la rueda del ratón para cambiar su tamaño.",
        accion: Some(("Reproduce con Espacio o ▶", Condicion::HaReproducido)),
    },
    Paso {
        zona: Some("texto"),
        titulo: "6 · Textos",
        texto: "«T Texto» añade un texto en el cabezal, en una pista por encima del vídeo. Escríbelo en el Inspector y colócalo arrastrándolo en el monitor.",
        accion: Some(("Añade un texto", Condicion::HayTexto)),
    },
    Paso {
        zona: Some("inspector"),
        titulo: "7 · El Inspector: todos los ajustes",
        texto: "Con un clip seleccionado, aquí están sus ajustes. Usa «Ir a» para saltar a Color (exposición, saturación, Lumetri, ruedas, curvas), Encuadre (posición, escala y animación), Audio y Fundidos.",
        accion: None,
    },
    Paso {
        zona: Some("menu_herramientas"),
        titulo: "8 · Herramientas automáticas",
        texto: "Acortar pausas, igualar el volumen de las voces, quitar ruido, multicámara, capítulos y más. En la pestaña «Transcripción» puedes editar el vídeo borrando texto.",
        accion: None,
    },
    Paso {
        zona: Some("exportar"),
        titulo: "9 · Exportar",
        texto: "Cuando esté listo, «Exportar MP4». Al lado tienes la resolución (1080p, 4K…) y el formato (H.264, HEVC…); en la ventana de exportación eliges preajustes y calidad.",
        accion: None,
    },
    Paso {
        zona: Some("ayuda"),
        titulo: "10 · Si te pierdes",
        texto: "«?» muestra todos los atajos y desde ahí puedes repetir este recorrido. 🔍 busca cualquier acción por su nombre.",
        accion: None,
    },
];

#[derive(Default)]
pub(super) struct Tutorial {
    pub activo: bool,
    pub paso: usize,
    /// Zonas de la interfaz del último fotograma, por nombre.
    pub zonas: HashMap<&'static str, egui::Rect>,
    /// Se ha reproducido algo desde que empezó el paso del monitor.
    pub reproducido: bool,
    /// Ya se ofreció solo en esta sesión.
    pub ofrecido: bool,
}

impl Tutorial {
    pub fn empezar(&mut self) {
        self.activo = true;
        self.paso = 0;
        self.reproducido = false;
    }

    pub fn marcar(&mut self, zona: &'static str, rect: egui::Rect) {
        if self.activo {
            self.zonas.insert(zona, rect);
        }
    }
}

/// Dónde va la tarjeta: debajo de la zona si cabe, si no encima, y si
/// tampoco, al lado; siempre dentro de la pantalla.
pub(super) fn colocar_tarjeta(
    zona: egui::Rect,
    tarjeta: egui::Vec2,
    pantalla: egui::Rect,
) -> egui::Pos2 {
    const AIRE: f32 = 12.0;
    let x_centrada = (zona.center().x - tarjeta.x / 2.0).clamp(
        pantalla.left() + AIRE,
        (pantalla.right() - tarjeta.x - AIRE).max(pantalla.left() + AIRE),
    );
    let candidatas = [
        egui::pos2(x_centrada, zona.bottom() + AIRE),
        egui::pos2(x_centrada, zona.top() - AIRE - tarjeta.y),
        egui::pos2(zona.right() + AIRE, zona.center().y - tarjeta.y / 2.0),
        egui::pos2(
            zona.left() - AIRE - tarjeta.x,
            zona.center().y - tarjeta.y / 2.0,
        ),
    ];
    let cabe = |pos: egui::Pos2| pantalla.contains_rect(egui::Rect::from_min_size(pos, tarjeta));
    let elegida = candidatas
        .into_iter()
        .find(|pos| cabe(*pos))
        .unwrap_or_else(|| {
            // Zona enorme (un panel entero): la tarjeta va dentro, abajo.
            egui::pos2(x_centrada, zona.bottom() - tarjeta.y - AIRE)
        });
    egui::pos2(
        elegida.x.clamp(
            pantalla.left() + AIRE,
            (pantalla.right() - tarjeta.x - AIRE).max(pantalla.left()),
        ),
        elegida.y.clamp(
            pantalla.top() + AIRE,
            (pantalla.bottom() - tarjeta.y - AIRE).max(pantalla.top()),
        ),
    )
}

impl NovaCutWindows {
    fn condicion_cumplida(&self, condicion: Condicion) -> bool {
        match condicion {
            Condicion::HayClips => !self.project.clips.is_empty(),
            Condicion::FuenteAbierta => self.source_monitor.is_some(),
            Condicion::ClipSeleccionado => self.selected.is_some(),
            Condicion::HaReproducido => self.tutorial.reproducido,
            Condicion::HayTexto => self.project.clips.iter().any(|clip| clip.title.is_some()),
        }
    }

    /// Dibuja el paso en curso: velo con hueco sobre la zona y la tarjeta.
    pub(super) fn mostrar_tutorial(&mut self, context: &egui::Context) {
        if !self.tutorial.activo {
            return;
        }
        if self.playback.is_some() {
            self.tutorial.reproducido = true;
        }
        // Un diálogo que pide decidir algo va primero: el recorrido espera
        // en vez de taparlo.
        if self.mejoras.hay_propuesta()
            || self.pending_recovery.is_some()
            || self.pending_document_action.is_some()
        {
            return;
        }
        if context.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.terminar_tutorial();
            return;
        }
        let Some(paso) = PASOS.get(self.tutorial.paso) else {
            self.terminar_tutorial();
            return;
        };
        let pantalla = context.screen_rect();
        let zona = paso
            .zona
            .and_then(|nombre| self.tutorial.zonas.get(nombre).copied())
            .filter(|rect| rect.is_positive() && pantalla.intersects(*rect))
            .map(|rect| rect.expand(6.0).intersect(pantalla));
        let hecho = paso
            .accion
            .map(|(_, condicion)| self.condicion_cumplida(condicion));
        // Hecho el paso, fuera el velo: deja ver lo que se acaba de abrir
        // (la vista previa, el texto…) en vez de oscurecerlo.
        let con_velo = hecho != Some(true);
        // Hecho el paso, la tarjeta se aparta a la esquina para dejar ver
        // su resultado (la vista previa, el texto en el monitor…).
        let apartar = hecho == Some(true);
        // Velo: cuatro bandas alrededor de la zona, solo pintadas.
        let velo = context.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("tutorial-velo"),
        ));
        let sombra = egui::Color32::from_black_alpha(150);
        match zona {
            Some(hueco) => {
                for banda in if con_velo {
                    vec![
                        egui::Rect::from_min_max(
                            pantalla.min,
                            egui::pos2(pantalla.right(), hueco.top()),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(pantalla.left(), hueco.bottom()),
                            pantalla.max,
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(pantalla.left(), hueco.top()),
                            egui::pos2(hueco.left(), hueco.bottom()),
                        ),
                        egui::Rect::from_min_max(
                            egui::pos2(hueco.right(), hueco.top()),
                            egui::pos2(pantalla.right(), hueco.bottom()),
                        ),
                    ]
                } else {
                    Vec::new()
                } {
                    if banda.is_positive() {
                        velo.rect_filled(banda, 0.0, sombra);
                    }
                }
                velo.rect_stroke(
                    hueco,
                    6.0,
                    egui::Stroke::new(2.5_f32, theme::ACCENT),
                    egui::StrokeKind::Outside,
                );
            }
            None => {
                velo.rect_filled(pantalla, 0.0, sombra);
            }
        }
        let tamano = egui::vec2(380.0_f32.min(pantalla.width() - 24.0), 0.0);
        let alto_previsto = egui::vec2(tamano.x, 230.0);
        let posicion = match zona {
            _ if apartar => pantalla.right_bottom() - alto_previsto - egui::vec2(16.0, 40.0),
            Some(hueco) => colocar_tarjeta(hueco, alto_previsto, pantalla),
            None => pantalla.center() - alto_previsto / 2.0,
        };
        let total = PASOS.len();
        let indice = self.tutorial.paso;
        let mut ir: Option<isize> = None;
        let mut cerrar = false;
        egui::Area::new(egui::Id::new("tutorial-tarjeta"))
            .order(egui::Order::Tooltip)
            .fixed_pos(posicion)
            .show(context, |ui| {
                egui::Frame::new()
                    .fill(theme::CARD)
                    .stroke(egui::Stroke::new(1.5_f32, theme::ACCENT))
                    .corner_radius(8.0)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(tamano.x - 28.0);
                        if indice > 0 {
                            ui.label(
                                egui::RichText::new(format!("Paso {indice} de {}", total - 1))
                                    .size(11.0)
                                    .color(theme::TEXT_FAINT),
                            );
                        }
                        ui.label(egui::RichText::new(paso.titulo).strong().size(15.0).color(theme::TEXT));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(paso.texto).size(12.5).color(theme::TEXT_DIM));
                        if let (Some((pide, _)), Some(hecho)) = (paso.accion, hecho) {
                            ui.add_space(6.0);
                            if hecho {
                                ui.label(egui::RichText::new(format!("✔ {pide}: hecho")).strong().color(theme::OK));
                            } else {
                                ui.label(
                                    egui::RichText::new(format!("👉 Pruébalo: {pide}"))
                                        .strong()
                                        .color(theme::ACCENT),
                                );
                            }
                        }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if indice == 0 {
                                if theme::accent_button(ui, "Empezar el recorrido").clicked() {
                                    ir = Some(1);
                                }
                                if ui.button("Ahora no").on_hover_text("Puedes verlo luego desde «?»").clicked() {
                                    cerrar = true;
                                }
                                return;
                            }
                            if ui.button("← Atrás").clicked() {
                                ir = Some(-1);
                            }
                            let ultimo = indice + 1 == total;
                            let texto = if ultimo {
                                "Terminar"
                            } else if hecho == Some(false) {
                                "Saltar este paso →"
                            } else {
                                "Siguiente →"
                            };
                            let boton = if hecho == Some(false) {
                                ui.button(texto)
                            } else {
                                theme::accent_button(ui, texto)
                            };
                            if boton.clicked() {
                                if ultimo {
                                    cerrar = true;
                                } else {
                                    ir = Some(1);
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .small_button("Cerrar")
                                    .on_hover_text("Termina el recorrido (Esc). Puedes repetirlo desde «?»")
                                    .clicked()
                                {
                                    cerrar = true;
                                }
                            });
                        });
                    });
            });
        if cerrar {
            self.terminar_tutorial();
        } else if let Some(delta) = ir {
            // Salir del paso de la vista previa la cierra: tapaba la línea
            // de tiempo del paso siguiente.
            if paso.accion.map(|(_, condicion)| condicion) == Some(Condicion::FuenteAbierta) {
                self.source_monitor = None;
            }
            self.tutorial.paso =
                (self.tutorial.paso as isize + delta).clamp(0, total as isize - 1) as usize;
            match PASOS[self.tutorial.paso].accion.map(|(_, c)| c) {
                Some(Condicion::HaReproducido) => self.tutorial.reproducido = false,
                // Al importar, el clip ya queda seleccionado: se suelta
                // para que el paso se haga con el ratón de verdad.
                Some(Condicion::ClipSeleccionado) => self.clear_selection(),
                _ => {}
            }
        }
        context.request_repaint();
    }

    pub(super) fn terminar_tutorial(&mut self) {
        self.tutorial.activo = false;
        self.tutorial_visto = true;
        self.tutorial.zonas.clear();
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn la_tarjeta_nunca_se_sale_de_la_pantalla() {
        let pantalla = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 640.0));
        let tarjeta = egui::vec2(380.0, 230.0);
        for zona in [
            // Botón arriba a la izquierda, timeline abajo, panel derecho entero.
            egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(80.0, 24.0)),
            egui::Rect::from_min_size(egui::pos2(340.0, 500.0), egui::vec2(800.0, 130.0)),
            egui::Rect::from_min_size(egui::pos2(900.0, 0.0), egui::vec2(380.0, 640.0)),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 640.0)),
        ] {
            let pos = colocar_tarjeta(zona, tarjeta, pantalla);
            let rect = egui::Rect::from_min_size(pos, tarjeta);
            assert!(pantalla.contains_rect(rect), "{zona:?} → {rect:?}");
        }
    }

    #[test]
    fn la_tarjeta_no_tapa_un_boton_pequeno() {
        let pantalla = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 640.0));
        let boton = egui::Rect::from_min_size(egui::pos2(600.0, 70.0), egui::vec2(90.0, 26.0));
        let pos = colocar_tarjeta(boton, egui::vec2(380.0, 230.0), pantalla);
        assert!(!egui::Rect::from_min_size(pos, egui::vec2(380.0, 230.0)).intersects(boton));
    }

    #[test]
    fn cada_paso_con_zona_tiene_nombre_y_el_primero_es_la_bienvenida() {
        assert!(PASOS[0].zona.is_none());
        let zonas: Vec<_> = PASOS.iter().filter_map(|paso| paso.zona).collect();
        for zona in [
            "importar",
            "medios",
            "timeline",
            "herramientas",
            "monitor",
            "texto",
            "inspector",
            "menu_herramientas",
            "exportar",
            "ayuda",
        ] {
            assert!(zonas.contains(&zona), "falta {zona}");
        }
    }
}
