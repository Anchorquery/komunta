// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Timeline seconds and the place in a clip a key is set at: a key's `at`
//! is a fraction of the clip's length, a recipe is asked in seconds.

use concat_project::model::Clip;

/// One frame at 30 fps: how far apart two keys are for a cut rather than
/// a move.
pub const FOTOGRAMA: f64 = 1.0 / 30.0;

/// Where timeline second `t` falls in `clip`, `0..=1`.
pub fn en_clip(clip: &Clip, t: f64) -> f64 {
    if clip.duration <= 0.0 {
        return 0.0;
    }
    ((t - clip.start) / clip.duration).clamp(0.0, 1.0)
}

/// The stretch `desde..hasta` of the timeline inside `clip`, or why it is
/// not.
pub fn tramo(clip: &Clip, desde: f64, hasta: f64) -> Result<(f64, f64), String> {
    let fin = clip.start + clip.duration;
    if !(desde.is_finite() && hasta.is_finite()) || hasta <= desde {
        return Err(format!("el tramo {desde}–{hasta} s no tiene duración"));
    }
    if desde < clip.start - 1e-6 || hasta > fin + 1e-6 {
        return Err(format!(
            "el tramo {desde:.2}–{hasta:.2} s se sale del clip {} ({:.2}–{fin:.2} s)",
            clip.id, clip.start
        ));
    }
    Ok((desde.max(clip.start), hasta.min(fin)))
}

#[cfg(test)]
pub(crate) mod prueba {
    use concat_project::model::{Clip, ClipKind};

    /// A video clip from `start`, `duration` seconds long.
    pub fn clip(start: f64, duration: f64) -> Clip {
        Clip::blank("c1", "t1", ClipKind::Video, "toma", start, duration)
    }
}

#[cfg(test)]
mod tests {
    use super::prueba::clip;
    use super::*;

    #[test]
    fn seconds_become_a_fraction_of_the_clip_and_a_stretch_outside_it_is_refused() {
        let toma = clip(10.0, 4.0);
        assert_eq!(en_clip(&toma, 11.0), 0.25);
        assert_eq!(en_clip(&toma, 30.0), 1.0);
        assert_eq!(tramo(&toma, 11.0, 12.0), Ok((11.0, 12.0)));
        assert!(tramo(&toma, 9.0, 12.0).unwrap_err().contains("se sale"));
        assert!(tramo(&toma, 12.0, 12.0).is_err());
    }
}
