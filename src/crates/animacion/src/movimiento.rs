// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Free motion: the keys an AI writes on a clip's framing - where it is,
//! how big, how turned, how see-through - checked before they land, and
//! the named moves an editor reaches for (a pop, a shake, a pulse, a slide
//! in, a spin in, a slow drift) said as those keys. Every value is the
//! property's own, the number the inspector shows: scale a multiplier of
//! the fitted size, x and y a fraction of the frame from centred, the turn
//! in clockwise degrees, opacity `0..=1`.

use concat_project::Command;
use concat_project::model::{Clip, KeyEase, KeyProperty};

use crate::llaves;
use crate::tiempo::{en_clip, tramo};

/// How a key is reached from the one before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curva {
    /// At an even pace.
    Lineal,
    /// Slow at first, fast at the end.
    Acelerar,
    /// Fast at first, slow at the end.
    Frenar,
    /// Slow at both ends.
    Suave,
    /// Past the value and back: a bounce into place.
    Rebote,
    /// A cubic bezier's two control points, `[x1, y1, x2, y2]`, as CSS.
    Propia([f64; 4]),
}

impl Curva {
    /// Every named curve, for a tool's schema.
    pub const NOMBRES: [&'static str; 5] = ["lineal", "acelerar", "frenar", "suave", "rebote"];

    /// The curve a word names.
    pub fn por_nombre(nombre: &str) -> Option<Curva> {
        Some(match nombre {
            "lineal" => Curva::Lineal,
            "acelerar" => Curva::Acelerar,
            "frenar" => Curva::Frenar,
            "suave" => Curva::Suave,
            "rebote" => Curva::Rebote,
            _ => return None,
        })
    }

    /// The key's ease.
    pub fn ease(self) -> KeyEase {
        match self {
            Curva::Lineal => KeyEase::LINEAR,
            Curva::Acelerar => KeyEase::IN,
            Curva::Frenar => KeyEase::OUT,
            Curva::Suave => KeyEase::IN_OUT,
            // easeOutBack: about a tenth past the value, then back.
            Curva::Rebote => KeyEase([0.34, 1.56, 0.64, 1.0]),
            Curva::Propia(puntos) => KeyEase(puntos).sane(),
        }
    }
}

/// The properties free motion keys, by the names a tool takes.
pub const PROPIEDADES: [(&str, KeyProperty); 5] = [
    ("escala", KeyProperty::Scale),
    ("x", KeyProperty::OffsetX),
    ("y", KeyProperty::OffsetY),
    ("giro", KeyProperty::Rotation),
    ("opacidad", KeyProperty::Opacity),
];

/// The property a name says.
pub fn propiedad(nombre: &str) -> Option<KeyProperty> {
    PROPIEDADES.iter().find(|(n, _)| *n == nombre).map(|(_, p)| *p)
}

/// The values a property may take here: wide enough for anything on
/// screen, narrow enough that a slip of the AI's does not throw the
/// picture a mile off.
fn limites(propiedad: KeyProperty) -> (f64, f64) {
    match propiedad {
        KeyProperty::Scale => (0.02, 10.0),
        KeyProperty::OffsetX | KeyProperty::OffsetY => (-3.0, 3.0),
        KeyProperty::Rotation => (-3600.0, 3600.0),
        _ => (0.0, 1.0),
    }
}

/// One key of free motion: at timeline second `t`, `valor`, reached as
/// `curva` says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Punto {
    /// Second of the timeline.
    pub t: f64,
    /// What the property is then.
    pub valor: f64,
    /// How it gets there from the key before.
    pub curva: Curva,
}

/// The commands that key `propiedad` of `clip` through `puntos`, taking
/// off the keys it had on that property between the first and the last.
/// At least two points, every one inside the clip and in the property's
/// range; they may come in any order.
pub fn claves(clip: &Clip, propiedad: KeyProperty, puntos: &[Punto]) -> Result<Vec<Command>, String> {
    if puntos.len() < 2 {
        return Err("hacen falta al menos dos puntos para que algo se mueva".into());
    }
    let mut puntos = puntos.to_vec();
    puntos.sort_by(|a, b| a.t.total_cmp(&b.t));
    let (primero, ultimo) = (puntos[0].t, puntos[puntos.len() - 1].t);
    tramo(clip, primero, ultimo)?;
    let (minimo, maximo) = limites(propiedad);
    if let Some(fuera) = puntos.iter().find(|p| !(minimo..=maximo).contains(&p.valor)) {
        return Err(format!(
            "{} no vale para {}: entre {minimo} y {maximo}",
            fuera.valor,
            propiedad.name()
        ));
    }
    let mut comandos = llaves::quitar(clip, primero, ultimo, &[propiedad]);
    comandos.extend(puntos.iter().map(|p| Command::SetClipKey {
        clip_id: clip.id.clone(),
        property: propiedad,
        at: en_clip(clip, p.t),
        value: p.valor,
        ease: p.curva.ease(),
    }));
    Ok(comandos)
}

/// Where a slide comes in from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lado {
    /// From the left edge.
    Izquierda,
    /// From the right edge.
    Derecha,
    /// From the top.
    Arriba,
    /// From the bottom.
    Abajo,
}

impl Lado {
    /// Every side, for a tool's schema.
    pub const NOMBRES: [&'static str; 4] = ["izquierda", "derecha", "arriba", "abajo"];

    /// The side a word names.
    pub fn por_nombre(nombre: &str) -> Option<Lado> {
        Some(match nombre {
            "izquierda" => Lado::Izquierda,
            "derecha" => Lado::Derecha,
            "arriba" => Lado::Arriba,
            "abajo" => Lado::Abajo,
            _ => return None,
        })
    }
}

/// A named move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Efecto {
    /// Comes in small, grows past its size and settles: a pop.
    Rebote,
    /// Shakes, less and less, and comes to rest.
    Sacudida,
    /// Beats: grows a little and back, twice a second.
    Pulso,
    /// Slides in from off the frame.
    Deslizar(Lado),
    /// Spins in from a quarter turn, growing into place.
    Giro,
    /// A slow push in and drift across the whole stretch: Ken Burns.
    Deriva,
}

impl Efecto {
    /// Every move's name, for a tool's schema.
    pub const NOMBRES: [&'static str; 6] = ["rebote", "sacudida", "pulso", "deslizar", "giro", "deriva"];

    /// The move a word names; a slide comes from `lado`, the left when
    /// none is said.
    pub fn por_nombre(nombre: &str, lado: Option<Lado>) -> Option<Efecto> {
        Some(match nombre {
            "rebote" => Efecto::Rebote,
            "sacudida" => Efecto::Sacudida,
            "pulso" => Efecto::Pulso,
            "deslizar" => Efecto::Deslizar(lado.unwrap_or(Lado::Izquierda)),
            "giro" => Efecto::Giro,
            "deriva" => Efecto::Deriva,
            _ => return None,
        })
    }
}

/// Seconds a way in takes, at most: a third of the stretch when shorter.
const ENTRADA: f64 = 0.5;
/// Seconds between two of a shake's keys: fifteen moves a second.
const PASO_SACUDIDA: f64 = 1.0 / 15.0;
/// Seconds one beat of a pulse takes.
const LATIDO: f64 = 0.5;

/// The commands that play `efecto` on `clip` over the timeline stretch
/// `desde..hasta`, `intensidad` times as strong as by default (`0.2..=3`).
/// The move starts and ends on the clip's own framing, and replaces the
/// keys it had on the properties it moves in that stretch.
pub fn efecto(clip: &Clip, efecto: Efecto, desde: f64, hasta: f64, intensidad: f64) -> Result<Vec<Command>, String> {
    let (desde, hasta) = tramo(clip, desde, hasta)?;
    if !(0.2..=3.0).contains(&intensidad) {
        return Err(format!("una intensidad de {intensidad} no sirve: entre 0.2 y 3"));
    }
    let entra = ENTRADA.min((hasta - desde) / 3.0);
    let pistas: Vec<(KeyProperty, Vec<Punto>)> = match efecto {
        Efecto::Rebote => vec![
            (
                KeyProperty::Scale,
                vec![
                    punto(desde, clip.scale * 0.6, Curva::Lineal),
                    punto(desde + entra, clip.scale, Curva::Rebote),
                ],
            ),
            (
                KeyProperty::Opacity,
                vec![
                    punto(desde, 0.0, Curva::Lineal),
                    punto(desde + entra / 3.0, clip.opacity, Curva::Frenar),
                ],
            ),
        ],
        Efecto::Sacudida => {
            let pasos = (((hasta - desde) / PASO_SACUDIDA).floor() as usize).max(2);
            let vaiven = |base: f64, amplitud: f64| -> Vec<Punto> {
                (0..=pasos)
                    .map(|i| {
                        let t = desde + (hasta - desde) * i as f64 / pasos as f64;
                        let resto = 1.0 - i as f64 / pasos as f64;
                        let signo = if i % 2 == 0 { 1.0 } else { -1.0 };
                        let valor = if i == 0 || i == pasos { base } else { base + signo * amplitud * resto };
                        punto(t, valor, Curva::Suave)
                    })
                    .collect()
            };
            vec![
                (KeyProperty::OffsetX, vaiven(clip.offset_x, 0.015 * intensidad)),
                (KeyProperty::Rotation, vaiven(clip.rotation, 1.5 * intensidad)),
            ]
        }
        Efecto::Pulso => {
            let latidos = (((hasta - desde) / LATIDO).floor() as usize).max(1);
            let largo = (hasta - desde) / latidos as f64;
            let mut puntos = vec![punto(desde, clip.scale, Curva::Lineal)];
            for i in 0..latidos {
                let inicio = desde + largo * i as f64;
                puntos.push(punto(inicio + largo * 0.4, clip.scale * (1.0 + 0.06 * intensidad), Curva::Frenar));
                puntos.push(punto(inicio + largo, clip.scale, Curva::Suave));
            }
            vec![(KeyProperty::Scale, puntos)]
        }
        Efecto::Deslizar(lado) => {
            let (propiedad, base, fuera) = match lado {
                Lado::Izquierda => (KeyProperty::OffsetX, clip.offset_x, clip.offset_x - 1.1),
                Lado::Derecha => (KeyProperty::OffsetX, clip.offset_x, clip.offset_x + 1.1),
                Lado::Arriba => (KeyProperty::OffsetY, clip.offset_y, clip.offset_y - 1.1),
                Lado::Abajo => (KeyProperty::OffsetY, clip.offset_y, clip.offset_y + 1.1),
            };
            let llegada = if intensidad > 1.0 { Curva::Rebote } else { Curva::Frenar };
            vec![(propiedad, vec![punto(desde, fuera, Curva::Lineal), punto(desde + entra, base, llegada)])]
        }
        Efecto::Giro => vec![
            (
                KeyProperty::Rotation,
                vec![
                    punto(desde, clip.rotation - 90.0 * intensidad, Curva::Lineal),
                    punto(desde + entra, clip.rotation, Curva::Frenar),
                ],
            ),
            (
                KeyProperty::Scale,
                vec![
                    punto(desde, clip.scale * 0.3, Curva::Lineal),
                    punto(desde + entra, clip.scale, Curva::Rebote),
                ],
            ),
        ],
        Efecto::Deriva => vec![
            (
                KeyProperty::Scale,
                vec![
                    punto(desde, clip.scale, Curva::Lineal),
                    punto(hasta, clip.scale * (1.0 + 0.12 * intensidad), Curva::Suave),
                ],
            ),
            (
                KeyProperty::OffsetX,
                vec![
                    punto(desde, clip.offset_x, Curva::Lineal),
                    punto(hasta, clip.offset_x - 0.04 * intensidad, Curva::Suave),
                ],
            ),
        ],
    };
    let mut comandos = Vec::new();
    for (propiedad, puntos) in pistas {
        comandos.extend(claves(clip, propiedad, &puntos)?);
    }
    Ok(comandos)
}

/// The keys that make a photo's two layers move apart over `desde..hasta`:
/// the background pushes in and drifts one way, the subject cut out above
/// it grows a touch more and drifts the other, so the eye reads depth.
/// The subject starts a little larger than the picture, so its edge covers
/// where it stands in the background beneath. `intensidad` as in
/// [`efecto`].
pub fn parallax(fondo: &Clip, sujeto: &Clip, desde: f64, hasta: f64, intensidad: f64) -> Result<Vec<Command>, String> {
    if !(0.2..=3.0).contains(&intensidad) {
        return Err(format!("una intensidad de {intensidad} no sirve: entre 0.2 y 3"));
    }
    let capas = [
        (fondo, 1.0, 1.0 + 0.08 * intensidad, 0.025 * intensidad),
        (sujeto, 1.03, 1.03 + 0.14 * intensidad, -0.035 * intensidad),
    ];
    let mut comandos = Vec::new();
    for (capa, inicio, fin, deriva) in capas {
        let escala = [
            punto(desde, capa.scale * inicio, Curva::Lineal),
            punto(hasta, capa.scale * fin, Curva::Suave),
        ];
        let x = [
            punto(desde, capa.offset_x, Curva::Lineal),
            punto(hasta, capa.offset_x + deriva, Curva::Suave),
        ];
        comandos.extend(claves(capa, KeyProperty::Scale, &escala)?);
        comandos.extend(claves(capa, KeyProperty::OffsetX, &x)?);
    }
    Ok(comandos)
}

fn punto(t: f64, valor: f64, curva: Curva) -> Punto {
    Punto { t, valor, curva }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiempo::prueba::clip;

    fn claves_de(comandos: &[Command], buscada: KeyProperty) -> Vec<(f64, f64)> {
        comandos
            .iter()
            .filter_map(|c| match c {
                Command::SetClipKey { property, at, value, .. } if *property == buscada => Some((*at, *value)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn free_keys_land_in_order_and_a_wild_value_is_refused() {
        let toma = clip(0.0, 10.0);
        let puntos = [
            punto(4.0, 1.5, Curva::Rebote),
            punto(2.0, 1.0, Curva::Lineal),
        ];
        let comandos = claves(&toma, KeyProperty::Scale, &puntos).unwrap();
        assert_eq!(claves_de(&comandos, KeyProperty::Scale), [(0.2, 1.0), (0.4, 1.5)]);
        assert!(claves(&toma, KeyProperty::Opacity, &[punto(1.0, 0.0, Curva::Lineal), punto(2.0, 4.0, Curva::Lineal)])
            .unwrap_err()
            .contains("entre 0 y 1"));
        assert!(claves(&toma, KeyProperty::Scale, &[punto(1.0, 1.0, Curva::Lineal)]).is_err());
        assert!(claves(&toma, KeyProperty::Scale, &[punto(1.0, 1.0, Curva::Lineal), punto(12.0, 1.0, Curva::Lineal)])
            .unwrap_err()
            .contains("se sale"));
    }

    #[test]
    fn a_bounce_overshoots_and_a_custom_curve_keeps_its_x_legal() {
        assert!(Curva::Rebote.ease().0[1] > 1.0);
        assert_eq!(Curva::Propia([1.5, 2.0, -1.0, 1.0]).ease().0, [1.0, 2.0, 0.0, 1.0]);
        for nombre in Curva::NOMBRES {
            assert!(Curva::por_nombre(nombre).is_some(), "{nombre}");
        }
    }

    #[test]
    fn every_move_starts_and_ends_on_the_clip_s_own_framing() {
        let toma = clip(0.0, 6.0);
        for nombre in Efecto::NOMBRES {
            let movimiento = Efecto::por_nombre(nombre, None).unwrap();
            let comandos = efecto(&toma, movimiento, 1.0, 4.0, 1.0).unwrap();
            assert!(!comandos.is_empty(), "{nombre}");
            for propiedad in [KeyProperty::OffsetX, KeyProperty::Rotation] {
                let llaves = claves_de(&comandos, propiedad);
                if let (Some(primera), Some(ultima)) = (llaves.first(), llaves.last()) {
                    if !matches!(movimiento, Efecto::Deriva | Efecto::Deslizar(_) | Efecto::Giro) {
                        assert_eq!(primera.1, ultima.1, "{nombre} {propiedad:?}");
                    }
                }
            }
            let escala = claves_de(&comandos, KeyProperty::Scale);
            if let Some(ultima) = escala.last()
                && !matches!(movimiento, Efecto::Deriva)
            {
                assert!((ultima.1 - toma.scale).abs() < 1e-9, "{nombre} ends at {}", ultima.1);
            }
        }
    }

    #[test]
    fn a_shake_dies_down_and_a_pulse_beats_twice_a_second() {
        let toma = clip(0.0, 4.0);
        let sacudida = efecto(&toma, Efecto::Sacudida, 0.0, 1.0, 1.0).unwrap();
        let xs = claves_de(&sacudida, KeyProperty::OffsetX);
        assert!(xs[1].1.abs() > xs[xs.len() - 2].1.abs());
        let pulso = efecto(&toma, Efecto::Pulso, 0.0, 2.0, 1.0).unwrap();
        assert_eq!(claves_de(&pulso, KeyProperty::Scale).len(), 1 + 2 * 4);
        assert!(efecto(&toma, Efecto::Pulso, 0.0, 2.0, 9.0).is_err());
    }

    #[test]
    fn a_slide_comes_from_off_the_frame_on_its_side() {
        let toma = clip(0.0, 4.0);
        let comandos = efecto(&toma, Efecto::Deslizar(Lado::Abajo), 0.0, 2.0, 1.0).unwrap();
        let ys = claves_de(&comandos, KeyProperty::OffsetY);
        assert!(ys[0].1 > 1.0 && ys[1].1 == toma.offset_y);
    }

    #[test]
    fn in_a_parallax_the_subject_moves_more_and_the_other_way() {
        let fondo = clip(0.0, 5.0);
        let mut sujeto = clip(0.0, 5.0);
        sujeto.id = "c2".into();
        let comandos = parallax(&fondo, &sujeto, 0.0, 5.0, 1.0).unwrap();
        let de = |id: &str, buscada: KeyProperty| -> Vec<f64> {
            comandos
                .iter()
                .filter_map(|c| match c {
                    Command::SetClipKey { clip_id, property, value, .. } if clip_id == id && *property == buscada => {
                        Some(*value)
                    }
                    _ => None,
                })
                .collect()
        };
        let crece = |v: Vec<f64>| v[1] - v[0];
        assert!(crece(de("c2", KeyProperty::Scale)) > crece(de("c1", KeyProperty::Scale)));
        assert!(crece(de("c1", KeyProperty::OffsetX)) > 0.0 && crece(de("c2", KeyProperty::OffsetX)) < 0.0);
        assert!(de("c2", KeyProperty::Scale)[0] > 1.0);
    }
}
