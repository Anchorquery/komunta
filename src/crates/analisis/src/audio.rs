//! A clip's sound, as the analysis hears it: mono floats, at 16 kHz for
//! the voice and whisper, at another rate for the music's beats.

use std::path::Path;

use concat_media::{AudioDecoder, AudioOptions, SampleFormat};

use crate::ruido::MUESTREO;

/// The whole of the clip's first audio stream, run through `filtros`
/// (ffmpeg audio filters, in order) on the way.
pub fn leer(ruta: &Path, filtros: Vec<String>) -> Result<Vec<f32>, concat_media::Error> {
    leer_a(ruta, filtros, MUESTREO)
}

/// As [`leer`], at `muestreo` samples a second.
pub fn leer_a(
    ruta: &Path,
    filtros: Vec<String>,
    muestreo: usize,
) -> Result<Vec<f32>, concat_media::Error> {
    let opciones = AudioOptions {
        filters: filtros,
        rate: muestreo as u32,
        channels: 1,
        format: SampleFormat::F32,
        ..AudioOptions::default()
    };
    AudioDecoder::open(ruta, &opciones)?.collect_f32()
}
