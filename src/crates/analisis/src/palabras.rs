//! The words whisper heard, made into the clip's word list: numbered,
//! rounded, with the fillers marked and whisper's loops dropped.

use serde::{Deserialize, Serialize};

use crate::Idioma;

/// A word as whisper gives it, before anything is decided about it.
#[derive(Clone, Debug, PartialEq)]
pub struct PalabraOida {
    /// Seconds from the clip's start.
    pub inicio: f64,
    /// Seconds from the clip's start.
    pub fin: f64,
    /// As heard, with whisper's leading space.
    pub texto: String,
    /// Whisper's confidence, 0 to 1.
    pub probabilidad: f64,
}

/// One word of a clip, as `analisis.json` keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palabra {
    /// Its place among the clip's words.
    #[serde(rename = "i")]
    pub indice: usize,
    /// Seconds from the clip's start, to the hundredth.
    #[serde(rename = "ini")]
    pub inicio: f64,
    /// Seconds from the clip's start, to the hundredth.
    pub fin: f64,
    /// The word, trimmed.
    #[serde(rename = "txt")]
    pub texto: String,
    /// Whisper's confidence, to the hundredth.
    #[serde(rename = "p")]
    pub probabilidad: f64,
    /// A filler - "eh", "pues", "ba" - that a cut may drop.
    pub coletilla: bool,
}

/// Whisper caught in a loop on background noise says the same word over
/// and over ("oro oro oro..."): past this many in a row, the rest go.
const REPETICIONES_MAX: usize = 3;

/// The clip's words: numbered in order, rounded, fillers marked, and a
/// word repeated more than [`REPETICIONES_MAX`] times running cut back.
pub fn palabras(oidas: impl IntoIterator<Item = PalabraOida>, idioma: &Idioma) -> Vec<Palabra> {
    let mut palabras = Vec::new();
    let mut anterior: Option<String> = None;
    let mut repetidas = 0;
    for oida in oidas {
        let forma = normaliza(&oida.texto);
        repetidas = if anterior.as_deref() == Some(forma.as_str()) {
            repetidas + 1
        } else {
            0
        };
        let coletilla = idioma.es_coletilla(&forma) && !en_mayusculas(oida.texto.trim());
        anterior = Some(forma);
        if repetidas >= REPETICIONES_MAX {
            continue;
        }
        palabras.push(Palabra {
            indice: palabras.len(),
            inicio: centesimas(oida.inicio),
            fin: centesimas(oida.fin),
            texto: oida.texto.trim().to_owned(),
            probabilidad: centesimas(oida.probabilidad),
            coletilla,
        });
    }
    palabras
}

/// The clip's words as one line of text.
pub fn texto(palabras: &[Palabra]) -> String {
    palabras
        .iter()
        .map(|palabra| palabra.texto.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A word reduced to what compares: lower case, letters and digits only.
/// "¿Pues," and "pues" are the same word, and "o sea" is "osea".
pub fn normaliza(palabra: &str) -> String {
    palabra
        .to_lowercase()
        .replace("o sea", "osea")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Shouted, or an acronym: "EH" said on purpose is not a filler.
fn en_mayusculas(palabra: &str) -> bool {
    palabra.chars().any(char::is_alphabetic)
        && palabra
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(char::is_uppercase)
}

fn centesimas(valor: f64) -> f64 {
    (valor * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oida(texto: &str, inicio: f64) -> PalabraOida {
        PalabraOida {
            inicio,
            fin: inicio + 0.333,
            texto: texto.into(),
            probabilidad: 0.876,
        }
    }

    #[test]
    fn words_are_numbered_trimmed_and_rounded() {
        let dichas = palabras([oida(" Hoy", 0.0), oida(" quiero", 0.4)], &Idioma::castellano());
        assert_eq!(dichas.len(), 2);
        assert_eq!(dichas[1].indice, 1);
        assert_eq!(dichas[1].texto, "quiero");
        assert_eq!((dichas[1].inicio, dichas[1].fin), (0.4, 0.73));
        assert_eq!(dichas[1].probabilidad, 0.88);
        assert_eq!(texto(&dichas), "Hoy quiero");
    }

    #[test]
    fn fillers_are_marked_in_their_own_language() {
        let es = palabras([oida(" Pues,", 0.0), oida(" ba", 1.0)], &Idioma::castellano());
        assert!(es[0].coletilla, "«pues» is a Spanish filler");
        assert!(!es[1].coletilla, "«ba» is not, in Spanish");
        let eu = palabras([oida(" Ba,", 0.0)], &Idioma::euskera());
        assert!(eu[0].coletilla);
        let gritado = palabras([oida(" EH", 0.0)], &Idioma::castellano());
        assert!(!gritado[0].coletilla, "said in capitals, on purpose");
        let otro = palabras([oida(" eh", 0.0)], &Idioma::from("en"));
        assert!(!otro[0].coletilla, "no list for a third language");
    }

    #[test]
    fn a_loop_is_cut_after_three_of_the_same() {
        let bucle = (0..6).map(|n| oida(if n % 2 == 0 { " oro" } else { " Oro," }, n as f64));
        let dichas = palabras(bucle.chain([oida(" fin", 9.0)]), &Idioma::castellano());
        let textos: Vec<&str> = dichas.iter().map(|p| p.texto.as_str()).collect();
        assert_eq!(textos, ["oro", "Oro,", "oro", "fin"]);
        assert_eq!(dichas.last().unwrap().indice, 3, "numbered after the cut");
    }

    #[test]
    fn a_word_is_written_as_the_engine_writes_it() {
        let palabra = &palabras([oida(" Pues", 1.0)], &Idioma::castellano())[0];
        let json = serde_json::to_value(palabra).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"i": 0, "ini": 1.0, "fin": 1.33, "txt": "Pues", "p": 0.88, "coletilla": true})
        );
    }
}
