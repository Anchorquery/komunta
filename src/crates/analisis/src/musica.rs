//! A song's tempo and beats, for cutting to the music: where the sound
//! changes most (spectral flux), how often that repeats (autocorrelation
//! between 70 and 180 BPM), and the grid of beats that lands on it best.

use serde::{Deserialize, Serialize};

/// The rate a song is heard at for this: plenty for drums.
pub const MUESTREO: usize = 11_025;

/// Samples between two looks at the spectrum.
const SALTO: usize = 128;
/// Samples in one look.
const VENTANA: usize = 1024;

/// The tempo range looked in.
const BPM_MIN: f64 = 70.0;
const BPM_MAX: f64 = 180.0;

/// A song's tempo and beats, as `analisis.json` keeps them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ritmo {
    /// Beats per minute, to the tenth; None for a song too short to tell.
    pub bpm: Option<f64>,
    /// Each beat, in seconds from the start, to the millisecond.
    pub pulsos: Vec<f64>,
}

impl Ritmo {
    /// The rhythm of `audio`: mono, at [`MUESTREO`].
    pub fn medir(audio: &[f32]) -> Self {
        if audio.len() < MUESTREO * 2 {
            return Self::default();
        }
        let flujo = normalizar(flujo(audio));
        let por_segundo = MUESTREO as f64 / SALTO as f64;
        let (desde, hasta) = (
            (por_segundo * 60.0 / BPM_MAX) as usize,
            (por_segundo * 60.0 / BPM_MIN) as usize + 2,
        );
        let autocorrelacion: Vec<f64> = (desde..hasta)
            .map(|retardo| correlacion(&flujo, retardo))
            .collect();
        let pico = (1..autocorrelacion.len() - 1)
            .max_by(|&a, &b| {
                autocorrelacion[a]
                    .total_cmp(&autocorrelacion[b])
                    .then(b.cmp(&a))
            })
            .unwrap_or(1);
        let periodo = (desde + pico) as f64 + vertice(&autocorrelacion[pico - 1..=pico + 1]);
        let fase = mejor_fase(&flujo, periodo);
        // Flux sample f compares windows f and f + 1: the hit falls towards
        // the middle of the second.
        let desfase = SALTO as f64 / MUESTREO as f64 + VENTANA as f64 / (2.0 * MUESTREO as f64);
        let duracion = audio.len() as f64 / MUESTREO as f64;
        let pulsos = rejilla(fase, periodo, flujo.len() - 1)
            .map(|f| ((f * SALTO as f64 / MUESTREO as f64 + desfase) * 1000.0).round() / 1000.0)
            .filter(|&t| t <= duracion)
            .collect();
        Self {
            bpm: Some((60.0 * por_segundo / periodo * 10.0).round() / 10.0),
            pulsos,
        }
    }
}

/// How much louder each band got from one look to the next, summed over
/// the bands that did: where notes and hits begin.
fn flujo(audio: &[f32]) -> Vec<f64> {
    let hann: Vec<f64> = (0..VENTANA)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (VENTANA - 1) as f64).cos())
        .collect();
    let espectros: Vec<Vec<f64>> = (0..=(audio.len() - VENTANA) / SALTO)
        .map(|n| {
            let mut marco: Vec<(f64, f64)> = audio[n * SALTO..n * SALTO + VENTANA]
                .iter()
                .zip(&hann)
                .map(|(muestra, peso)| (f64::from(*muestra) * peso, 0.0))
                .collect();
            fft(&mut marco);
            marco[..=VENTANA / 2]
                .iter()
                .map(|(re, im)| re.hypot(*im).ln_1p())
                .collect()
        })
        .collect();
    espectros
        .windows(2)
        .map(|par| {
            par[1]
                .iter()
                .zip(&par[0])
                .map(|(ahora, antes)| (ahora - antes).max(0.0))
                .sum()
        })
        .collect()
}

/// `valores` with mean 0 and deviation 1.
fn normalizar(valores: Vec<f64>) -> Vec<f64> {
    let n = valores.len().max(1) as f64;
    let media = valores.iter().sum::<f64>() / n;
    let desviacion = (valores.iter().map(|v| (v - media).powi(2)).sum::<f64>() / n).sqrt();
    let desviacion = if desviacion > 0.0 { desviacion } else { 1.0 };
    valores
        .into_iter()
        .map(|v| (v - media) / desviacion)
        .collect()
}

/// How alike `serie` is to itself `retardo` samples later, per pair.
fn correlacion(serie: &[f64], retardo: usize) -> f64 {
    if retardo >= serie.len() {
        return 0.0;
    }
    let suma: f64 = serie
        .iter()
        .zip(&serie[retardo..])
        .map(|(a, b)| a * b)
        .sum();
    suma / (serie.len() - retardo) as f64
}

/// Where between three samples their parabola peaks, from -0.5 to 0.5
/// around the middle one.
fn vertice(tres: &[f64]) -> f64 {
    let [a, b, c] = [tres[0], tres[1], tres[2]];
    let curva = a - 2.0 * b + c;
    if curva == 0.0 {
        0.0
    } else {
        0.5 * (a - c) / curva
    }
}

/// The offset, in quarter flux samples, where a beat grid of `periodo`
/// lands on the most flux.
fn mejor_fase(flujo: &[f64], periodo: f64) -> f64 {
    let pasos = (periodo / 0.25).ceil() as usize;
    let fuerza = |fase: f64| -> f64 {
        rejilla(fase, periodo, flujo.len() - 1)
            .map(|f| flujo[f.round() as usize])
            .sum()
    };
    (0..pasos)
        .map(|n| n as f64 * 0.25)
        .filter(|fase| *fase < periodo)
        .max_by(|a, b| fuerza(*a).total_cmp(&fuerza(*b)).then(b.total_cmp(a)))
        .unwrap_or(0.0)
}

/// `fase`, `fase + periodo`, ... below `hasta`.
fn rejilla(fase: f64, periodo: f64, hasta: usize) -> impl Iterator<Item = f64> {
    (0..)
        .map(move |n| fase + n as f64 * periodo)
        .take_while(move |f| f.round() < hasta as f64)
}

/// In-place radix-2 FFT of a power-of-two length.
fn fft(datos: &mut [(f64, f64)]) {
    let n = datos.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            datos.swap(i, j);
        }
    }
    let mut largo = 2;
    while largo <= n {
        let angulo = -2.0 * std::f64::consts::PI / largo as f64;
        for inicio in (0..n).step_by(largo) {
            for k in 0..largo / 2 {
                let (seno, coseno) = (angulo * k as f64).sin_cos();
                let (a, b) = (datos[inicio + k], datos[inicio + k + largo / 2]);
                let giro = (b.0 * coseno - b.1 * seno, b.0 * seno + b.1 * coseno);
                datos[inicio + k] = (a.0 + giro.0, a.1 + giro.1);
                datos[inicio + k + largo / 2] = (a.0 - giro.0, a.1 - giro.1);
            }
        }
        largo <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicks at `bpm` for `segundos`, starting at `primero` seconds.
    fn metronomo(bpm: f64, segundos: f64, primero: f64) -> Vec<f32> {
        let mut audio = vec![0.0f32; (segundos * MUESTREO as f64) as usize];
        let mut t = primero;
        while t < segundos {
            let inicio = (t * MUESTREO as f64) as usize;
            for (i, muestra) in audio.iter_mut().skip(inicio).take(200).enumerate() {
                *muestra = (i as f32 * 0.9).sin() * (1.0 - i as f32 / 200.0);
            }
            t += 60.0 / bpm;
        }
        audio
    }

    #[test]
    fn the_fft_finds_a_pure_tone() {
        let mut datos: Vec<(f64, f64)> = (0..64)
            .map(|i| {
                (
                    (2.0 * std::f64::consts::PI * 5.0 * i as f64 / 64.0).cos(),
                    0.0,
                )
            })
            .collect();
        fft(&mut datos);
        let magnitudes: Vec<f64> = datos.iter().map(|(re, im)| re.hypot(*im)).collect();
        let pico = (0..32)
            .max_by(|&a, &b| magnitudes[a].total_cmp(&magnitudes[b]))
            .unwrap();
        assert_eq!(pico, 5);
        assert!((magnitudes[5] - 32.0).abs() < 1e-9);
    }

    #[test]
    fn a_metronome_is_heard_at_its_tempo_and_on_its_clicks() {
        let ritmo = Ritmo::medir(&metronomo(120.0, 20.0, 0.25));
        let bpm = ritmo.bpm.unwrap();
        assert!((bpm - 120.0).abs() < 1.5, "{bpm} BPM");
        let primero = ritmo.pulsos[0];
        let lejos = (primero - 0.25)
            .rem_euclid(0.5)
            .min((0.25 - primero).rem_euclid(0.5));
        assert!(
            lejos < 0.05,
            "the first beat, {primero} s, is off the clicks"
        );
    }

    #[test]
    fn a_short_sound_has_no_tempo() {
        assert_eq!(Ritmo::medir(&[0.0; 1000]), Ritmo::default());
    }

    #[test]
    fn the_peak_between_samples_is_found() {
        assert_eq!(vertice(&[1.0, 2.0, 1.0]), 0.0);
        assert!(vertice(&[1.0, 2.0, 1.9]) > 0.3);
    }
}
