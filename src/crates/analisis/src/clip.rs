//! One clip heard end to end: its voice, its room, and - when it is a take
//! - its words. The sound half of a clip's entry in `analisis.json`.

use std::path::Path;

use serde::Serialize;

use crate::escucha::{self, Oido};
use crate::{audio, voz, Idioma, Ruido, Transcripcion};

/// The engine's knobs for the sound half, with its defaults.
#[derive(Clone, Debug)]
pub struct Ajustes {
    /// The share of a clip that must be speech for it to be a take.
    pub umbral_voz: f64,
    /// Clean the sound before whisper hears it.
    pub limpiar: bool,
    /// The noise floor above which a clip is noisy, in dBFS.
    pub umbral_ruido_db: f64,
    /// The shoot's language, when known; otherwise whisper's guess.
    pub idioma: Option<Idioma>,
}

impl Default for Ajustes {
    fn default() -> Self {
        Self {
            umbral_voz: 0.35,
            limpiar: true,
            umbral_ruido_db: crate::ruido::UMBRAL_DB,
            idioma: None,
        }
    }
}

/// What a clip's sound holds, under the names `analisis.json` gives it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Escucha {
    /// Where somebody speaks, in seconds.
    pub voz: Vec<[f64; 2]>,
    /// The room's noise.
    pub audio_info: Ruido,
    /// The share of the clip that is speech.
    pub ratio_voz: f64,
    /// Spoken enough to be a take.
    pub toma_candidata: bool,
    /// What a take says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcripcion: Option<Transcripcion>,
}

/// Why a clip could not be heard.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file's sound would not decode.
    #[error("could not read the sound of {}: {motivo}", ruta.display())]
    Audio {
        /// The clip.
        ruta: std::path::PathBuf,
        /// What the decoder said.
        motivo: String,
    },
    /// whisper failed.
    #[error(transparent)]
    Escucha(#[from] escucha::Error),
}

/// Hears the clip at `ruta`, `duracion` seconds long; `con_audio` false for
/// a clip with no sound stream, which is heard as silence.
pub fn escuchar(oido: &mut Oido, ruta: &Path, duracion: f64, con_audio: bool, ajustes: &Ajustes) -> Result<Escucha, Error> {
    let crudo = if con_audio { leer(ruta, Vec::new())? } else { Vec::new() };
    let voz = if crudo.is_empty() { Vec::new() } else { voz::tramos(oido.voz(&crudo)?) };
    let pares: Vec<(f64, f64)> = voz.iter().map(|&[inicio, fin]| (inicio, fin)).collect();
    let audio_info = Ruido::medir(&crudo, &pares, ajustes.umbral_ruido_db);
    let ratio_voz = voz::ratio(&voz, duracion);
    let toma_candidata = voz::es_toma(ratio_voz, ajustes.umbral_voz);
    let transcripcion = if toma_candidata {
        let limpio = if ajustes.limpiar { leer(ruta, audio_info.filtros())? } else { crudo };
        Some(oido.transcribir(&limpio, ajustes.idioma.as_ref())?)
    } else {
        None
    };
    Ok(Escucha { voz, audio_info, ratio_voz, toma_candidata, transcripcion })
}

fn leer(ruta: &Path, filtros: Vec<String>) -> Result<Vec<f32>, Error> {
    audio::leer(ruta, filtros).map_err(|error| Error::Audio { ruta: ruta.to_owned(), motivo: error.to_string() })
}
