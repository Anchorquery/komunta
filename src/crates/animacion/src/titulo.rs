// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! How a title comes in and goes: the keys and fades that animate a text
//! clip already on the timeline. The clip is placed first - the editor
//! mints its id - and these are applied to it after.

use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{Clip, KeyEase, KeyProperty};

use crate::tiempo::en_clip;

/// A way in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrada {
    /// Fades in and out.
    Aparecer,
    /// Rises a little into its place as it fades in.
    Subir,
    /// Grows from small, past its size and back: a pop.
    Escalar,
}

impl Entrada {
    /// The way in a word names: "aparecer", "subir", "escalar".
    pub fn por_nombre(nombre: &str) -> Option<Entrada> {
        Some(match nombre {
            "aparecer" => Entrada::Aparecer,
            "subir" => Entrada::Subir,
            "escalar" => Entrada::Escalar,
            _ => return None,
        })
    }

    /// Every name, for a tool's schema.
    pub const NOMBRES: [&'static str; 3] = ["aparecer", "subir", "escalar"];
}

/// Seconds the way in takes.
const ENTRA: f64 = 0.4;
/// Seconds the fade out takes.
const SALE: f64 = 0.3;
/// How far below its place a rising title starts, as a fraction of the
/// frame's height.
const SUBIDA: f64 = 0.05;

/// The commands that bring `titulo` in as `entrada` says and fade it out.
/// A title too short for the way in gets it squeezed into a third of its
/// length.
pub fn animar(titulo: &Clip, entrada: Entrada) -> Vec<Command> {
    let entra = ENTRA.min(titulo.duration / 3.0);
    let sale = SALE.min(titulo.duration / 3.0);
    let mut comandos = vec![Command::UpdateClip {
        clip_id: titulo.id.clone(),
        patch: ClipPatch {
            fade_in: Some(entra),
            fade_out: Some(sale),
            ..ClipPatch::default()
        },
    }];
    let llegada = en_clip(titulo, titulo.start + entra);
    let llave = |propiedad, at, valor, ease| Command::SetClipKey {
        clip_id: titulo.id.clone(),
        property: propiedad,
        at,
        value: valor,
        ease,
    };
    match entrada {
        Entrada::Aparecer => {}
        Entrada::Subir => comandos.extend([
            llave(KeyProperty::OffsetY, 0.0, titulo.offset_y + SUBIDA, KeyEase::LINEAR),
            llave(KeyProperty::OffsetY, llegada, titulo.offset_y, KeyEase::OUT),
        ]),
        Entrada::Escalar => {
            let pasado = en_clip(titulo, titulo.start + entra * 0.7);
            comandos.extend([
                llave(KeyProperty::Scale, 0.0, titulo.scale * 0.6, KeyEase::LINEAR),
                llave(KeyProperty::Scale, pasado, titulo.scale * 1.08, KeyEase::OUT),
                llave(KeyProperty::Scale, llegada, titulo.scale, KeyEase::IN_OUT),
            ]);
        }
    }
    comandos
}

#[cfg(test)]
mod tests {
    use concat_project::model::{Clip, ClipKind};

    use super::*;

    fn titulo(duracion: f64) -> Clip {
        let mut clip = Clip::blank("c9", "t2", ClipKind::Text, "Hola", 3.0, duracion);
        clip.offset_y = 0.3;
        clip
    }

    #[test]
    fn a_rising_title_starts_below_its_place_and_lands_on_it() {
        let comandos = animar(&titulo(2.0), Entrada::Subir);
        let valores: Vec<(f64, f64)> = comandos
            .iter()
            .filter_map(|c| match c {
                Command::SetClipKey { at, value, property: KeyProperty::OffsetY, .. } => Some((*at, *value)),
                _ => None,
            })
            .collect();
        assert_eq!(valores.len(), 2);
        assert!((valores[0].1 - 0.35).abs() < 1e-9);
        assert!((valores[1].0 - 0.2).abs() < 1e-9 && valores[1].1 == 0.3, "{valores:?}");
    }

    #[test]
    fn every_way_in_fades_and_a_short_title_squeezes_it() {
        for nombre in Entrada::NOMBRES {
            let entrada = Entrada::por_nombre(nombre).unwrap();
            match &animar(&titulo(0.6), entrada)[0] {
                Command::UpdateClip { patch, .. } => assert!((patch.fade_in.unwrap() - 0.2).abs() < 1e-9),
                otro => panic!("{otro:?}"),
            }
        }
        assert_eq!(Entrada::por_nombre("girar"), None);
    }
}
