//! How noisy a clip's room is, measured where nobody speaks, and the
//! clean-up its sound gets before whisper hears it.

use serde::{Deserialize, Serialize};

/// The rate the analysis hears audio at.
pub const MUESTREO: usize = 16_000;

/// Loudness is measured in 50 ms frames.
const MARCOS_POR_SEGUNDO: usize = 20;

/// With fewer silent frames than this, the whole clip is measured.
const MARCOS_FUERA_MIN: usize = 10;

/// The noise floor is the 20th percentile of the silent frames' loudness:
/// low enough to skip coughs and doors, high enough to be the room.
const PERCENTIL: f64 = 20.0;

/// A silent clip, or one too short to measure.
const SILENCIO_DB: f64 = -120.0;

/// Above this floor, by default, a clip is noisy.
pub const UMBRAL_DB: f64 = -48.0;

/// A clip's noise floor, as `analisis.json` keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ruido {
    /// The room's loudness where nobody speaks, in dBFS, to the tenth.
    pub suelo_db: f64,
    /// Loud enough to need noise reduction.
    pub ruidoso: bool,
}

impl Ruido {
    /// Measures `audio` (mono, at [`MUESTREO`]) outside the `voz` stretches
    /// (seconds), and calls it noisy above `umbral_db`.
    pub fn medir(audio: &[f32], voz: &[(f64, f64)], umbral_db: f64) -> Self {
        let marcos = sonoridad(audio);
        if marcos.is_empty() {
            return Self { suelo_db: SILENCIO_DB, ruidoso: false };
        }
        let fuera = fuera_de_la_voz(&marcos, voz);
        let medidos = if fuera.len() >= MARCOS_FUERA_MIN { fuera } else { marcos };
        let suelo = percentil(medidos, PERCENTIL);
        Self {
            suelo_db: (suelo * 10.0).round() / 10.0,
            ruidoso: suelo > umbral_db,
        }
    }

    /// The ffmpeg filters that ready the clip's sound for whisper, in
    /// order: rumble cut always, and the room's hiss taken down when it is
    /// noisy.
    pub fn filtros(&self) -> Vec<String> {
        let mut filtros = vec!["highpass=f=80".to_owned()];
        if self.ruidoso {
            let suelo = self.suelo_db.clamp(-80.0, -20.0);
            filtros.push(format!("afftdn=nr=12:nf={suelo:.0}:tn=1"));
        }
        filtros
    }
}

/// Each whole 50 ms frame's loudness, in dBFS.
fn sonoridad(audio: &[f32]) -> Vec<f64> {
    audio
        .chunks_exact(MUESTREO / MARCOS_POR_SEGUNDO)
        .map(|marco| {
            let energia = marco.iter().map(|&m| f64::from(m).powi(2)).sum::<f64>() / marco.len() as f64;
            20.0 * (energia + 1e-12).sqrt().log10()
        })
        .collect()
}

/// The frames no voice stretch touches.
fn fuera_de_la_voz(marcos: &[f64], voz: &[(f64, f64)]) -> Vec<f64> {
    let mut con_voz = vec![false; marcos.len()];
    for &(inicio, fin) in voz {
        let primero = marco_de(inicio).min(marcos.len());
        let ultimo = (marco_de(fin) + 1).min(marcos.len());
        con_voz[primero..ultimo.max(primero)].fill(true);
    }
    marcos
        .iter()
        .zip(con_voz)
        .filter(|(_, voz)| !voz)
        .map(|(&db, _)| db)
        .collect()
}

fn marco_de(segundos: f64) -> usize {
    (segundos * MARCOS_POR_SEGUNDO as f64) as usize
}

/// The `q`th percentile, interpolated between neighbours as numpy does.
fn percentil(mut valores: Vec<f64>, q: f64) -> f64 {
    valores.sort_by(f64::total_cmp);
    let posicion = q / 100.0 * (valores.len() - 1) as f64;
    let abajo = posicion.floor() as usize;
    let arriba = posicion.ceil() as usize;
    let peso = posicion - abajo as f64;
    valores[abajo] + (valores[arriba] - valores[abajo]) * peso
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARCO: usize = MUESTREO / MARCOS_POR_SEGUNDO;

    /// `segundos` of a steady tone whose frames all measure `db`.
    fn tono(db: f64, segundos: f64) -> Vec<f32> {
        let amplitud = 10f64.powf(db / 20.0) as f32;
        vec![amplitud; (segundos * MUESTREO as f64) as usize]
    }

    #[test]
    fn the_floor_is_measured_where_nobody_speaks() {
        let mut audio = tono(-60.0, 2.0);
        audio.extend(tono(-10.0, 2.0));
        let ruido = Ruido::medir(&audio, &[(2.0, 4.0)], UMBRAL_DB);
        assert_eq!(ruido, Ruido { suelo_db: -60.0, ruidoso: false });
    }

    #[test]
    fn a_noisy_room_is_flagged() {
        let ruido = Ruido::medir(&tono(-40.0, 3.0), &[], UMBRAL_DB);
        assert_eq!(ruido, Ruido { suelo_db: -40.0, ruidoso: true });
    }

    #[test]
    fn with_too_little_silence_the_whole_clip_counts() {
        let mut audio = tono(-70.0, 0.25);
        audio.extend(tono(-30.0, 3.0));
        let ruido = Ruido::medir(&audio, &[(0.3, 3.25)], UMBRAL_DB);
        let todo = percentil(sonoridad(&audio), PERCENTIL);
        assert_eq!(ruido.suelo_db, (todo * 10.0).round() / 10.0);
    }

    #[test]
    fn a_clip_shorter_than_a_frame_is_silent() {
        let ruido = Ruido::medir(&vec![0.5; MARCO - 1], &[], UMBRAL_DB);
        assert_eq!(ruido, Ruido { suelo_db: -120.0, ruidoso: false });
    }

    #[test]
    fn voice_past_the_end_is_harmless() {
        let ruido = Ruido::medir(&tono(-60.0, 1.0), &[(0.5, 99.0), (50.0, 60.0)], UMBRAL_DB);
        assert_eq!(ruido.suelo_db, -60.0);
    }

    #[test]
    fn percentiles_interpolate_like_numpy() {
        assert_eq!(percentil(vec![4.0, 1.0, 3.0, 2.0], 20.0), 1.6);
        assert_eq!(percentil(vec![7.0], 20.0), 7.0);
    }

    #[test]
    fn the_filters_clean_only_a_noisy_room() {
        let quieto = Ruido { suelo_db: -60.0, ruidoso: false };
        assert_eq!(quieto.filtros(), ["highpass=f=80"]);
        let ruidoso = Ruido { suelo_db: -42.3, ruidoso: true };
        assert_eq!(ruidoso.filtros(), ["highpass=f=80", "afftdn=nr=12:nf=-42:tn=1"]);
        let estruendo = Ruido { suelo_db: -5.0, ruidoso: true };
        assert_eq!(estruendo.filtros()[1], "afftdn=nr=12:nf=-20:tn=1");
    }

    #[test]
    fn noise_is_written_as_the_engine_writes_it() {
        let json = serde_json::to_value(Ruido { suelo_db: -51.2, ruidoso: false }).unwrap();
        assert_eq!(json, serde_json::json!({"suelo_db": -51.2, "ruidoso": false}));
    }
}
