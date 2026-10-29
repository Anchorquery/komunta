//! Which language a clip is in, and its fillers.
//!
//! Whisper's own guess is good for long Spanish takes and poor for short
//! Basque ones: "Pausoz pauso" comes back as Spanish, unsure. So a Basque
//! guess is redone with the Basque model, an unsure Spanish one is heard
//! by both models, and the transcription its model trusts more wins.

/// A clip's language, as whisper names it ("es", "eu", ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Idioma(String);

/// Below this, a "Spanish" guess is checked against the Basque model.
const DUDA: f64 = 0.9;

const COLETILLAS_ES: &[&str] = &["eh", "ehm", "em", "mm", "pues", "vale", "bueno", "osea", "este", "eeh"];
const COLETILLAS_EU: &[&str] = &["eh", "ehm", "ba", "beno", "bueno", "ze", "mm", "eeh"];

impl Idioma {
    /// Spanish.
    pub fn castellano() -> Self {
        Self::from("es")
    }

    /// Basque.
    pub fn euskera() -> Self {
        Self::from("eu")
    }

    /// Whisper's code for it.
    pub fn codigo(&self) -> &str {
        &self.0
    }

    /// Whether `palabra`, already normalised, is a filler in this language.
    /// Only Spanish and Basque have a list.
    pub fn es_coletilla(&self, palabra: &str) -> bool {
        let lista: &[&str] = match self.codigo() {
            "es" => COLETILLAS_ES,
            "eu" => COLETILLAS_EU,
            _ => &[],
        };
        lista.contains(&palabra)
    }
}

impl From<&str> for Idioma {
    fn from(codigo: &str) -> Self {
        Self(codigo.to_owned())
    }
}

/// What to do after whisper's first pass, which guessed the language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segunda {
    /// The first pass stands.
    Ninguna,
    /// Heard as Basque: hear it again with the Basque model, and keep that.
    Euskera,
    /// Heard as Spanish, unsure: hear it with the Basque model too, and keep
    /// whichever its model trusts more ([`gana_la_segunda`]).
    Contrastar,
}

/// The second pass a first one calls for. None when the language was
/// given, or when one model serves both languages.
pub fn segunda_pasada(adivinado: &Idioma, probabilidad: f64, dado: bool, un_solo_modelo: bool) -> Segunda {
    if dado || un_solo_modelo {
        return Segunda::Ninguna;
    }
    match adivinado.codigo() {
        "eu" => Segunda::Euskera,
        "es" if probabilidad < DUDA => Segunda::Contrastar,
        _ => Segunda::Ninguna,
    }
}

/// A stretch of a transcription, as far as trusting it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tramo {
    /// Whisper's mean log-probability over the stretch's tokens.
    pub logprob_medio: f64,
    /// How many words it holds.
    pub palabras: usize,
}

/// How much a model trusts its transcription: the mean log-probability,
/// weighted by words. With no words, none at all.
pub fn confianza(tramos: &[Tramo]) -> f64 {
    let palabras: usize = tramos.iter().map(|tramo| tramo.palabras).sum();
    if palabras == 0 {
        return f64::NEG_INFINITY;
    }
    let suma: f64 = tramos.iter().map(|tramo| tramo.logprob_medio * tramo.palabras as f64).sum();
    suma / palabras as f64
}

/// Whether the second transcription beats the first.
pub fn gana_la_segunda(primera: &[Tramo], segunda: &[Tramo]) -> bool {
    confianza(segunda) > confianza(primera)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_basque_guess_is_heard_again_in_basque() {
        assert_eq!(segunda_pasada(&Idioma::euskera(), 0.99, false, false), Segunda::Euskera);
    }

    #[test]
    fn an_unsure_spanish_guess_is_checked_against_basque() {
        assert_eq!(segunda_pasada(&Idioma::castellano(), 0.6, false, false), Segunda::Contrastar);
        assert_eq!(segunda_pasada(&Idioma::castellano(), 0.95, false, false), Segunda::Ninguna);
        assert_eq!(segunda_pasada(&Idioma::from("fr"), 0.3, false, false), Segunda::Ninguna);
    }

    #[test]
    fn no_second_pass_when_given_or_with_one_model() {
        assert_eq!(segunda_pasada(&Idioma::euskera(), 0.5, true, false), Segunda::Ninguna);
        assert_eq!(segunda_pasada(&Idioma::castellano(), 0.5, false, true), Segunda::Ninguna);
    }

    #[test]
    fn trust_is_weighted_by_words() {
        let tramos = [
            Tramo { logprob_medio: -0.2, palabras: 3 },
            Tramo { logprob_medio: -1.0, palabras: 1 },
        ];
        assert!((confianza(&tramos) - -0.4).abs() < 1e-12);
        assert_eq!(confianza(&[]), f64::NEG_INFINITY);
        assert_eq!(confianza(&[Tramo { logprob_medio: -0.1, palabras: 0 }]), f64::NEG_INFINITY);
    }

    #[test]
    fn the_more_trusted_transcription_wins() {
        let es = [Tramo { logprob_medio: -0.9, palabras: 2 }];
        let eu = [Tramo { logprob_medio: -0.3, palabras: 2 }];
        assert!(gana_la_segunda(&es, &eu));
        assert!(!gana_la_segunda(&eu, &es));
        assert!(!gana_la_segunda(&es, &[]), "a silent pass never wins");
    }
}
