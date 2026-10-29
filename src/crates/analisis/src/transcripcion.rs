//! A clip's transcription, as `analisis.json` keeps it.

use serde::{Deserialize, Serialize};

use crate::palabras::{self, Palabra, PalabraOida};
use crate::Idioma;

/// What was said in a clip, word by word, and in which language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transcripcion {
    /// Whisper's code for the language.
    pub idioma: String,
    /// How sure whisper was of the language, to the hundredth.
    pub prob_idioma: f64,
    /// The words, numbered.
    pub palabras: Vec<Palabra>,
    /// The words as one line.
    pub texto: String,
}

impl Transcripcion {
    /// The transcription of what whisper heard in `idioma`, sure of it to
    /// `probabilidad`.
    pub fn nueva(oidas: impl IntoIterator<Item = PalabraOida>, idioma: &Idioma, probabilidad: f64) -> Self {
        let palabras = palabras::palabras(oidas, idioma);
        Self {
            idioma: idioma.codigo().to_owned(),
            prob_idioma: (probabilidad * 100.0).round() / 100.0,
            texto: palabras::texto(&palabras),
            palabras,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transcription_is_written_as_the_engine_writes_it() {
        let oida = PalabraOida { inicio: 0.0, fin: 0.5, texto: " Kaixo".into(), probabilidad: 0.9 };
        let transcripcion = Transcripcion::nueva([oida], &Idioma::euskera(), 0.876);
        let json = serde_json::to_value(&transcripcion).unwrap();
        assert_eq!(json["idioma"], "eu");
        assert_eq!(json["prob_idioma"], 0.88);
        assert_eq!(json["texto"], "Kaixo");
        assert_eq!(json["palabras"][0]["txt"], "Kaixo");
    }
}
