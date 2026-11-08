//! What a media file is: length, picture size and sound. Built with the
//! `sonda` feature - as the app is - it asks the app's own libavformat
//! (`concat_media::probe`), so nothing outside the app is needed. Without
//! it, as the command line is built, `ffprobe` answers as the engine's did.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// A probed file.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sondeo {
    /// Seconds.
    pub duracion: f64,
    /// Displayed width (a quarter turn already applied).
    pub ancho: u32,
    /// Displayed height.
    pub alto: u32,
    /// Frames per second.
    pub fps: f64,
    /// Whether it carries sound.
    pub audio: bool,
}

/// Probes a file with what this build has: the app's libavformat with
/// `sonda`, `ffprobe` otherwise. None when there is nothing to play.
pub fn sondear(ruta: &Path) -> Option<Sondeo> {
    #[cfg(feature = "sonda")]
    return libav(ruta);
    #[cfg(not(feature = "sonda"))]
    ffprobe(ruta)
}

/// Probes a file with the app's libavformat. None when it cannot be
/// opened or holds neither picture nor sound.
#[cfg(feature = "sonda")]
pub fn libav(ruta: &Path) -> Option<Sondeo> {
    let info = concat_media::probe(ruta).ok()?;
    let duracion = info.duration.map_or(0.0, |d| d.as_f64());
    let audio = info.audio.is_some();
    let Some(video) = info.video else {
        // Sound only: a length and nothing to show.
        return audio.then_some(Sondeo {
            duracion,
            audio,
            ..Sondeo::default()
        });
    };
    Some(Sondeo {
        duracion,
        // Already turned a quarter where the stream says so.
        ancho: video.width,
        alto: video.height,
        fps: video.frame_rate.fps().as_f64(),
        audio,
    })
}

/// Probes a file the way the engine's `sondear` does. None when there is
/// no `ffprobe` or nothing to play.
pub fn ffprobe(ruta: &Path) -> Option<Sondeo> {
    let mut orden = Command::new("ffprobe");
    orden.args([
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_streams",
        "-show_format",
    ]);
    orden.arg(ruta);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up from a windowed app.
        orden.creation_flags(0x0800_0000);
    }
    let salida = orden.output().ok()?;
    if !salida.status.success() {
        return None;
    }
    leer(&serde_json::from_slice(&salida.stdout).ok()?)
}

/// Reads `ffprobe -show_streams -show_format` JSON.
fn leer(j: &Value) -> Option<Sondeo> {
    let streams = j["streams"].as_array()?;
    let audio = streams.iter().any(|s| s["codec_type"] == "audio");
    let duracion = j["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);
    let Some(v) = streams.iter().find(|s| s["codec_type"] == "video") else {
        // Sound only: a length and nothing to show.
        return audio.then_some(Sondeo {
            duracion,
            audio,
            ..Sondeo::default()
        });
    };
    let rotacion = v["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|sd| sd["rotation"].as_i64())
        .unwrap_or(0);
    let (mut ancho, mut alto) = (v["width"].as_u64()? as u32, v["height"].as_u64()? as u32);
    if rotacion.abs() == 90 || rotacion.abs() == 270 {
        (ancho, alto) = (alto, ancho);
    }
    let fps = v["r_frame_rate"]
        .as_str()
        .and_then(|r| r.split_once('/'))
        .and_then(|(n, d)| Some(n.parse::<f64>().ok()? / d.parse::<f64>().ok()?.max(1.0)))
        .unwrap_or(30.0);
    Some(Sondeo {
        duracion,
        ancho,
        alto,
        fps,
        audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_phone_clip_turned_a_quarter_is_tall() {
        let j = json!({
            "streams": [
                {"codec_type": "video", "width": 3840, "height": 2160, "r_frame_rate": "30/1",
                 "side_data_list": [{"rotation": -90}]},
                {"codec_type": "audio"}
            ],
            "format": {"duration": "5.032"}
        });
        let s = leer(&j).unwrap();
        assert_eq!((s.ancho, s.alto), (2160, 3840));
        assert!(s.audio);
        assert!((s.duracion - 5.032).abs() < 1e-9);
    }

    #[cfg(feature = "sonda")]
    #[test]
    fn a_file_that_is_not_there_has_nothing_to_play() {
        assert_eq!(libav(Path::new("no-existe/clip.mp4")), None);
    }
}
