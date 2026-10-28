//! Recipes: what kind of video a montage is. The engine's four fixed
//! templates become recipes the team can copy, edit and add to - plain
//! instructions for the AI plus the few choices that are better as buttons
//! than as prose (frame, length, pace, captions, music).
//!
//! A recipe is a JSON file; the four built in are always there.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::entrada::{Subtitulo, Trabajo};

/// What a montage does with captions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModoSubtitulos {
    /// The captions document, word for word, when there is one; from the
    /// voice otherwise.
    #[default]
    Documento,
    /// Written from what is said: transcription cut into lines.
    Voz,
    /// Only a title and key words.
    Claves,
    /// No captions.
    Ninguno,
}

/// Where a montage's music comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModoMusica {
    /// None.
    Ninguna,
    /// Only music brought with the material.
    #[default]
    Material,
    /// The brand kit's music when the material has none.
    Kit,
}

/// A kind of video.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Receta {
    /// File name and handle, `[a-z0-9_]`.
    pub id: String,
    /// What the team calls it.
    pub nombre: String,
    /// One line for the recipe's card.
    pub descripcion: String,
    /// Rules for the AI in plain words: what goes first, what to cut, what
    /// to show. Wins over the general rules where they disagree.
    pub instrucciones: String,
    /// Main frame: "vertical", "16x9" or "1x1".
    pub formato: String,
    /// Other frames made from the same plan.
    pub formatos_extra: Vec<String>,
    /// Target length in seconds, shortest and longest.
    pub duracion: Option<[f64; 2]>,
    /// What to do with captions.
    pub subtitulos: ModoSubtitulos,
    /// Where the music comes from.
    pub musica: ModoMusica,
    /// Music level under the voice.
    pub volumen_musica: f64,
    /// Share of the time B-roll covers the speaker, `0..=1`.
    pub recursos: f64,
    /// Length of each piece of B-roll or shot, shortest and longest.
    pub plano: [f64; 2],
    /// Longest stretch of a face talking without B-roll, in seconds.
    pub max_cara: f64,
    /// Share of caption lines in emphasis.
    pub enfasis: f64,
}

impl Default for Receta {
    fn default() -> Self {
        Receta {
            id: String::new(),
            nombre: String::new(),
            descripcion: String::new(),
            instrucciones: String::new(),
            formato: "vertical".to_owned(),
            formatos_extra: Vec::new(),
            duracion: None,
            subtitulos: ModoSubtitulos::Documento,
            musica: ModoMusica::Material,
            volumen_musica: 0.16,
            recursos: 0.5,
            plano: [1.5, 2.5],
            max_cara: 6.0,
            enfasis: 0.33,
        }
    }
}

impl Receta {
    /// The recipe's choices as rules for the AI, after its own
    /// instructions.
    pub fn reglas(&self) -> String {
        let mut reglas = vec![format!("## Receta: {}", self.nombre)];
        if !self.instrucciones.trim().is_empty() {
            reglas.push(self.instrucciones.trim().to_owned());
        }
        if let Some([corta, larga]) = self.duracion {
            reglas.push(format!(
                "- Duración total: entre {corta:.0} y {larga:.0} s."
            ));
        }
        reglas.push(format!(
            "- Recursos: cubren aprox. el {:.0} % del tiempo, de {:.1} a {:.1} s cada uno; nunca más de {:.0} s seguidos de persona hablando sin recurso.",
            self.recursos * 100.0,
            self.plano[0],
            self.plano[1],
            self.max_cara
        ));
        reglas.push(
            match self.subtitulos {
                ModoSubtitulos::Documento => {
                    "- Subtítulos: si hay documento de subtítulos, úsalo tal cual; si no, subtitula lo que se dice."
                }
                ModoSubtitulos::Voz => {
                    "- Subtítulos: NO los escribas; se generan solos de la voz. Deja \"subtitulos\" vacío salvo títulos."
                }
                ModoSubtitulos::Claves => {
                    "- Subtítulos: solo un título al principio y las palabras clave cuando se mencionan (estilo \"clave\")."
                }
                ModoSubtitulos::Ninguno => "- Subtítulos: ninguno. Deja \"subtitulos\" vacío.",
            }
            .to_owned(),
        );
        if matches!(
            self.subtitulos,
            ModoSubtitulos::Documento | ModoSubtitulos::Voz
        ) {
            reglas.push(format!(
                "- Énfasis: aprox. 1 de cada {:.0} líneas.",
                (1.0 / self.enfasis.max(0.05)).round()
            ));
        }
        reglas.push(
            match self.musica {
                ModoMusica::Ninguna => "- Música: ninguna.".to_owned(),
                ModoMusica::Material => format!(
                    "- Música: solo si viene en el material, a volumen {:.2}.",
                    self.volumen_musica
                ),
                ModoMusica::Kit => format!(
                    "- Música: la del material o, si no hay, la del kit de marca (\"clip\": \"musica_kit\"), a volumen {:.2}.",
                    self.volumen_musica
                ),
            },
        );
        reglas.join("\n")
    }
}

/// The four templates the engine started with, as recipes.
pub fn de_serie() -> Vec<Receta> {
    vec![
        Receta {
            id: "mensaje_a_camara".to_owned(),
            nombre: "Mensaje a cámara".to_owned(),
            descripcion: "Una persona habla a cámara; los recursos cubren la imagen mientras se oye su voz.".to_owned(),
            instrucciones: "- Subtítulos completos.".to_owned(),
            ..Receta::default()
        },
        Receta {
            id: "voz_en_off".to_owned(),
            nombre: "Voz en off".to_owned(),
            descripcion: "Presentación a cámara y voz en off grabada aparte, cubierta de recursos.".to_owned(),
            instrucciones: "\
- Una persona presenta a cámara al principio y despide al final; en medio suena una voz en off grabada aparte (clip largo cuya imagen es negra o no sirve): márcala con \"solo_audio\": true.
- Toda la voz en off queda cubierta por recursos encadenados sin huecos (2-3 s cada uno, anclados con toma/palabra). Fotos o planos antiguos con \"efecto\": \"bn\".
- Las palabras clave, apiladas hacia arriba (\"apilar\" + \"apilar_hacia\": \"arriba\").".to_owned(),
            subtitulos: ModoSubtitulos::Claves,
            recursos: 0.9,
            plano: [2.0, 3.0],
            ..Receta::default()
        },
        Receta {
            id: "entrevista".to_owned(),
            nombre: "Entrevista".to_owned(),
            descripcion: "Preguntas y respuestas: las preguntas salen como texto, las respuestas recortadas.".to_owned(),
            instrucciones: "\
- Dos o más personas: quien pregunta y quien responde. Lo importante son las respuestas.
- Cada pregunta aparece como subtítulo \"clave\" arriba (\"posicion\": \"arriba\") al empezar su respuesta; la voz de quien pregunta solo se deja si es corta y clara.
- Respuestas recortadas a lo esencial, en el orden del guion si lo hay.".to_owned(),
            ..Receta::default()
        },
        Receta {
            id: "resumen_acto".to_owned(),
            nombre: "Resumen de un acto".to_owned(),
            descripcion: "Planos del acto al ritmo de la música, con alguna frase destacada.".to_owned(),
            instrucciones: "\
- No hay un discurso que seguir: el vídeo es una secuencia de planos (tomas SIN palabras, con desde_seg y dur) al ritmo de la música, eligiendo los mejores momentos.
- Si no hay música, monta igual con \"audio\": \"ambiente\".
- Estructura: 1-2 planos generales para situar, luego detalles y gente, y un final con el plano más potente.
- Puedes intercalar 1-3 frases cortas con voz si dicen algo potente; con subtítulo.
- Título al principio sobre el primer plano (estilo \"clave\"). No repitas planos.".to_owned(),
            duracion: Some([20.0, 45.0]),
            subtitulos: ModoSubtitulos::Claves,
            musica: ModoMusica::Kit,
            volumen_musica: 0.75,
            recursos: 1.0,
            plano: [1.5, 3.0],
            ..Receta::default()
        },
    ]
}

/// The built-in recipes and the team's own from `carpeta` (one JSON file
/// each); a file with a built-in's id replaces it.
pub fn cargar(carpeta: &Path) -> Vec<Receta> {
    let mut recetas = de_serie();
    let Ok(entradas) = std::fs::read_dir(carpeta) else {
        return recetas;
    };
    let mut propias: Vec<Receta> = entradas
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok())
        .collect();
    propias.sort_by(|a: &Receta, b| a.nombre.cmp(&b.nombre));
    for receta in propias {
        match recetas.iter_mut().find(|r| r.id == receta.id) {
            Some(hueco) => *hueco = receta,
            None => recetas.push(receta),
        }
    }
    recetas
}

/// Writes a recipe to `carpeta/<id>.json`.
pub fn guardar(carpeta: &Path, receta: &Receta) -> Result<(), Error> {
    std::fs::create_dir_all(carpeta).map_err(|e| Error::Escribir(carpeta.into(), e))?;
    let ruta = carpeta.join(format!("{}.json", receta.id));
    let texto = serde_json::to_string_pretty(receta).expect("a recipe always serialises");
    std::fs::write(&ruta, texto).map_err(|e| Error::Escribir(ruta, e))
}

/// The recipe's caption choice applied to a plan: none clears them; from
/// the voice writes them when the plan brought none (the AI may still have
/// placed titles).
pub fn aplicar(trabajo: &mut Trabajo, receta: &Receta, max_letras: usize) {
    match receta.subtitulos {
        ModoSubtitulos::Ninguno => trabajo.plan.subtitulos.clear(),
        ModoSubtitulos::Voz if trabajo.plan.subtitulos.is_empty() => {
            trabajo.plan.subtitulos = subtitulos_de_la_voz(trabajo, max_letras);
        }
        _ => {}
    }
}

/// Captions written from what each take says: the kept words, cut into
/// lines at sentence ends, pauses, a length in letters or three seconds.
/// Fillers the analysis marked are left out of the text.
pub fn subtitulos_de_la_voz(trabajo: &Trabajo, max_letras: usize) -> Vec<Subtitulo> {
    let mut subs = Vec::new();
    for (k, toma) in trabajo.plan.tomas.iter().enumerate() {
        if toma.es_plano() {
            continue;
        }
        let Some(palabras) = trabajo
            .analisis
            .clips
            .iter()
            .find(|c| c.id == toma.clip)
            .and_then(|c| c.transcripcion.as_ref())
            .map(|t| &t.palabras)
        else {
            continue;
        };
        let (desde, hasta) = (toma.desde.unwrap_or(0), toma.hasta.unwrap_or(0));
        let fuera = |i: usize| toma.quitar.iter().any(|[a, b]| (*a..=*b).contains(&i));
        // (position, word) of what is kept.
        let kept: Vec<(usize, &crate::entrada::Palabra)> = palabras
            .iter()
            .enumerate()
            .skip(desde)
            .take((hasta + 1).saturating_sub(desde))
            .filter(|(_, p)| !fuera(p.i))
            .collect();
        let mut linea: Vec<(usize, &crate::entrada::Palabra)> = Vec::new();
        let cerrar = |linea: &mut Vec<(usize, &crate::entrada::Palabra)>,
                      subs: &mut Vec<Subtitulo>| {
            let texto = linea
                .iter()
                .filter(|(_, p)| !p.coletilla)
                .map(|(_, p)| p.txt.trim())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if let (Some((primera, _)), Some((ultima, _))) = (linea.first(), linea.last())
                && !texto.is_empty()
            {
                subs.push(Subtitulo {
                    texto,
                    toma: k,
                    desde: Some(*primera),
                    hasta: Some(*ultima),
                    estilo: Some("normal".to_owned()),
                    ..Subtitulo::default()
                });
            }
            linea.clear();
        };
        for (n, p) in kept {
            if let Some((_, ant)) = linea.last() {
                let letras: usize = linea
                    .iter()
                    .map(|(_, p)| p.txt.trim().chars().count() + 1)
                    .sum();
                let fin_frase = ant.txt.trim_end().ends_with(['.', '?', '!', ':', ';']);
                let pausa = p.ini - ant.fin > 0.6;
                let larga = letras + p.txt.trim().chars().count() > max_letras;
                let lenta = p.fin - linea[0].1.ini > 3.0;
                if fin_frase || pausa || larga || lenta {
                    cerrar(&mut linea, &mut subs);
                }
            }
            linea.push((n, p));
        }
        cerrar(&mut linea, &mut subs);
    }
    subs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entrada::{ClipAnalisis, Palabra, Toma, Transcripcion};

    fn trabajo(palabras: &[(&str, f64, f64)]) -> Trabajo {
        let mut t = Trabajo::default();
        t.analisis.clips.push(ClipAnalisis {
            id: "a".to_owned(),
            duracion: 30.0,
            transcripcion: Some(Transcripcion {
                palabras: palabras
                    .iter()
                    .enumerate()
                    .map(|(i, (txt, ini, fin))| Palabra {
                        i,
                        ini: *ini,
                        fin: *fin,
                        txt: (*txt).to_owned(),
                        coletilla: *txt == "eh,",
                    })
                    .collect(),
            }),
            encuadre: None,
        });
        t.plan.tomas.push(Toma {
            clip: "a".to_owned(),
            desde: Some(0),
            hasta: Some(palabras.len() - 1),
            ..Toma::default()
        });
        t
    }

    #[test]
    fn lines_break_at_sentence_ends_and_pauses() {
        let t = trabajo(&[
            ("Hola", 0.0, 0.3),
            ("a", 0.35, 0.4),
            ("todos.", 0.45, 0.8),
            ("eh,", 0.9, 1.0),
            ("Hoy", 1.1, 1.3),
            ("empezamos", 2.5, 3.0),
        ]);
        let subs = subtitulos_de_la_voz(&t, 32);
        let textos: Vec<&str> = subs.iter().map(|s| s.texto.as_str()).collect();
        assert_eq!(textos, ["Hola a todos.", "Hoy", "empezamos"]);
        assert_eq!((subs[1].desde, subs[1].hasta), (Some(3), Some(4)));
    }

    #[test]
    fn none_clears_and_voice_fills_only_an_empty_plan() {
        let mut t = trabajo(&[("Hola", 0.0, 0.3)]);
        let voz = Receta {
            subtitulos: ModoSubtitulos::Voz,
            ..Receta::default()
        };
        aplicar(&mut t, &voz, 32);
        assert_eq!(t.plan.subtitulos.len(), 1);
        let ninguno = Receta {
            subtitulos: ModoSubtitulos::Ninguno,
            ..Receta::default()
        };
        aplicar(&mut t, &ninguno, 32);
        assert!(t.plan.subtitulos.is_empty());
    }

    #[test]
    fn built_ins_round_trip_as_json() {
        for receta in de_serie() {
            let texto = serde_json::to_string(&receta).unwrap();
            assert_eq!(serde_json::from_str::<Receta>(&texto).unwrap(), receta);
            assert!(receta.reglas().contains(&receta.nombre));
        }
    }
}
