// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! A copy of a clip, made of commands: the editor places a clip of the
//! whole file, and these make it the copy - what Duplicate and Paste do,
//! and what a title behind a person needs, the person copied above it.

use concat_project::Command;
use concat_project::commands::{ClipMove, ClipPatch, TrimEdge};
use concat_project::model::Clip;

/// The edits that make `placed` - the whole of `source`'s file, just laid
/// on a free stretch of the lane - into a copy of `source` at `start` on
/// `track_id`: its frames, speed, placement, keys and mix.
///
/// The footage is trimmed at normal speed to what the source covers, then
/// retimed - the speed command keeps the footage covered and recomputes
/// the length - and only then moved to `start`, so neighbours there never
/// push the whole file aside and shift the frames the trims cut.
pub fn comandos(source: &Clip, placed: &Clip, track_id: &str, start: f64) -> Vec<Command> {
    let created = &placed.id;
    let mut commands = Vec::new();
    let head = source.source_start - placed.source_start;
    if head.abs() > 1e-6 {
        commands.push(Command::TrimClip {
            clip_id: created.clone(),
            edge: TrimEdge::Start,
            delta: head,
            ripple: false,
        });
    }
    // The footage the source covers, at normal speed.
    let covered = source.duration * source.speed;
    let tail = covered - (placed.duration - head);
    if tail.abs() > 1e-6 {
        commands.push(Command::TrimClip {
            clip_id: created.clone(),
            edge: TrimEdge::End,
            delta: tail,
            ripple: false,
        });
    }
    if (source.speed - 1.0).abs() > 1e-9 {
        commands.push(Command::SetClipSpeed {
            clip_id: created.clone(),
            speed: source.speed,
        });
    }
    if source.speed_curve.is_some() {
        commands.push(Command::SetClipSpeedCurve {
            clip_id: created.clone(),
            curve: source.speed_curve.clone(),
        });
    }
    commands.push(Command::MoveClips {
        moves: vec![ClipMove {
            clip_id: created.clone(),
            start,
            track_id: track_id.to_owned(),
        }],
    });
    commands.push(Command::SetClipTransform {
        clip_id: created.clone(),
        scale: Some(source.scale),
        offset_x: Some(source.offset_x),
        offset_y: Some(source.offset_y),
        rotation: Some(source.rotation),
        stretch_x: Some(source.stretch_x),
        stretch_y: Some(source.stretch_y),
    });
    for key in &source.keys {
        commands.push(Command::SetClipKey {
            clip_id: created.clone(),
            property: key.property,
            at: key.at,
            value: key.value,
            ease: key.ease,
        });
    }
    commands.push(Command::UpdateClip {
        clip_id: created.clone(),
        patch: ClipPatch {
            name: Some(format!("{} copy", source.name)),
            volume: Some(source.volume),
            fade_in: Some(source.fade_in),
            fade_out: Some(source.fade_out),
            opacity: Some(source.opacity),
            preserve_pitch: Some(source.preserve_pitch),
            muted: source.muted,
            flip_h: Some(source.flip_h),
            flip_v: Some(source.flip_v),
            blend: Some(source.blend.clone()),
            crop: Some(source.crop),
            filters: Some(source.filters.clone()),
            video_effects: Some(source.video_effects.clone()),
            ..ClipPatch::default()
        },
    });
    commands
}

/// `clip` cut to the timeline stretch `desde..hasta`, as a copy of that
/// stretch alone would be: its start, length and first frame moved, its
/// keys put where they fall in the shorter clip, those outside it dropped.
/// What a copy of part of a clip is made from.
pub fn tramo(clip: &Clip, desde: f64, hasta: f64) -> Clip {
    let desde = desde.max(clip.start);
    let hasta = hasta.min(clip.start + clip.duration).max(desde);
    let duracion = hasta - desde;
    let mut parte = clip.clone();
    parte.start = desde;
    parte.duration = duracion;
    parte.source_start = clip.source_start + (desde - clip.start) * clip.speed;
    parte.fade_in = 0.0;
    parte.fade_out = 0.0;
    parte.keys = clip
        .keys
        .iter()
        .filter_map(|llave| {
            let t = clip.start + llave.at * clip.duration;
            let at = if duracion > 0.0 { (t - desde) / duracion } else { 0.0 };
            (-1e-9..=1.0 + 1e-9).contains(&at).then(|| concat_project::model::ClipKey {
                at: at.clamp(0.0, 1.0),
                ..*llave
            })
        })
        .collect();
    parte
}

#[cfg(test)]
mod tests {
    use concat_project::model::{ClipKey, KeyEase, KeyProperty};

    use super::*;
    use crate::tiempo::prueba::clip;

    #[test]
    fn a_stretch_of_a_clip_starts_later_in_the_footage_and_keeps_its_keys_where_they_fall() {
        let mut toma = clip(10.0, 10.0);
        toma.source_start = 2.0;
        toma.keys = vec![
            ClipKey { property: KeyProperty::Scale, at: 0.1, value: 1.0, ease: KeyEase::LINEAR },
            ClipKey { property: KeyProperty::Scale, at: 0.5, value: 1.2, ease: KeyEase::LINEAR },
        ];
        let parte = tramo(&toma, 14.0, 16.0);
        assert_eq!((parte.start, parte.duration, parte.source_start), (14.0, 2.0, 6.0));
        assert_eq!(parte.keys.len(), 1);
        assert!((parte.keys[0].at - 0.5).abs() < 1e-9);
    }
}
