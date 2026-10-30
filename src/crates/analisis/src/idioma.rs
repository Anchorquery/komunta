//! Which language a clip is in, and its fillers.
//!
//! Whisper's own guess is good for long Spanish takes and poor for short
//! Basque ones: "Pausoz pauso" comes back as Spanish, unsure. So a Basque
//! guess goes to the Basque model, an unsure Spanish one is heard by both
//! models, and the transcription its model trusts more wins.

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

/// Which models hear a clip, given whisper's guess at its language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reparto {
    /// The model that guessed, and that is that.
    Adivinador,
    /// Guessed Basque: the Basque model, alone.
    Euskera,
    /// Guessed Spanish, unsure: both, keeping whichever its model trusts
    /// more ([`gana_la_segunda`]).
    Ambos,
}

/// The language of a clip from whisper's guess over all of them: Spanish
/// or Basque, whichever it finds likelier, and how sure it is between the
/// two. The team shoots in nothing else, and a second of speech can come
/// out German at 20% - and then be heard, and written, in German.
pub fn entre_los_dos(castellano: f64, euskera: f64) -> (Idioma, f64) {
    let total = castellano + euskera;
    if total <= 0.0 {
        (Idioma::castellano(), 0.0)
    } else if euskera > castellano {
        (Idioma::euskera(), euskera / total)
    } else {
        (Idioma::castellano(), castellano / total)
    }
}

/// Who hears a clip whose language whisper guessed. With one model serving
/// both languages, there is nothing to choose.
pub fn reparto(adivinado: &Idioma, probabilidad: f64, un_solo_modelo: bool) -> Reparto {
    if un_solo_modelo {
        return Reparto::Adivinador;
    }
    match adivinado.codigo() {
        "eu" => Reparto::Euskera,
        "es" if probabilidad < DUDA => Reparto::Ambos,
        _ => Reparto::Adivinador,
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
    fn the_guess_is_spanish_or_basque() {
        let (idioma, seguro) = entre_los_dos(0.06, 0.02);
        assert_eq!(idioma, Idioma::castellano());
        assert!((seguro - 0.75).abs() < 1e-9);
        assert_eq!(entre_los_dos(0.1, 0.3).0, Idioma::euskera());
        assert_eq!(entre_los_dos(0.0, 0.0), (Idioma::castellano(), 0.0));
    }

    #[test]
    fn a_basque_guess_goes_to_the_basque_model() {
        assert_eq!(reparto(&Idioma::euskera(), 0.99, false), Reparto::Euskera);
    }

    #[test]
    fn an_unsure_spanish_guess_is_checked_against_basque() {
        assert_eq!(reparto(&Idioma::castellano(), 0.6, false), Reparto::Ambos);
        assert_eq!(reparto(&Idioma::castellano(), 0.95, false), Reparto::Adivinador);
        assert_eq!(reparto(&Idioma::from("fr"), 0.3, false), Reparto::Adivinador);
    }

    #[test]
    fn one_model_serves_whatever_the_guess() {
        assert_eq!(reparto(&Idioma::euskera(), 0.5, true), Reparto::Adivinador);
        assert_eq!(reparto(&Idioma::castellano(), 0.5, true), Reparto::Adivinador);
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
