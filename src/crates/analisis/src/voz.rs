//! Where in a clip somebody speaks, and whether that makes it a take: a
//! clip mostly spoken to camera, rather than a cutaway.

/// The stretches the voice detector heard speech in, in seconds, rounded
/// to the hundredth as `analisis.json` keeps them.
pub fn tramos(segundos: impl IntoIterator<Item = (f64, f64)>) -> Vec<[f64; 2]> {
    segundos
        .into_iter()
        .map(|(inicio, fin)| [centesimas(inicio), centesimas(fin)])
        .collect()
}

/// How much of the clip is speech, 0 to 1 and rounded to the hundredth.
pub fn ratio(voz: &[[f64; 2]], duracion: f64) -> f64 {
    // A fold from +0: an empty f64 sum is -0, which the JSON would keep.
    let hablado = voz
        .iter()
        .fold(0.0, |total, [inicio, fin]| total + fin - inicio);
    centesimas(hablado / duracion.max(0.01))
}

/// A clip with at least `umbral` of speech is a take, and is transcribed.
pub fn es_toma(ratio: f64, umbral: f64) -> bool {
    ratio >= umbral
}

fn centesimas(valor: f64) -> f64 {
    (valor * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretches_are_rounded() {
        assert_eq!(tramos([(0.123, 1.456)]), [[0.12, 1.46]]);
    }

    #[test]
    fn no_speech_is_a_plain_zero() {
        assert!(ratio(&[], 3.0).is_sign_positive());
    }

    #[test]
    fn the_ratio_is_speech_over_length() {
        let voz = [[0.0, 2.0], [3.0, 4.0]];
        assert_eq!(ratio(&voz, 6.0), 0.5);
        assert_eq!(ratio(&[], 6.0), 0.0);
        assert_eq!(ratio(&[[0.0, 0.5]], 0.0), 50.0, "no division by zero");
    }

    #[test]
    fn a_take_has_enough_speech() {
        assert!(es_toma(0.35, 0.35));
        assert!(!es_toma(0.34, 0.35));
    }
}
