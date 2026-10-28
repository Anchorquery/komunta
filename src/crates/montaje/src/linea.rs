//! The timeline the plan describes, in seconds: a port of
//! `construir_linea` in the Python engine's `motor/render.py`. Same plan,
//! same numbers - the golden test holds the two to it.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::Error;
use crate::entrada::{ClipAnalisis, Palabra, Trabajo};

/// Air before each piece of speech.
pub const ACOLCHADO_INI: f64 = 0.08;
/// Air after each piece of speech.
pub const ACOLCHADO_FIN: f64 = 0.18;
/// Pauses inside a take longer than this are squeezed...
pub const SILENCIO_MAX: f64 = 0.6;
/// ...down to this.
pub const SILENCIO_QUEDA: f64 = 0.25;
/// The shortest shot or B-roll worth showing.
pub const DUR_MIN_PLANO: f64 = 0.8;

/// A piece of a source placed on the voice line.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Trozo {
    /// Inventory id.
    pub clip: String,
    /// Absolute path.
    pub ruta: String,
    /// Start in the source.
    pub ini: f64,
    /// End in the source.
    pub fin: f64,
    /// Start on the timeline.
    pub t: f64,
    /// Index of the take it comes from.
    pub toma: usize,
    /// A shot (cut by seconds), not speech.
    pub plano: bool,
    /// A still.
    pub imagen: bool,
    /// A shot's sound: "silencio" or "ambiente".
    pub audio: Option<String>,
    /// Speech heard under someone else's picture.
    pub solo_audio: bool,
}

/// A caption with its times worked out.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sub {
    /// The words.
    pub texto: String,
    /// "normal", "enfasis" or "clave".
    pub estilo: String,
    /// "auto", "arriba", "centro" or "abajo".
    pub posicion: String,
    /// Start on the timeline.
    pub ini: f64,
    /// End on the timeline.
    pub fin: f64,
}

/// B-roll with its times worked out.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecursoLinea {
    /// Inventory id.
    pub clip: String,
    /// Absolute path.
    pub ruta: String,
    /// A still.
    pub imagen: bool,
    /// Start on the timeline.
    pub t: f64,
    /// Length.
    pub dur: f64,
    /// Start in the source.
    pub desde: f64,
    /// Hand-drawn mark, if any.
    pub garabato: Option<String>,
}

/// Background music.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MusicaLinea {
    /// Audio id.
    pub clip: String,
    /// Absolute path, when known.
    pub ruta: Option<String>,
    /// Gain under the voice.
    pub volumen: f64,
    /// Start in the track.
    pub desde_seg: f64,
}

/// Everything placed, in timeline seconds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Linea {
    /// The voice line.
    pub trozos: Vec<Trozo>,
    /// Captions, in order, without overlaps.
    pub subtitulos: Vec<Sub>,
    /// B-roll, in order, without overlaps.
    pub recursos: Vec<RecursoLinea>,
    /// Music, if any.
    pub musica: Option<MusicaLinea>,
    /// Length of the voice line.
    pub duracion_voz: f64,
}

/// Where each kept piece of a take went: (source start, source end,
/// timeline start).
type Mapa = Vec<Vec<(f64, f64, f64)>>;

/// The source intervals kept from a take, padded and with long pauses
/// squeezed.
pub fn trozos_de_toma(
    palabras: &[Palabra],
    desde: usize,
    hasta: usize,
    quitar: &[[usize; 2]],
) -> Vec<(f64, f64)> {
    let fuera: HashSet<usize> = quitar.iter().flat_map(|[a, b]| *a..=*b).collect();
    let fin_rango = (hasta + 1).min(palabras.len());
    let utiles: Vec<&Palabra> = palabras
        .get(desde.min(fin_rango)..fin_rango)
        .unwrap_or_default()
        .iter()
        .filter(|p| !fuera.contains(&p.i))
        .collect();
    let Some(primera) = utiles.first() else {
        return Vec::new();
    };
    let mut trozos = Vec::new();
    let mut ini = primera.ini - ACOLCHADO_INI;
    let mut ant = *primera;
    for p in &utiles[1..] {
        let hueco = p.ini - ant.fin;
        let salto = p.i != ant.i + 1;
        if salto || hueco > SILENCIO_MAX {
            let corte = if hueco > SILENCIO_MAX {
                SILENCIO_QUEDA / 2.0
            } else {
                0.03
            };
            trozos.push((ini, ant.fin + corte));
            ini = p.ini - corte;
        }
        ant = p;
    }
    trozos.push((ini, ant.fin + ACOLCHADO_FIN));
    trozos.into_iter().map(|(a, b)| (a.max(0.0), b)).collect()
}

/// Stretches or shortens a shot so its cut lands on the nearest beat, when
/// one is close.
fn al_pulso(t: f64, dur: f64, maximo: f64, pulsos: &[f64]) -> f64 {
    let fin = t + dur;
    let Some(candidato) = pulsos
        .iter()
        .copied()
        .min_by(|a, b| (a - fin).abs().total_cmp(&(b - fin).abs()))
    else {
        return dur;
    };
    let nueva = candidato - t;
    if (candidato - fin).abs() <= 0.4 && (DUR_MIN_PLANO..=maximo).contains(&nueva) {
        nueva
    } else {
        dur
    }
}

/// Builds the timeline a plan describes.
pub fn construir_linea(trabajo: &Trabajo) -> Result<Linea, Error> {
    let plan = &trabajo.plan;
    let inv = &trabajo.inventario;
    let clips: HashMap<&str, &ClipAnalisis> = trabajo
        .analisis
        .clips
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();
    let imagenes: HashSet<&str> = inv.imagenes.iter().map(|i| i.id.as_str()).collect();
    let rutas: HashMap<&str, &str> = inv
        .videos
        .iter()
        .map(|v| (v.id.as_str(), v.ruta.as_str()))
        .chain(
            inv.imagenes
                .iter()
                .map(|i| (i.id.as_str(), i.ruta.as_str())),
        )
        .chain(inv.audios.iter().map(|a| (a.id.as_str(), a.ruta.as_str())))
        .collect();
    let ruta = |id: &str| -> Result<String, Error> {
        rutas
            .get(id)
            .map(|r| (*r).to_owned())
            .ok_or_else(|| Error::Falta(id.to_owned()))
    };
    let clip = |id: &str| -> Result<&ClipAnalisis, Error> {
        clips
            .get(id)
            .copied()
            .ok_or_else(|| Error::Falta(id.to_owned()))
    };
    let palabras_de = |id: &str| -> Result<&[Palabra], Error> {
        Ok(clip(id)?
            .transcripcion
            .as_ref()
            .map(|t| t.palabras.as_slice())
            .unwrap_or_default())
    };

    // The music only counts when the analysis knows the track.
    let musica = plan.musica.as_ref().and_then(|m| {
        let id = m.clip.as_deref()?;
        let audio = trabajo.analisis.audios.iter().find(|a| a.id == id)?;
        Some((m, audio))
    });
    let pulsos: Vec<f64> = musica
        .map(|(m, audio)| {
            let desde = m.desde_seg.unwrap_or(0.0);
            audio.pulsos.iter().map(|p| p - desde).collect()
        })
        .unwrap_or_default();

    let mut trozos = Vec::new();
    let mut mapa: Mapa = Vec::new();
    let mut t = 0.0;
    for (k, toma) in plan.tomas.iter().enumerate() {
        let mut m = Vec::new();
        if toma.es_plano() {
            let es_imagen = imagenes.contains(toma.clip.as_str());
            let total = if es_imagen {
                3600.0
            } else {
                clip(&toma.clip)?.duracion
            };
            let a = toma
                .desde_seg
                .unwrap_or(0.0)
                .min(total - DUR_MIN_PLANO)
                .max(0.0);
            let mut dur = toma.dur.unwrap_or(2.0).min(total - a);
            if toma.al_ritmo.unwrap_or(true) {
                dur = al_pulso(t, dur, total - a, &pulsos);
            }
            trozos.push(Trozo {
                clip: toma.clip.clone(),
                ruta: ruta(&toma.clip)?,
                ini: a,
                fin: a + dur,
                t,
                toma: k,
                plano: true,
                imagen: es_imagen,
                audio: Some(toma.audio.clone().unwrap_or_else(|| "silencio".to_owned())),
                solo_audio: false,
            });
            m.push((a, a + dur, t));
            t += dur;
            mapa.push(m);
            continue;
        }
        let c = clip(&toma.clip)?;
        let (desde, hasta) = (toma.desde.unwrap_or(0), toma.hasta.unwrap_or(0));
        for (a, b) in trozos_de_toma(palabras_de(&toma.clip)?, desde, hasta, &toma.quitar) {
            let b = b.min(c.duracion);
            trozos.push(Trozo {
                clip: toma.clip.clone(),
                ruta: ruta(&toma.clip)?,
                ini: a,
                fin: b,
                t,
                toma: k,
                plano: false,
                imagen: false,
                audio: None,
                solo_audio: toma.solo_audio,
            });
            m.push((a, b, t));
            t += b - a;
        }
        mapa.push(m);
    }
    let duracion_voz = t;

    let toma_de = |k: usize| mapa.get(k).ok_or(Error::Toma(k));
    let inicio_toma =
        |k: usize| -> Result<f64, Error> { Ok(toma_de(k)?.first().map_or(0.0, |(_, _, t0)| *t0)) };
    let fin_toma = |k: usize| -> Result<f64, Error> {
        Ok(toma_de(k)?.last().map_or(0.0, |(a, b, t0)| t0 + b - a))
    };
    // Where word `i` of take `k` landed; a word that was cut takes the
    // nearest kept edge.
    let tiempo = |k: usize, i: usize, inicio: bool| -> Result<f64, Error> {
        let toma = plan.tomas.get(k).ok_or(Error::Toma(k))?;
        let p = palabras_de(&toma.clip)?
            .get(i)
            .ok_or_else(|| Error::Palabra(toma.clip.clone(), i))?;
        let s = if inicio { p.ini } else { p.fin };
        let piezas = toma_de(k)?;
        if let Some((a, _, t0)) = piezas.iter().find(|(a, b, _)| *a <= s && s <= *b) {
            return Ok(t0 + s - a);
        }
        let mejor = piezas
            .iter()
            .map(|(a, b, t0)| {
                if inicio {
                    ((s - a).abs(), *t0)
                } else {
                    ((s - b).abs(), t0 + b - a)
                }
            })
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
        Ok(mejor.map_or(0.0, |(_, t)| t))
    };

    let mut subs = Vec::new();
    for s in &plan.subtitulos {
        let (ini, fin) = match (s.desde, s.hasta) {
            (Some(desde), Some(hasta)) => {
                (tiempo(s.toma, desde, true)?, tiempo(s.toma, hasta, false)?)
            }
            _ => {
                // Over a shot without words: the whole take, or `dur` from `seg`.
                let ini = inicio_toma(s.toma)? + s.seg.unwrap_or(0.0);
                let fin = match s.dur {
                    Some(dur) if dur != 0.0 => (ini + dur).min(duracion_voz),
                    _ => fin_toma(s.toma)?,
                };
                (ini, fin)
            }
        };
        subs.push(Sub {
            texto: s.texto.clone(),
            estilo: s.estilo.clone().unwrap_or_else(|| "normal".to_owned()),
            posicion: s.posicion.clone().unwrap_or_else(|| "auto".to_owned()),
            ini,
            fin: fin.max(ini + 0.8),
        });
    }
    // No short gaps and no overlaps between captions.
    for n in 1..subs.len() {
        let (a_ini, a_fin, b_ini) = (subs[n - 1].ini, subs[n - 1].fin, subs[n].ini);
        let hueco = b_ini - a_fin;
        if (0.0..0.4).contains(&hueco) || b_ini < a_fin {
            subs[n - 1].fin = b_ini.max(a_ini + 0.3);
        }
    }
    if let Some(ultimo) = subs.last_mut() {
        ultimo.fin = (ultimo.fin + 0.3).min(duracion_voz);
    }

    let mut recursos = Vec::new();
    for r in &plan.recursos {
        let (t_ini, mut dur) = if let Some(n) = r.subtitulo {
            let s = subs.get(n).ok_or(Error::Subtitulo(n))?;
            (s.ini, (s.fin - s.ini).clamp(1.2, 3.0))
        } else {
            let k = r.toma.ok_or(Error::Anclaje(r.clip.clone()))?;
            let t_ini = match r.palabra {
                Some(i) => tiempo(k, i, true)?,
                None => inicio_toma(k)? + r.seg.unwrap_or(0.0),
            };
            let mut dur = r.dur.unwrap_or(2.0);
            // Does not run into the next take (e.g. the sign-off to camera).
            if plan.tomas.get(k).is_some_and(|toma| toma.solo_audio) && !toma_de(k)?.is_empty() {
                dur = dur.min(fin_toma(k)? - t_ini);
            }
            (t_ini, dur)
        };
        dur = dur.min(duracion_voz - t_ini);
        let es_imagen = imagenes.contains(r.clip.as_str());
        let mut desde = r.desde_seg.unwrap_or(0.0);
        if !es_imagen {
            let total = clip(&r.clip)?.duracion;
            dur = dur.min(total);
            desde = desde.min(total - dur - 0.05).max(0.0);
        }
        recursos.push(RecursoLinea {
            clip: r.clip.clone(),
            ruta: ruta(&r.clip)?,
            imagen: es_imagen,
            t: t_ini,
            dur,
            desde,
            garabato: r.garabato.clone(),
        });
    }
    // No overlaps: each piece ends where the next begins; what is left too
    // short goes.
    recursos.sort_by(|a, b| a.t.total_cmp(&b.t));
    let siguientes: Vec<Option<f64>> = (0..recursos.len())
        .map(|n| recursos.get(n + 1).map(|r| r.t))
        .collect();
    let recursos = recursos
        .into_iter()
        .zip(siguientes)
        .filter_map(|(mut r, siguiente)| {
            if let Some(t) = siguiente {
                r.dur = r.dur.min(t - r.t);
            }
            (r.dur >= DUR_MIN_PLANO).then_some(r)
        })
        .collect();

    let musica = musica.map(|(m, audio)| MusicaLinea {
        clip: audio.id.clone(),
        ruta: rutas
            .get(audio.id.as_str())
            .map(|r| (*r).to_owned())
            .or_else(|| audio.ruta.clone()),
        volumen: m.volumen.unwrap_or(0.16),
        desde_seg: m.desde_seg.unwrap_or(0.0),
    });

    Ok(Linea {
        trozos,
        subtitulos: subs,
        recursos,
        musica,
        duracion_voz,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palabras(tiempos: &[(f64, f64)]) -> Vec<Palabra> {
        tiempos
            .iter()
            .enumerate()
            .map(|(i, (ini, fin))| Palabra {
                i,
                ini: *ini,
                fin: *fin,
            })
            .collect()
    }

    #[test]
    fn a_take_is_padded_at_both_ends() {
        let p = palabras(&[(1.0, 1.4), (1.5, 1.9)]);
        let trozos = trozos_de_toma(&p, 0, 1, &[]);
        assert_eq!(trozos.len(), 1);
        assert!((trozos[0].0 - 0.92).abs() < 1e-9);
        assert!((trozos[0].1 - 2.08).abs() < 1e-9);
    }

    #[test]
    fn a_long_pause_is_squeezed() {
        let p = palabras(&[(1.0, 1.4), (3.0, 3.4)]);
        let trozos = trozos_de_toma(&p, 0, 1, &[]);
        assert_eq!(trozos.len(), 2);
        assert!((trozos[0].1 - 1.525).abs() < 1e-9);
        assert!((trozos[1].0 - 2.875).abs() < 1e-9);
    }

    #[test]
    fn removed_words_split_the_take() {
        let p = palabras(&[(1.0, 1.2), (1.3, 1.5), (1.6, 1.8)]);
        let trozos = trozos_de_toma(&p, 0, 2, &[[1, 1]]);
        assert_eq!(trozos.len(), 2);
        assert!((trozos[0].1 - 1.23).abs() < 1e-9);
        assert!((trozos[1].0 - 1.57).abs() < 1e-9);
    }

    #[test]
    fn the_first_pad_never_goes_before_zero() {
        let p = palabras(&[(0.02, 0.4)]);
        assert_eq!(trozos_de_toma(&p, 0, 0, &[])[0].0, 0.0);
    }

    #[test]
    fn a_cut_snaps_to_a_close_beat() {
        assert!((al_pulso(0.0, 2.0, 10.0, &[2.3]) - 2.3).abs() < 1e-9);
        assert_eq!(al_pulso(0.0, 2.0, 10.0, &[3.0]), 2.0);
        assert_eq!(al_pulso(0.0, 2.0, 10.0, &[]), 2.0);
    }
}
