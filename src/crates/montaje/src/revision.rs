//! The montage's quality check: the rules of the Python engine's
//! `motor/revision.py` that need no AI and no rendered video, run on the
//! timeline ([`Linea`]) the plan lays out. Each warning says whether the
//! AI could put it right and, if so, carries the instruction to send it
//! (the second pass).
//!
//! Left out: faces and bright backgrounds under captions (they looked at
//! frames of the render) and overlapping B-roll, which [`Linea`] already
//! trims away.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::entrada::{Palabra, Trabajo};
use crate::linea::Linea;

/// Longest stretch of someone talking to camera without B-roll.
pub const MAX_PERSONA_SIN_RECURSO: f64 = 6.0;
/// Shortest caption worth reading.
pub const SUBTITULO_MIN: f64 = 0.7;
/// Longest caption read at a glance.
pub const SUBTITULO_MAX: f64 = 5.5;
/// A voice-over gap shorter than this is not worth a warning.
const TOLERANCIA_SOLO_AUDIO: f64 = 0.15;
/// How much of what is said a warning quotes, in letters.
const CITA_MAX: usize = 140;

/// What a warning is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    /// Someone talking too long with nothing over them.
    SinRecurso,
    /// Voice-over with no picture: the audio clip's own picture shows.
    VozEnOffSinImagen,
    /// A caption gone before it can be read.
    SubtituloCorto,
    /// A caption too long to read at a glance.
    SubtituloLargo,
    /// The same B-roll more than once.
    RecursoRepetido,
    /// A file of the material that could not be used.
    Descartado,
}

/// One thing the check found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Aviso {
    /// What it is about.
    pub tipo: Tipo,
    /// For the team.
    pub texto: String,
    /// For the AI, when it can put it right.
    pub instruccion: Option<String>,
    /// How bad it is; the score adds these up.
    pub peso: f64,
}

impl Aviso {
    fn nuevo(tipo: Tipo, texto: String, instruccion: Option<String>, peso: f64) -> Aviso {
        Aviso {
            tipo,
            texto,
            instruccion,
            peso: redondear(peso, 2),
        }
    }

    /// Whether a second pass of the AI could fix it.
    pub fn corregible(&self) -> bool {
        self.instruccion.is_some()
    }
}

/// Every warning for the plan of `trabajo`, laid out as `linea`.
pub fn revisar(trabajo: &Trabajo, linea: &Linea) -> Vec<Aviso> {
    let mut avisos = Vec::new();
    avisos.extend(sin_recurso(trabajo, linea));
    avisos.extend(voz_en_off_sin_imagen(trabajo, linea));
    avisos.extend(subtitulos(linea));
    avisos.extend(descartados(trabajo));
    avisos.extend(repetidos(linea));
    avisos
}

/// Lower is better. Only what the AI can fix counts: the rest is the same
/// in any version of the plan.
pub fn puntuacion(avisos: &[Aviso]) -> f64 {
    let suma: f64 = avisos.iter().filter(|a| a.corregible()).map(|a| a.peso).sum();
    redondear(suma, 2)
}

/// The message asking the AI to fix what it can, and nothing else.
pub fn mensaje_correccion(avisos: &[Aviso]) -> String {
    let puntos: Vec<&str> = avisos.iter().filter_map(|a| a.instruccion.as_deref()).collect();
    format!(
        "Revisión automática del vídeo montado con tu plan. Corrige SOLO estos puntos, sin cambiar \
         lo demás, y devuelve el plan completo en JSON con el mismo formato:\n- {}",
        puntos.join("\n- ")
    )
}

fn sin_recurso(trabajo: &Trabajo, linea: &Linea) -> Vec<Aviso> {
    tramos_sin_recurso(linea)
        .into_iter()
        .map(|(a, b)| {
            let d = b - a;
            Aviso::nuevo(
                Tipo::SinRecurso,
                format!("Persona hablando {d:.1} s seguidos sin recurso ({a:.1}–{b:.1} s)."),
                Some(format!(
                    "Entre {a:.1} y {b:.1} s se ve a la persona hablando {d:.1} s seguidos (máximo \
                     ~6 s). Añade 1 o 2 recursos que ilustren lo que se dice ahí, anclados con \
                     toma/palabra: {}.",
                    palabras_en_tramo(trabajo, linea, a, b)
                )),
                d / 3.0,
            )
        })
        .collect()
}

fn voz_en_off_sin_imagen(trabajo: &Trabajo, linea: &Linea) -> Vec<Aviso> {
    huecos_solo_audio(linea)
        .into_iter()
        .map(|(a, b)| {
            let d = b - a;
            Aviso::nuevo(
                Tipo::VozEnOffSinImagen,
                format!(
                    "VOZ EN OFF SIN IMAGEN {d:.1} s ({a:.1}–{b:.1} s): se vería la imagen del \
                     clip de audio."
                ),
                Some(format!(
                    "La voz en off queda sin imagen entre {a:.1} y {b:.1} s. Cubre ese tramo con \
                     recursos encadenados: {}.",
                    palabras_en_tramo(trabajo, linea, a, b)
                )),
                3.0 + d,
            )
        })
        .collect()
}

/// Captions too short or too long to read. Piled ones stay on screen to
/// the end of their group, so their own length says nothing.
fn subtitulos(linea: &Linea) -> Vec<Aviso> {
    let mut avisos = Vec::new();
    for (n, s) in linea.subtitulos.iter().enumerate() {
        if s.apilar.is_some() {
            continue;
        }
        let (d, texto) = (s.fin - s.ini, &s.texto);
        if d < SUBTITULO_MIN {
            avisos.push(Aviso::nuevo(
                Tipo::SubtituloCorto,
                format!("Subtítulo {n} «{texto}» dura solo {d:.1} s."),
                Some(format!(
                    "El subtítulo {n} («{texto}») dura {d:.1} s: amplía su rango de palabras o \
                     únelo al siguiente."
                )),
                1.0,
            ));
        }
        if d > SUBTITULO_MAX {
            avisos.push(Aviso::nuevo(
                Tipo::SubtituloLargo,
                format!("Subtítulo {n} «{texto}» dura {d:.1} s (largo para leer de un vistazo)."),
                Some(format!(
                    "El subtítulo {n} («{texto}») dura {d:.1} s: divídelo en dos subtítulos \
                     consecutivos (parte el texto por donde tenga sentido y reparte sus palabras \
                     desde/hasta)."
                )),
                1.0,
            ));
        }
    }
    avisos
}

fn descartados(trabajo: &Trabajo) -> Vec<Aviso> {
    trabajo
        .inventario
        .descartados
        .iter()
        .map(|d| {
            Aviso::nuevo(
                Tipo::Descartado,
                format!("Archivo descartado: {} — {}", d.archivo, d.motivo),
                None,
                0.0,
            )
        })
        .collect()
}

fn repetidos(linea: &Linea) -> Vec<Aviso> {
    let mut veces: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &linea.recursos {
        *veces.entry(r.clip.as_str()).or_default() += 1;
    }
    veces
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(clip, n)| {
            Aviso::nuevo(
                Tipo::RecursoRepetido,
                format!("Recurso repetido: {clip}."),
                Some(format!(
                    "El recurso {clip} aparece {n} veces: sustituye las repeticiones por otros \
                     recursos sin usar."
                )),
                1.0,
            )
        })
        .collect()
}

/// Stretches of someone talking with no picture over them, to a tenth of a
/// second. Shots (no speech) count as picture.
pub fn tramos_sin_recurso(linea: &Linea) -> Vec<(f64, f64)> {
    let mut cubiertos: Vec<(f64, f64)> = linea
        .recursos
        .iter()
        .map(|r| (r.t, r.t + r.dur))
        .chain(
            linea
                .trozos
                .iter()
                .filter(|tr| tr.plano)
                .map(|tr| (tr.t, tr.t + tr.fin - tr.ini)),
        )
        .collect();
    cubiertos.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
    cubiertos.push((linea.duracion_voz, linea.duracion_voz));
    let mut huecos = Vec::new();
    let mut t: f64 = 0.0;
    for (a, b) in cubiertos {
        if a - t > MAX_PERSONA_SIN_RECURSO {
            huecos.push((decima(t), decima(a)));
        }
        t = t.max(b);
    }
    huecos
}

/// Stretches of voice-over (speech heard under someone else's picture)
/// that no B-roll covers, to a tenth of a second.
pub fn huecos_solo_audio(linea: &Linea) -> Vec<(f64, f64)> {
    let mut cubiertos: Vec<(f64, f64)> = linea.recursos.iter().map(|r| (r.t, r.t + r.dur)).collect();
    cubiertos.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
    let mut huecos = Vec::new();
    for tr in linea.trozos.iter().filter(|tr| tr.solo_audio) {
        let (mut t, fin) = (tr.t, tr.t + tr.fin - tr.ini);
        for &(a, b) in &cubiertos {
            if b <= t || a >= fin {
                continue;
            }
            if a - t > TOLERANCIA_SOLO_AUDIO {
                huecos.push((decima(t), decima(a)));
            }
            t = t.max(b);
        }
        if fin - t > TOLERANCIA_SOLO_AUDIO {
            huecos.push((decima(t), decima(fin)));
        }
    }
    huecos
}

/// What is said between `a` and `b` on the timeline, as the plan names it:
/// «toma 2 (IMG_8443), palabras 0–8: "EH Bildu…"».
pub fn palabras_en_tramo(trabajo: &Trabajo, linea: &Linea, a: f64, b: f64) -> String {
    let tomas = &trabajo.plan.tomas;
    let mut por_toma: BTreeMap<usize, Vec<&Palabra>> = BTreeMap::new();
    for tr in &linea.trozos {
        let Some(toma) = tomas.get(tr.toma) else {
            continue;
        };
        let dichas = palabras(trabajo, &toma.clip).iter().filter(|p| {
            let en_linea = tr.t + p.ini - tr.ini;
            (tr.ini..=tr.fin).contains(&p.ini) && (a..=b).contains(&en_linea)
        });
        por_toma.entry(tr.toma).or_default().extend(dichas);
    }
    let partes: Vec<String> = por_toma
        .into_iter()
        .filter_map(|(k, ps)| {
            let (primera, ultima) = (ps.first()?, ps.last()?);
            let texto: String = ps
                .iter()
                .map(|p| p.txt.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(CITA_MAX)
                .collect();
            Some(format!(
                "toma {k} ({}), palabras {}–{}: «{texto}»",
                tomas[k].clip, primera.i, ultima.i
            ))
        })
        .collect();
    if partes.is_empty() {
        "sin palabras".to_owned()
    } else {
        partes.join("; ")
    }
}

/// The transcribed words of a clip; none if it has no speech.
fn palabras<'a>(trabajo: &'a Trabajo, clip: &str) -> &'a [Palabra] {
    trabajo
        .analisis
        .clips
        .iter()
        .find(|c| c.id == clip)
        .and_then(|c| c.transcripcion.as_ref())
        .map(|t| t.palabras.as_slice())
        .unwrap_or_default()
}

/// `x` to a tenth.
fn decima(x: f64) -> f64 {
    redondear(x, 1)
}

/// `x` to `digitos` decimals, rounding as Python's `round` does (half to
/// even on the exact decimal value), so the numbers match the old
/// engine's.
fn redondear(x: f64, digitos: usize) -> f64 {
    format!("{x:.digitos$}").parse().unwrap_or(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entrada::{
        Analisis, ClipAnalisis, Descartado, Inventario, Plan, Toma, Transcripcion,
    };
    use crate::linea::{RecursoLinea, Sub, Trozo};

    fn trozo(t: f64, dur: f64) -> Trozo {
        Trozo {
            clip: "habla".into(),
            ruta: String::new(),
            ini: 0.0,
            fin: dur,
            t,
            toma: 0,
            plano: false,
            imagen: false,
            audio: None,
            solo_audio: false,
            efecto: None,
        }
    }

    fn recurso(clip: &str, t: f64, dur: f64) -> RecursoLinea {
        RecursoLinea {
            clip: clip.into(),
            ruta: String::new(),
            imagen: false,
            t,
            dur,
            desde: 0.0,
            efecto: None,
            garabato: None,
            garabato_pos: [0.5, 0.45],
        }
    }

    fn sub(texto: &str, ini: f64, fin: f64) -> Sub {
        Sub {
            texto: texto.into(),
            estilo: "normal".into(),
            posicion: "auto".into(),
            ini,
            fin,
            apilar: None,
            apilar_arriba: false,
        }
    }

    fn linea(trozos: Vec<Trozo>, recursos: Vec<RecursoLinea>) -> Linea {
        let duracion_voz = trozos.iter().map(|t| t.t + t.fin - t.ini).fold(0.0, f64::max);
        Linea {
            trozos,
            subtitulos: Vec::new(),
            recursos,
            musica: None,
            duracion_voz,
        }
    }

    /// One take of "habla", a word a second: "w0".."w19".
    fn trabajo() -> Trabajo {
        let palabras = (0..20)
            .map(|i| Palabra {
                i,
                ini: i as f64,
                fin: i as f64 + 0.8,
                txt: format!("w{i}"),
                coletilla: false,
            })
            .collect();
        Trabajo {
            plan: Plan {
                tomas: vec![Toma {
                    clip: "habla".into(),
                    ..Toma::default()
                }],
                ..Plan::default()
            },
            analisis: Analisis {
                clips: vec![ClipAnalisis {
                    id: "habla".into(),
                    duracion: 20.0,
                    transcripcion: Some(Transcripcion { palabras }),
                    ..ClipAnalisis::default()
                }],
                ..Analisis::default()
            },
            inventario: Inventario::default(),
        }
    }

    #[test]
    fn talking_too_long_without_broll_is_a_stretch() {
        let l = linea(vec![trozo(0.0, 20.0)], vec![recurso("a", 8.0, 2.0)]);
        assert_eq!(tramos_sin_recurso(&l), vec![(0.0, 8.0), (10.0, 20.0)]);
    }

    #[test]
    fn a_shot_counts_as_picture() {
        let mut plano = trozo(5.0, 3.0);
        plano.plano = true;
        let l = linea(vec![trozo(0.0, 5.0), plano, trozo(8.0, 4.0)], Vec::new());
        assert!(tramos_sin_recurso(&l).is_empty());
    }

    #[test]
    fn voice_over_gaps_are_what_broll_leaves_uncovered() {
        let mut off = trozo(0.0, 6.0);
        off.solo_audio = true;
        let l = linea(vec![off], vec![recurso("a", 1.0, 2.0), recurso("b", 3.1, 2.9)]);
        assert_eq!(huecos_solo_audio(&l), vec![(0.0, 1.0)]);
    }

    #[test]
    fn the_quote_says_which_words_of_which_take() {
        let t = trabajo();
        let l = linea(vec![trozo(0.0, 20.0)], Vec::new());
        assert_eq!(
            palabras_en_tramo(&t, &l, 2.0, 4.0),
            "toma 0 (habla), palabras 2–4: «w2 w3 w4»"
        );
        assert_eq!(palabras_en_tramo(&t, &l, 30.0, 40.0), "sin palabras");
    }

    #[test]
    fn a_long_stretch_asks_for_broll_where_it_is() {
        let t = trabajo();
        let l = linea(vec![trozo(0.0, 20.0)], vec![recurso("a", 0.0, 13.0)]);
        let avisos = revisar(&t, &l);
        assert_eq!(avisos.len(), 1);
        assert_eq!(avisos[0].tipo, Tipo::SinRecurso);
        assert_eq!(avisos[0].texto, "Persona hablando 7.0 s seguidos sin recurso (13.0–20.0 s).");
        assert!(avisos[0].instruccion.as_deref().unwrap().contains("palabras 13–19"));
        assert_eq!(avisos[0].peso, 2.33);
    }

    #[test]
    fn captions_out_of_reading_length_are_flagged_unless_piled() {
        let mut l = linea(vec![trozo(0.0, 20.0)], vec![recurso("a", 0.0, 20.0)]);
        let mut apilado = sub("pila", 0.0, 0.3);
        apilado.apilar = Some("1".into());
        l.subtitulos = vec![sub("hola", 0.0, 0.5), sub("bien", 1.0, 3.0), sub("largo", 4.0, 10.0), apilado];
        let tipos: Vec<Tipo> = revisar(&trabajo(), &l).iter().map(|a| a.tipo).collect();
        assert_eq!(tipos, vec![Tipo::SubtituloCorto, Tipo::SubtituloLargo]);
    }

    #[test]
    fn repeated_broll_is_named_once() {
        let l = linea(
            vec![trozo(0.0, 6.0)],
            vec![recurso("a", 0.0, 2.0), recurso("b", 2.0, 2.0), recurso("a", 4.0, 2.0)],
        );
        let avisos = revisar(&trabajo(), &l);
        assert_eq!(avisos.len(), 1);
        assert_eq!(avisos[0].texto, "Recurso repetido: a.");
        assert!(avisos[0].instruccion.as_deref().unwrap().contains("aparece 2 veces"));
    }

    #[test]
    fn a_set_aside_file_is_told_but_not_scored() {
        let mut t = trabajo();
        t.inventario.descartados.push(Descartado {
            archivo: "roto.mov".into(),
            motivo: "no se pudo abrir".into(),
        });
        let l = linea(vec![trozo(0.0, 4.0)], Vec::new());
        let avisos = revisar(&t, &l);
        assert_eq!(avisos[0].texto, "Archivo descartado: roto.mov — no se pudo abrir");
        assert!(!avisos[0].corregible());
        assert_eq!(puntuacion(&avisos), 0.0);
    }

    #[test]
    fn the_correction_asks_only_for_what_can_be_fixed() {
        let avisos = vec![
            Aviso::nuevo(Tipo::Descartado, "x".into(), None, 0.0),
            Aviso::nuevo(Tipo::RecursoRepetido, "y".into(), Some("Cambia a.".into()), 1.0),
            Aviso::nuevo(Tipo::SinRecurso, "z".into(), Some("Añade b.".into()), 2.25),
        ];
        assert!(mensaje_correccion(&avisos).ends_with(":\n- Cambia a.\n- Añade b."));
        assert_eq!(puntuacion(&avisos), 3.25);
    }

    #[test]
    fn tenths_round_half_to_even_like_python() {
        assert_eq!(decima(0.25), 0.2);
        assert_eq!(decima(0.35), 0.3);
        assert_eq!(decima(12.34), 12.3);
    }
}
