//! Primitivas de montaje sobre tiempos de timeline: partir, mover bordes,
//! rodar, deslizar (slip/slide), levantar/extraer y la ventana de render
//! que permite reproducir o exportar desde un instante sin componer lo
//! anterior.
//!
//! Todas respetan velocidad, clips invertidos, keyframes de transformación y
//! la banda de volumen, y todas son lógica pura: el host valida bloqueos,
//! registra el deshacer y refresca el monitor.

use super::{efectos, RoughClip, TransformKeyframe};

/// Margen mínimo de un clip tras cualquier edición (un fotograma a 25 fps).
pub const MIN_CLIP: f64 = 0.04;

/// Secuencias anidadas y rampas no se pueden cortar por dentro: su tiempo de
/// origen no es lineal con el de timeline.
pub fn is_complex(clip: &RoughClip) -> bool {
    clip.nested.is_some()
        || clip
            .speed_ramp
            .as_ref()
            .is_some_and(|points| !points.is_empty())
}

fn speed(clip: &RoughClip) -> f64 {
    clip.speed.clamp(0.1, 8.0)
}

/// Último segundo de origen utilizable. Sin duración conocida (títulos,
/// capas de ajuste, imágenes) el material no se acaba.
fn source_ceiling(clip: &RoughClip) -> f64 {
    clip.source_duration_seconds
        .map(|duration| duration.max(clip.out_seconds))
        .unwrap_or(f64::INFINITY)
}

/// Cuánto puede moverse cada borde sin inventar material. Devuelve
/// `(cabeza_min, cabeza_max, cola_min, cola_max)` en segundos de timeline:
/// un delta de cabeza positivo recorta (el clip empieza más tarde) y uno de
/// cola positivo alarga.
pub fn edge_limits(clip: &RoughClip) -> (f64, f64, f64, f64) {
    let speed = speed(clip);
    let duration = clip.duration();
    let ceiling = source_ceiling(clip);
    // Material de reserva antes de la cabeza y después de la cola, en el
    // sentido de reproducción.
    let (before_head, after_tail) = if clip.freeze_at.is_some() {
        (f64::INFINITY, f64::INFINITY)
    } else if clip.fx.reverse {
        (ceiling - clip.out_seconds, clip.in_seconds)
    } else {
        (clip.in_seconds, ceiling - clip.out_seconds)
    };
    let room = (duration - MIN_CLIP).max(0.0);
    (
        -(before_head.max(0.0) / speed).min(clip.timeline_start),
        room,
        -room,
        after_tail.max(0.0) / speed,
    )
}

/// Mueve los bordes del clip en tiempo de timeline sin desplazar su
/// contenido: `head` > 0 recorta la cabeza (el clip empieza más tarde),
/// `tail` > 0 alarga la cola. Los keyframes y la banda de volumen siguen
/// anclados al mismo fotograma de origen. Devuelve `None` si el clip no
/// admite la edición o se quedaría sin material.
pub fn retime(clip: &RoughClip, head: f64, tail: f64) -> Option<RoughClip> {
    if is_complex(clip) {
        return None;
    }
    let (head_min, head_max, tail_min, tail_max) = edge_limits(clip);
    let eps = 1e-6;
    if head < head_min - eps
        || head > head_max + eps
        || tail < tail_min - eps
        || tail > tail_max + eps
        || clip.duration() - head + tail < MIN_CLIP - eps
    {
        return None;
    }
    let speed = speed(clip);
    let mut out = clip.clone();
    if clip.fx.reverse {
        out.out_seconds -= head * speed;
        out.in_seconds -= tail * speed;
    } else {
        out.in_seconds += head * speed;
        out.out_seconds += tail * speed;
    }
    out.in_seconds = out.in_seconds.max(0.0);
    out.timeline_start = clip.timeline_start + head;
    if head.abs() > eps {
        shift_local_time(&mut out, clip, head);
    }
    Some(out)
}

/// Reancla keyframes y banda de volumen cuando la cabeza se mueve `head`
/// segundos: el valor en el nuevo inicio se conserva como keyframe 0.
fn shift_local_time(out: &mut RoughClip, original: &RoughClip, head: f64) {
    let new_duration = out.duration();
    if let Some(keyframes) = &original.keyframes {
        if !keyframes.is_empty() {
            let (x, y, scale, opacity) = original.evaluate_transform(head.max(0.0));
            let mut shifted: Vec<TransformKeyframe> = keyframes
                .iter()
                .map(|keyframe| {
                    let mut keyframe = *keyframe;
                    keyframe.t -= head;
                    keyframe
                })
                .filter(|keyframe| keyframe.t > 0.001 && keyframe.t <= new_duration + 0.001)
                .collect();
            if head > 0.0 || keyframes.iter().all(|keyframe| keyframe.t - head > 0.001) {
                shifted.insert(
                    0,
                    TransformKeyframe {
                        t: 0.0,
                        x,
                        y,
                        scale,
                        opacity,
                    },
                );
            }
            out.keyframes = Some(shifted);
        }
    }
    if !original.fx.volume_keys.is_empty() {
        let level = original.fx.volume_at(head.max(0.0));
        let mut keys: Vec<efectos::VolumeKey> = original
            .fx
            .volume_keys
            .iter()
            .map(|key| efectos::VolumeKey {
                t: key.t - head,
                db: key.db,
            })
            .filter(|key| key.t > 0.001 && key.t <= new_duration + 0.001)
            .collect();
        keys.insert(0, efectos::VolumeKey { t: 0.0, db: level });
        out.fx.volume_keys = keys;
    }
}

/// El tramo `[from, to]` (tiempo local de timeline) de un clip como clip
/// independiente, respetando velocidad, clips invertidos, keyframes y la
/// banda de volumen. Su `timeline_start` queda en el inicio del tramo. Los
/// fundidos y la transición de entrada solo sobreviven en el tramo que
/// conserva ese borde.
pub fn clip_portion(clip: &RoughClip, from: f64, to: f64) -> Option<RoughClip> {
    let duration = clip.duration();
    let from = from.clamp(0.0, duration);
    let to = to.clamp(from, duration);
    if to - from < 0.001 || is_complex(clip) {
        return None;
    }
    let mut part = retime(clip, from, to - duration).or_else(|| {
        // Tramos más cortos que MIN_CLIP (restos de un corte) siguen siendo
        // válidos aquí; retime solo protege las ediciones interactivas.
        let mut part = clip.clone();
        let speed = speed(clip);
        if clip.fx.reverse {
            part.in_seconds = clip.out_seconds - to * speed;
            part.out_seconds = clip.out_seconds - from * speed;
        } else {
            part.in_seconds = clip.in_seconds + from * speed;
            part.out_seconds = clip.in_seconds + to * speed;
        }
        part.timeline_start = clip.timeline_start + from;
        if from > 0.0 {
            shift_local_time(&mut part, clip, from);
        }
        Some(part)
    })?;
    if from > 0.001 {
        part.fade_in_seconds = 0.0;
        part.transition = None;
    }
    if to < duration - 0.001 {
        part.fade_out_seconds = 0.0;
    }
    Some(part)
}

/// Parte el clip en `local` segundos desde su inicio. Ninguna mitad puede
/// quedar por debajo de `MIN_CLIP`.
pub fn split(clip: &RoughClip, local: f64) -> Option<(RoughClip, RoughClip)> {
    let duration = clip.duration();
    if local < MIN_CLIP || local > duration - MIN_CLIP {
        return None;
    }
    Some((
        clip_portion(clip, 0.0, local)?,
        clip_portion(clip, local, duration)?,
    ))
}

fn same_lane(a: &RoughClip, b: &RoughClip) -> bool {
    a.track == b.track && a.has_video == b.has_video
}

fn end_of(clip: &RoughClip) -> f64 {
    clip.timeline_start + clip.duration()
}

/// Índices `(izquierdo, derecho)` de dos clips contiguos del mismo carril
/// cuyo corte está a menos de `tolerance` de `time`.
pub fn cut_at(clips: &[RoughClip], lane_of: usize, time: f64, tolerance: f64) -> Option<(usize, usize)> {
    let lane = clips.get(lane_of)?;
    let mut best: Option<(usize, usize, f64)> = None;
    for (left_index, left) in clips.iter().enumerate() {
        if !same_lane(left, lane) {
            continue;
        }
        let cut = end_of(left);
        if (cut - time).abs() > tolerance {
            continue;
        }
        for (right_index, right) in clips.iter().enumerate() {
            if right_index == left_index || !same_lane(right, lane) {
                continue;
            }
            let gap = (right.timeline_start - cut).abs();
            if gap < 0.002 && best.is_none_or(|(_, _, distance)| (cut - time).abs() < distance) {
                best = Some((left_index, right_index, (cut - time).abs()));
            }
        }
    }
    best.map(|(left, right, _)| (left, right))
}

/// Edición de rodar (Premiere «Rodar», N): el corte entre `left` y `right`
/// se mueve `delta` segundos; uno se alarga lo que el otro se acorta y
/// nada más del montaje cambia. Se limita al material disponible y devuelve
/// el delta aplicado.
pub fn roll(clips: &mut [RoughClip], left: usize, right: usize, delta: f64) -> Option<f64> {
    let (_, _, left_min, left_max) = edge_limits(&clips[left]);
    let (right_min, right_max, _, _) = edge_limits(&clips[right]);
    let delta = delta.clamp(left_min.max(right_min), left_max.min(right_max));
    if delta.abs() < 1e-6 {
        return Some(0.0);
    }
    let new_left = retime(&clips[left], 0.0, delta)?;
    let new_right = retime(&clips[right], delta, 0.0)?;
    clips[left] = new_left;
    clips[right] = new_right;
    Some(delta)
}

/// Desplazar contenido (Premiere «Desplazar», Y): el clip se queda en su
/// sitio con la misma duración pero muestra otro tramo del medio. `delta`
/// > 0 enseña material posterior. Devuelve el delta aplicado.
pub fn slip(clip: &mut RoughClip, delta: f64) -> Option<f64> {
    if is_complex(clip) || clip.freeze_at.is_some() {
        return None;
    }
    let speed = speed(clip);
    let ceiling = source_ceiling(clip);
    // En segundos de timeline, cuánto material hay antes y después.
    let (before, after) = if clip.fx.reverse {
        (ceiling - clip.out_seconds, clip.in_seconds)
    } else {
        (clip.in_seconds, ceiling - clip.out_seconds)
    };
    let delta = delta.clamp(-before / speed, after / speed);
    let source = delta * speed;
    if clip.fx.reverse {
        clip.in_seconds -= source;
        clip.out_seconds -= source;
    } else {
        clip.in_seconds += source;
        clip.out_seconds += source;
    }
    clip.in_seconds = clip.in_seconds.max(0.0);
    Some(delta)
}

/// Deslizar (Premiere «Deslizar», U): el clip conserva su contenido y se
/// mueve `delta` segundos entre sus vecinos del carril, que ceden o ganan
/// ese tiempo. Sin vecino en un lado, ese lado queda como hueco. Devuelve el
/// delta aplicado.
pub fn slide(clips: &mut [RoughClip], index: usize, delta: f64) -> Option<f64> {
    if is_complex(&clips[index]) {
        return None;
    }
    let start = clips[index].timeline_start;
    let end = end_of(&clips[index]);
    let neighbour = |edge: f64, before: bool| {
        clips.iter().enumerate().position(|(other, clip)| {
            other != index
                && same_lane(clip, &clips[index])
                && if before {
                    (end_of(clip) - edge).abs() < 0.002
                } else {
                    (clip.timeline_start - edge).abs() < 0.002
                }
        })
    };
    let left = neighbour(start, true);
    let right = neighbour(end, false);
    let mut low = -start;
    let mut high = f64::INFINITY;
    if let Some(left) = left {
        let (_, _, tail_min, tail_max) = edge_limits(&clips[left]);
        low = low.max(tail_min);
        high = high.min(tail_max);
    } else {
        // Sin vecino a la izquierda no puede invadir otro clip previo.
        let previous_end = clips
            .iter()
            .enumerate()
            .filter(|(other, clip)| {
                *other != index && same_lane(clip, &clips[index]) && end_of(clip) <= start + 0.002
            })
            .map(|(_, clip)| end_of(clip))
            .fold(0.0, f64::max);
        low = low.max(previous_end - start);
    }
    if let Some(right) = right {
        let (head_min, head_max, _, _) = edge_limits(&clips[right]);
        low = low.max(head_min);
        high = high.min(head_max);
    } else {
        let next_start = clips
            .iter()
            .enumerate()
            .filter(|(other, clip)| {
                *other != index && same_lane(clip, &clips[index]) && clip.timeline_start >= end - 0.002
            })
            .map(|(_, clip)| clip.timeline_start)
            .fold(f64::INFINITY, f64::min);
        high = high.min(next_start - end);
    }
    if low > high {
        return None;
    }
    let delta = delta.clamp(low, high);
    if delta.abs() < 1e-6 {
        return Some(0.0);
    }
    if let Some(left) = left {
        clips[left] = retime(&clips[left], 0.0, delta)?;
    }
    if let Some(right) = right {
        clips[right] = retime(&clips[right], delta, 0.0)?;
    }
    clips[index].timeline_start += delta;
    Some(delta)
}

/// Levantar (Premiere «Levantar», `;`): quita `[start, end]` de los clips
/// que `editable` acepta y deja el hueco. Falla sin tocar nada si el rango
/// corta por dentro una secuencia anidada o una rampa.
pub fn lift(
    clips: &[RoughClip],
    start: f64,
    end: f64,
    editable: impl Fn(&RoughClip) -> bool,
) -> Result<Vec<RoughClip>, String> {
    let mut out = Vec::with_capacity(clips.len() + 1);
    for clip in clips {
        let clip_start = clip.timeline_start;
        let clip_end = end_of(clip);
        if !editable(clip) || clip_end <= start + 0.001 || clip_start >= end - 0.001 {
            out.push(clip.clone());
            continue;
        }
        let inside = clip_start >= start - 0.001 && clip_end <= end + 0.001;
        if inside {
            continue;
        }
        if is_complex(clip) {
            return Err(format!(
                "El rango corta «{}», que es una secuencia anidada o tiene rampa de velocidad",
                clip.name()
            ));
        }
        if let Some(head) = clip_portion(clip, 0.0, start - clip_start) {
            out.push(head);
        }
        if let Some(tail) = clip_portion(clip, end - clip_start, clip.duration()) {
            out.push(tail);
        }
    }
    Ok(out)
}

/// Instante desde el que hay que componer para que `time` salga idéntico al
/// montaje completo. Normalmente es `time`; retrocede cuando recortar la
/// cabeza de un clip cambiaría lo que se ve: secuencias anidadas y rampas
/// (no se cortan por dentro), fundidos de entrada a medias, fundidos de
/// salida que el render limitaría a la mitad del clip recortado y
/// transiciones, que dependen de la duración de ambos clips.
pub fn window_start(clips: &[RoughClip], time: f64) -> f64 {
    let mut start = time.max(0.0);
    loop {
        let mut next = start;
        for clip in clips {
            let clip_start = clip.timeline_start;
            let clip_end = end_of(clip);
            if clip.transition.is_some() {
                let d = clip.transition_duration.max(0.04);
                if start > clip_start - 2.0 * d + 1e-9 && start < clip_start + d + 1e-9 {
                    next = next.min(clip_start - 2.0 * d);
                }
            }
            if clip_start >= start - 1e-9 || clip_end <= start + 1e-9 {
                continue;
            }
            let local = start - clip_start;
            let (fade_in, fade_out) = clip.effective_fades();
            let duration = clip.duration();
            if is_complex(clip) || (fade_in > 0.0 && local < fade_in + 1e-6) {
                next = next.min(clip_start);
            } else if fade_out > 0.0 && local > duration - 2.0 * fade_out - 1e-6 {
                next = next.min(clip_end - 2.0 * fade_out);
            }
        }
        let next = next.max(0.0);
        if next >= start - 1e-9 {
            return start;
        }
        start = next;
    }
}

/// Clips que alcanzan `[start, end)` desplazados para que `start` sea el
/// cero, más el preroll: los segundos iniciales del resultado que hay que
/// descartar porque `window_start` tuvo que retroceder. Las colas no se
/// recortan (el render corta por duración de salida) para no alterar
/// fundidos ni transiciones. Compuesto desde el preroll, el resultado es
/// idéntico al del montaje completo.
pub fn window(clips: &[RoughClip], start: f64, end: f64) -> (Vec<RoughClip>, f64) {
    let origin = window_start(clips, start);
    let mut out = Vec::new();
    for clip in clips {
        let clip_start = clip.timeline_start;
        let clip_end = end_of(clip);
        if clip_end <= origin + 0.001 || clip_start >= end - 0.001 {
            continue;
        }
        let part = if is_complex(clip) || clip_start >= origin - 0.001 {
            Some(clip.clone())
        } else {
            clip_portion(clip, origin - clip_start, clip.duration())
        };
        if let Some(mut part) = part {
            part.timeline_start = (part.timeline_start - origin).max(0.0);
            out.push(part);
        }
    }
    (out, start - origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::efectos::VolumeKey;

    fn media(timeline_start: f64, in_s: f64, out_s: f64) -> RoughClip {
        RoughClip {
            in_seconds: in_s,
            out_seconds: out_s,
            timeline_start,
            source_duration_seconds: Some(20.0),
            ..Default::default()
        }
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn split_reversed_clip_keeps_playback_order() {
        let mut clip = media(0.0, 2.0, 10.0);
        clip.fx.reverse = true;
        let (left, right) = split(&clip, 3.0).unwrap();
        // Invertido: los 3 primeros segundos en pantalla son el final del medio.
        assert!(approx(left.in_seconds, 7.0) && approx(left.out_seconds, 10.0));
        assert!(approx(right.in_seconds, 2.0) && approx(right.out_seconds, 7.0));
        assert!(approx(right.timeline_start, 3.0));
    }

    #[test]
    fn split_moves_fades_transition_and_keyframes_to_the_right_half() {
        let mut clip = media(5.0, 0.0, 8.0);
        clip.fade_in_seconds = 1.0;
        clip.fade_out_seconds = 1.0;
        clip.transition = Some("dissolve".into());
        clip.keyframes = Some(vec![
            TransformKeyframe { t: 0.0, x: 0.0, y: 0.0, scale: 100.0, opacity: 100.0 },
            TransformKeyframe { t: 8.0, x: 800.0, y: 0.0, scale: 100.0, opacity: 100.0 },
        ]);
        clip.fx.volume_keys = vec![VolumeKey { t: 0.0, db: 0.0 }, VolumeKey { t: 8.0, db: -16.0 }];
        let (left, right) = split(&clip, 2.0).unwrap();
        assert_eq!((left.fade_in_seconds, left.fade_out_seconds), (1.0, 0.0));
        assert_eq!((right.fade_in_seconds, right.fade_out_seconds), (0.0, 1.0));
        assert!(left.transition.is_some() && right.transition.is_none());
        // La animación sigue en el mismo sitio de la pantalla.
        let (x, ..) = right.evaluate_transform(0.0);
        assert!(approx(x, 200.0));
        let (x, ..) = right.evaluate_transform(6.0);
        assert!(approx(x, 800.0));
        assert!(approx(right.fx.volume_at(0.0), -4.0));
        assert!(approx(right.fx.volume_at(6.0), -16.0));
    }

    #[test]
    fn split_refuses_edges_and_complex_clips() {
        let clip = media(0.0, 0.0, 4.0);
        assert!(split(&clip, 0.01).is_none());
        assert!(split(&clip, 3.99).is_none());
        let mut nested = clip.clone();
        nested.nested = Some(vec![clip]);
        assert!(split(&nested, 2.0).is_none());
    }

    #[test]
    fn retime_respects_speed_reverse_and_media_limits() {
        let mut clip = media(4.0, 2.0, 10.0);
        clip.speed = 2.0;
        // Cabeza: hay 2 s de medio antes = 1 s de timeline a 2x.
        assert!(retime(&clip, -1.5, 0.0).is_none());
        let longer = retime(&clip, -1.0, 0.0).unwrap();
        assert!(approx(longer.in_seconds, 0.0) && approx(longer.timeline_start, 3.0));
        // Cola: quedan 10 s de medio = 5 s de timeline.
        let longer = retime(&clip, 0.0, 5.0).unwrap();
        assert!(approx(longer.out_seconds, 20.0));
        assert!(retime(&clip, 0.0, 5.1).is_none());
        clip.fx.reverse = true;
        // Invertido, alargar la cola consume material del principio del medio.
        let longer = retime(&clip, 0.0, 1.0).unwrap();
        assert!(approx(longer.in_seconds, 0.0) && approx(longer.out_seconds, 10.0));
        let shorter = retime(&clip, 1.0, 0.0).unwrap();
        assert!(approx(shorter.out_seconds, 8.0) && approx(shorter.timeline_start, 5.0));
    }

    #[test]
    fn retime_extending_head_keeps_animation_on_the_same_frames() {
        let mut clip = media(4.0, 4.0, 10.0);
        clip.keyframes = Some(vec![
            TransformKeyframe { t: 0.0, x: 0.0, y: 0.0, scale: 100.0, opacity: 100.0 },
            TransformKeyframe { t: 2.0, x: 100.0, y: 0.0, scale: 100.0, opacity: 100.0 },
        ]);
        let longer = retime(&clip, -2.0, 0.0).unwrap();
        let keys = longer.keyframes.unwrap();
        assert_eq!(keys.len(), 3);
        assert!(approx(keys[1].t, 2.0) && approx(keys[2].t, 4.0));
    }

    #[test]
    fn roll_moves_only_the_cut() {
        let mut clips = vec![media(0.0, 0.0, 5.0), media(5.0, 5.0, 10.0), media(10.0, 0.0, 3.0)];
        assert_eq!(cut_at(&clips, 0, 5.01, 0.05), Some((0, 1)));
        let applied = roll(&mut clips, 0, 1, 1.5).unwrap();
        assert!(approx(applied, 1.5));
        assert!(approx(clips[0].out_seconds, 6.5));
        assert!(approx(clips[1].in_seconds, 6.5) && approx(clips[1].timeline_start, 6.5));
        assert!(approx(clips[2].timeline_start, 10.0), "lo demás no se mueve");
        // Limitado por el material que le queda al de la izquierda.
        let applied = roll(&mut clips, 0, 1, 100.0).unwrap();
        assert!(approx(clips[1].duration(), MIN_CLIP));
        assert!(applied > 0.0);
    }

    #[test]
    fn slip_changes_content_not_position() {
        let mut clip = media(3.0, 4.0, 6.0);
        assert!(approx(slip(&mut clip, 100.0).unwrap(), 14.0));
        assert!(approx(clip.in_seconds, 18.0) && approx(clip.out_seconds, 20.0));
        assert!(approx(clip.timeline_start, 3.0));
        assert!(approx(slip(&mut clip, -100.0).unwrap(), -18.0));
        assert!(approx(clip.in_seconds, 0.0));
    }

    #[test]
    fn slide_trades_time_with_neighbours() {
        let mut clips = vec![media(0.0, 0.0, 4.0), media(4.0, 0.0, 2.0), media(6.0, 6.0, 10.0)];
        let applied = slide(&mut clips, 1, 1.0).unwrap();
        assert!(approx(applied, 1.0));
        assert!(approx(clips[0].out_seconds, 5.0));
        assert!(approx(clips[1].timeline_start, 5.0) && approx(clips[1].in_seconds, 0.0));
        assert!(approx(clips[2].timeline_start, 7.0) && approx(clips[2].in_seconds, 7.0));
        assert!(approx(end_of(&clips[2]), 10.0), "el final del montaje no cambia");
    }

    #[test]
    fn lift_leaves_gap_and_skips_locked() {
        let clips = vec![media(0.0, 0.0, 10.0), {
            let mut audio = media(0.0, 0.0, 10.0);
            audio.has_video = false;
            audio
        }];
        let lifted = lift(&clips, 2.0, 5.0, |clip| clip.has_video).unwrap();
        assert_eq!(lifted.len(), 3);
        assert!(approx(lifted[1].timeline_start, 5.0) && approx(lifted[1].in_seconds, 5.0));
        assert!(approx(lifted[2].duration(), 10.0), "la pista bloqueada no se toca");
    }

    #[test]
    fn window_starts_at_the_playhead_when_nothing_depends_on_the_past() {
        let clips = vec![media(0.0, 0.0, 10.0), media(10.0, 0.0, 10.0)];
        let (windowed, preroll) = window(&clips, 12.0, 20.0);
        assert_eq!(preroll, 0.0);
        assert_eq!(windowed.len(), 1);
        assert!(approx(windowed[0].timeline_start, 0.0) && approx(windowed[0].in_seconds, 2.0));
    }

    #[test]
    fn window_backs_up_for_fades_transitions_and_nested() {
        let mut fading = media(10.0, 0.0, 10.0);
        fading.fade_in_seconds = 2.0;
        assert!(approx(window_start(&[fading.clone()], 11.0), 10.0));
        assert!(approx(window_start(&[fading], 13.0), 13.0));
        let before = media(0.0, 0.0, 10.0);
        let mut after = media(10.0, 0.0, 10.0);
        after.transition = Some("dissolve".into());
        after.transition_duration = 1.0;
        let clips = vec![before, after];
        // En mitad de la transición: hace falta el saliente con margen.
        let (windowed, preroll) = window(&clips, 10.5, 20.0);
        assert!(approx(preroll, 2.5));
        assert_eq!(windowed.len(), 2);
        let mut ending = media(0.0, 0.0, 10.0);
        ending.fade_out_seconds = 1.0;
        // Recortar la cabeza no puede dejar el clip por debajo de 2× el fundido.
        assert!(approx(window_start(&[ending.clone()], 9.5), 8.0));
        assert!(approx(window_start(&[ending], 5.0), 5.0));
        let mut nested = media(5.0, 0.0, 4.0);
        nested.nested = Some(vec![media(0.0, 0.0, 4.0)]);
        let (windowed, preroll) = window(&[nested], 7.0, 9.0);
        assert!(approx(preroll, 2.0) && approx(windowed[0].timeline_start, 0.0));
    }
}
