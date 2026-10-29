//! A clip's sound, as the analysis hears it: 16 kHz mono floats.

use std::path::Path;

use concat_media::{AudioDecoder, AudioOptions, SampleFormat};

use crate::ruido::MUESTREO;

/// The whole of the clip's first audio stream, run through `filtros`
/// (ffmpeg audio filters, in order) on the way.
pub fn leer(ruta: &Path, filtros: Vec<String>) -> Result<Vec<f32>, concat_media::Error> {
    let opciones = AudioOptions {
        filters: filtros,
        rate: MUESTREO as u32,
        channels: 1,
        format: SampleFormat::F32,
        ..AudioOptions::default()
    };
    AudioDecoder::open(ruta, &opciones)?.collect_f32()
}
