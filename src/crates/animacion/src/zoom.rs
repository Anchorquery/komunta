// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! A push in on a stretch of a clip: the picture grows to `factor` over
//! the phrase that matters and goes back after it, either eased in and out
//! or cut on the frame - the punch-in of talking-head edits. Towards a
//! point of the frame, a face say, which stays where it was while the rest
//! grows around it.

use concat_project::Command;
use concat_project::model::{Clip, KeyEase, KeyProperty};

use crate::llaves;
use crate::tiempo::{FOTOGRAMA, en_clip, tramo};

/// How the picture gets to its zoom and back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Estilo {
    /// Eased over a quarter of a second each way.
    Suave,
    /// Cut on the frame: in at once, out at once.
    Golpe,
}

/// What to zoom on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    /// Timeline second the zoom starts at.
    pub desde: f64,
    /// Timeline second it is back by.
    pub hasta: f64,
    /// How much larger, over the clip's own scale: 1.15 is a typical push.
    pub factor: f64,
    /// The point of the frame that stays put, `0..=1` from the top left;
    /// the middle when None.
    pub centro: Option<(f64, f64)>,
    /// Eased or cut.
    pub estilo: Estilo,
}

/// Longest ease of a [`Estilo::Suave`] zoom, each way.
const RAMPA: f64 = 0.25;
/// The zoom factors a recipe accepts: less is not seen, more is a crop.
const FACTORES: std::ops::RangeInclusive<f64> = 1.02..=2.5;
/// What a zoom keys, and so replaces.
const ENCUADRE: [KeyProperty; 3] = [KeyProperty::Scale, KeyProperty::OffsetX, KeyProperty::OffsetY];

/// The commands that zoom `clip` as `zoom` says, or why not. Keys the clip
/// already has on its scale and offsets inside the stretch are replaced.
pub fn en(clip: &Clip, zoom: Zoom) -> Result<Vec<Command>, String> {
    let (desde, hasta) = tramo(clip, zoom.desde, zoom.hasta)?;
    if !FACTORES.contains(&zoom.factor) {
        return Err(format!("un zoom de {} no sirve: entre 1.02 y 2.5", zoom.factor));
    }
    let mut comandos = llaves::quitar(clip, desde, hasta, &ENCUADRE);
    let (x, y) = zoom.centro.unwrap_or((0.5, 0.5));
    let rampa = match zoom.estilo {
        Estilo::Suave => RAMPA.min((hasta - desde) / 3.0),
        Estilo::Golpe => FOTOGRAMA.min((hasta - desde) / 3.0),
    };
    let ease = match zoom.estilo {
        Estilo::Suave => KeyEase::IN_OUT,
        Estilo::Golpe => KeyEase::LINEAR,
    };
    let propiedades = [
        (KeyProperty::Scale, clip.scale, clip.scale * zoom.factor),
        (KeyProperty::OffsetX, clip.offset_x, clip.offset_x + (0.5 - x) * (zoom.factor - 1.0)),
        (KeyProperty::OffsetY, clip.offset_y, clip.offset_y + (0.5 - y) * (zoom.factor - 1.0)),
    ];
    for (propiedad, base, cerca) in propiedades {
        if (cerca - base).abs() < 1e-9 {
            continue;
        }
        let puntos = [
            (desde, base),
            (desde + rampa, cerca),
            (hasta - rampa, cerca),
            (hasta, base),
        ];
        comandos.extend(puntos.into_iter().map(|(t, valor)| Command::SetClipKey {
            clip_id: clip.id.clone(),
            property: propiedad,
            at: en_clip(clip, t),
            value: valor,
            ease,
        }));
    }
    Ok(comandos)
}

#[cfg(test)]
mod tests {
    use concat_project::Editor;
    use concat_project::model::{ClipKind, MediaKind};

    use super::*;
    use crate::tiempo::prueba::clip;

    fn zoom(desde: f64, hasta: f64, factor: f64) -> Zoom {
        Zoom {
            desde,
            hasta,
            factor,
            centro: None,
            estilo: Estilo::Suave,
        }
    }

    fn llaves(comandos: &[Command]) -> Vec<(KeyProperty, f64, f64)> {
        comandos
            .iter()
            .filter_map(|c| match c {
                Command::SetClipKey { property, at, value, .. } => Some((*property, *at, *value)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_centred_zoom_keys_only_the_scale_in_and_back_out() {
        let toma = clip(0.0, 10.0);
        let comandos = en(&toma, zoom(2.0, 4.0, 1.2)).unwrap();
        let escala = llaves(&comandos);
        assert_eq!(escala.len(), 4, "{escala:?}");
        assert!(escala.iter().all(|(p, ..)| *p == KeyProperty::Scale));
        assert_eq!(escala[0], (KeyProperty::Scale, 0.2, 1.0));
        assert!((escala[1].1 - 0.225).abs() < 1e-9 && (escala[1].2 - 1.2).abs() < 1e-9);
        assert_eq!(escala[3], (KeyProperty::Scale, 0.4, 1.0));
    }

    #[test]
    fn a_zoom_towards_a_face_moves_the_picture_so_the_face_stays() {
        let toma = clip(0.0, 10.0);
        let comandos = en(
            &toma,
            Zoom {
                centro: Some((0.7, 0.3)),
                estilo: Estilo::Golpe,
                ..zoom(2.0, 4.0, 1.5)
            },
        )
        .unwrap();
        let x: Vec<_> = llaves(&comandos).into_iter().filter(|(p, ..)| *p == KeyProperty::OffsetX).collect();
        // A point right of the middle stays put when the picture moves left.
        assert!((x[1].2 - (-0.1)).abs() < 1e-9, "{x:?}");
        // A cut: in within a frame.
        assert!((x[1].1 - x[0].1) * 10.0 <= FOTOGRAMA + 1e-9);
    }

    #[test]
    fn a_zoom_outside_the_clip_or_too_strong_is_refused() {
        let toma = clip(5.0, 2.0);
        assert!(en(&toma, zoom(4.0, 6.0, 1.2)).is_err());
        assert!(en(&toma, zoom(5.5, 6.0, 4.0)).unwrap_err().contains("2.5"));
    }

    #[test]
    fn the_editor_takes_the_commands_and_plays_the_zoom() {
        let mut editor = Editor::new();
        let medio = editor
            .apply(Command::AddMedia {
                item: concat_project::commands::NewMedia {
                    path: "/a.mp4".into(),
                    name: "a.mp4".into(),
                    duration: Some(10.0),
                    kind: MediaKind::Video,
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some(30.0),
                    frame_rate_fraction: Some("30/1".into()),
                    video_codec: Some("h264".into()),
                    audio_codec: None,
                    has_audio: false,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .unwrap()
            .created_id
            .unwrap();
        let pista = editor.project().active().tracks[0].id.clone();
        let id = editor
            .apply(Command::AddClip {
                media_id: medio,
                track_id: pista,
                start: 0.0,
                ripple: false,
            })
            .unwrap()
            .created_id
            .unwrap();
        let toma = editor.project().active().clip(&id).unwrap().clone();
        assert_eq!(toma.kind, ClipKind::Video);
        let comandos = en(&toma, zoom(2.0, 4.0, 1.2)).unwrap();
        editor.apply(Command::Batch { commands: comandos }).unwrap();
        assert_eq!(editor.project().active().clip(&id).unwrap().keys.len(), 4);

        // A second zoom over the same stretch replaces the first.
        let toma = editor.project().active().clip(&id).unwrap().clone();
        let comandos = en(&toma, zoom(2.0, 4.0, 1.4)).unwrap();
        editor.apply(Command::Batch { commands: comandos }).unwrap();
        let keys = &editor.project().active().clip(&id).unwrap().keys;
        assert_eq!(keys.len(), 4);
        assert!(keys.iter().any(|k| (k.value - 1.4).abs() < 1e-9));
    }
}
