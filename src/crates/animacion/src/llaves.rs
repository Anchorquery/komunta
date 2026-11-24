// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The keys a recipe replaces: a new zoom or a new path for a title takes
//! off what the clip had on the same properties in the same stretch.

use concat_project::Command;
use concat_project::model::{Clip, KeyProperty};

use crate::tiempo::en_clip;

/// The commands that take off `clip`'s keys on `propiedades` inside the
/// timeline stretch `desde..=hasta`.
pub fn quitar(clip: &Clip, desde: f64, hasta: f64, propiedades: &[KeyProperty]) -> Vec<Command> {
    let (a, b) = (en_clip(clip, desde), en_clip(clip, hasta));
    clip.keys
        .iter()
        .filter(|llave| propiedades.contains(&llave.property))
        .filter(|llave| llave.at >= a - 1e-9 && llave.at <= b + 1e-9)
        .map(|llave| Command::ClearClipKey {
            clip_id: clip.id.clone(),
            property: llave.property,
            at: llave.at,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use concat_project::model::{ClipKey, KeyEase};

    use super::*;
    use crate::tiempo::prueba::clip;

    fn llave(property: KeyProperty, at: f64) -> ClipKey {
        ClipKey {
            property,
            at,
            value: 1.0,
            ease: KeyEase::LINEAR,
        }
    }

    #[test]
    fn only_the_named_properties_inside_the_stretch_go() {
        let mut toma = clip(0.0, 10.0);
        toma.keys = vec![
            llave(KeyProperty::OffsetX, 0.1),
            llave(KeyProperty::OffsetX, 0.5),
            llave(KeyProperty::Scale, 0.5),
            llave(KeyProperty::OffsetY, 0.9),
        ];
        let quitadas = quitar(&toma, 2.0, 6.0, &[KeyProperty::OffsetX, KeyProperty::OffsetY]);
        assert_eq!(quitadas.len(), 1, "{quitadas:?}");
        assert!(matches!(
            &quitadas[0],
            Command::ClearClipKey { property: KeyProperty::OffsetX, at, .. } if (*at - 0.5).abs() < 1e-9
        ));
    }
}
