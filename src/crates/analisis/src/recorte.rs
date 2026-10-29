//! A clip's speech cut out and joined up for whisper, and the way back
//! from the joined-up times to the clip's own.
//!
//! Whisper fed long silences makes words up in them, so it hears only the
//! stretches with a voice, end to end. What it hears is timed on that
//! shorter sound; each word goes back to the clip by the stretch its middle
//! falls in, as faster-whisper restores them.

use crate::PalabraOida;
use crate::ruido::MUESTREO;

/// Where each kept stretch sits, in the joined sound and in the clip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapa {
    /// Per stretch: (start in the joined sound, start in the clip), seconds.
    tramos: Vec<(f64, f64)>,
}

/// The speech stretches of `audio` (16 kHz mono) joined end to end, and the
/// map back. `voz` is in seconds, in order and apart.
pub fn recortar(audio: &[f32], voz: &[(f64, f64)]) -> (Vec<f32>, Mapa) {
    let mut unido = Vec::new();
    let mut mapa = Mapa::default();
    for &(inicio, fin) in voz {
        let desde = muestra(inicio).min(audio.len());
        let hasta = muestra(fin).min(audio.len());
        if hasta <= desde {
            continue;
        }
        mapa.tramos.push((segundos(unido.len()), segundos(desde)));
        unido.extend_from_slice(&audio[desde..hasta]);
    }
    (unido, mapa)
}

impl Mapa {
    /// `palabra`, timed on the joined sound, timed on the clip instead.
    pub fn devolver(&self, palabra: PalabraOida) -> PalabraOida {
        let Some(&(unido, original)) = self.tramo_de((palabra.inicio + palabra.fin) / 2.0) else {
            return palabra;
        };
        let desplazamiento = original - unido;
        PalabraOida {
            inicio: palabra.inicio + desplazamiento,
            fin: palabra.fin + desplazamiento,
            ..palabra
        }
    }

    /// The last stretch starting at or before `instante` in the joined sound.
    fn tramo_de(&self, instante: f64) -> Option<&(f64, f64)> {
        let siguientes = self.tramos.partition_point(|&(inicio, _)| inicio <= instante);
        self.tramos.get(siguientes.saturating_sub(1))
    }
}

fn muestra(segundos: f64) -> usize {
    (segundos * MUESTREO as f64).round().max(0.0) as usize
}

fn segundos(muestras: usize) -> f64 {
    muestras as f64 / MUESTREO as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palabra(inicio: f64, fin: f64) -> PalabraOida {
        PalabraOida { inicio, fin, texto: " hola".into(), probabilidad: 1.0 }
    }

    #[test]
    fn only_the_voice_is_kept() {
        let audio: Vec<f32> = (0..MUESTREO * 4).map(|n| (n / MUESTREO) as f32).collect();
        let (unido, _) = recortar(&audio, &[(1.0, 2.0), (3.0, 3.5)]);
        assert_eq!(unido.len(), MUESTREO * 3 / 2);
        assert_eq!((unido[0], unido[MUESTREO]), (1.0, 3.0));
    }

    #[test]
    fn words_go_back_by_the_stretch_their_middle_is_in() {
        let audio = vec![0.0; MUESTREO * 10];
        let (_, mapa) = recortar(&audio, &[(1.0, 3.0), (6.0, 8.0)]);
        assert_eq!(mapa.devolver(palabra(0.5, 1.0)).inicio, 1.5);
        let segunda = mapa.devolver(palabra(2.5, 3.0));
        assert_eq!((segunda.inicio, segunda.fin), (6.5, 7.0));
        let a_caballo = mapa.devolver(palabra(1.8, 2.4));
        assert!((a_caballo.inicio - 5.8).abs() < 1e-9, "its middle is in the second stretch");
        assert!((a_caballo.fin - 6.4).abs() < 1e-9);
    }

    #[test]
    fn stretches_past_the_end_or_empty_are_skipped() {
        let audio = vec![0.0; MUESTREO];
        let (unido, mapa) = recortar(&audio, &[(0.5, 0.5), (0.5, 9.0), (2.0, 3.0)]);
        assert_eq!(unido.len(), MUESTREO / 2);
        assert_eq!(mapa.tramos, [(0.0, 0.5)]);
    }

    #[test]
    fn with_no_voice_words_stay_put() {
        let mapa = Mapa::default();
        assert_eq!(mapa.devolver(palabra(1.0, 2.0)), palabra(1.0, 2.0));
    }
}
