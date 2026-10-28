//! The montage script: the plan as the team reads it, one block per take
//! with its words, captions and B-roll, and the edits the script pane
//! makes to it.
//!
//! [`Guion`] is what is shown; it is rebuilt from the job after every
//! change. The edits work on `ia/plan.json` itself as JSON, not on
//! [`Plan`](crate::entrada::Plan), so the fields this crate does not read
//! (the AI's notes, the template's name) survive a save. Every save keeps
//! the previous plan as a version, the way the engine's `edicion.py` does.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::entrada::Trabajo;
use crate::{Error, linea};

/// The plan as blocks, in the order they play.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guion {
    /// One per take.
    pub bloques: Vec<Bloque>,
}

/// What kind of take a block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    /// Someone speaking, cut by words.
    Voz,
    /// The voice is heard over other pictures.
    SoloAudio,
    /// A shot cut by seconds, no words.
    Plano,
}

/// One take.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloque {
    /// Index of the take in the plan.
    pub toma: usize,
    /// The clip it comes from.
    pub clip: String,
    /// Words, heard only, or a shot.
    pub tipo: Tipo,
    /// The words between the take's first and last, struck or kept.
    pub palabras: Vec<PalabraGuion>,
    /// Seconds it lasts with the cuts as they are.
    pub duracion: f64,
    /// The captions over it.
    pub subtitulos: Vec<SubGuion>,
    /// The B-roll over it.
    pub recursos: Vec<RecursoGuion>,
}

/// One word of a take.
#[derive(Clone, Debug, PartialEq)]
pub struct PalabraGuion {
    /// Its index in the clip's transcription.
    pub i: usize,
    /// As transcribed.
    pub txt: String,
    /// Cut out of the video.
    pub quitada: bool,
    /// A filler word ("eh", "pues") the transcription marked.
    pub coletilla: bool,
}

/// A caption shown over a block.
#[derive(Clone, Debug, PartialEq)]
pub struct SubGuion {
    /// Its index in the plan.
    pub indice: usize,
    /// What it says.
    pub texto: String,
    /// "normal", "enfasis" or "clave".
    pub estilo: String,
}

/// A piece of B-roll over a block.
#[derive(Clone, Debug, PartialEq)]
pub struct RecursoGuion {
    /// Its index in the plan.
    pub indice: usize,
    /// The clip shown.
    pub clip: String,
    /// "bn" and the like, or empty.
    pub efecto: String,
    /// The hand-drawn mark over it, or empty.
    pub garabato: String,
}

impl Guion {
    /// The script of a job as it stands.
    pub fn de(trabajo: &Trabajo) -> Guion {
        let plan = &trabajo.plan;
        let clips = &trabajo.analisis.clips;
        let bloques = plan
            .tomas
            .iter()
            .enumerate()
            .map(|(n, toma)| {
                let transcripcion = clips
                    .iter()
                    .find(|c| c.id == toma.clip)
                    .and_then(|c| c.transcripcion.as_ref());
                let (tipo, palabras, duracion) = match (transcripcion, toma.es_plano()) {
                    (Some(t), false) => {
                        let desde = toma.desde.unwrap_or(0);
                        let hasta = toma.hasta.unwrap_or(t.palabras.len().saturating_sub(1));
                        let fuera =
                            |i: usize| toma.quitar.iter().any(|[a, b]| (*a..=*b).contains(&i));
                        let palabras = t
                            .palabras
                            .iter()
                            .filter(|p| (desde..=hasta).contains(&p.i))
                            .map(|p| PalabraGuion {
                                i: p.i,
                                txt: p.txt.trim().to_owned(),
                                quitada: fuera(p.i),
                                coletilla: p.coletilla,
                            })
                            .collect();
                        let duracion =
                            linea::trozos_de_toma(&t.palabras, desde, hasta, &toma.quitar)
                                .iter()
                                .map(|(a, b)| b - a)
                                .sum();
                        let tipo = if toma.solo_audio {
                            Tipo::SoloAudio
                        } else {
                            Tipo::Voz
                        };
                        (tipo, palabras, duracion)
                    }
                    _ => (Tipo::Plano, Vec::new(), toma.dur.unwrap_or(0.0)),
                };
                let subtitulos = plan
                    .subtitulos
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.toma == n)
                    .map(|(indice, s)| SubGuion {
                        indice,
                        texto: s.texto.clone(),
                        estilo: s.estilo.clone().unwrap_or_else(|| "normal".into()),
                    })
                    .collect();
                let recursos = plan
                    .recursos
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        r.toma == Some(n)
                            || r.subtitulo
                                .and_then(|k| plan.subtitulos.get(k))
                                .is_some_and(|s| s.toma == n)
                    })
                    .map(|(indice, r)| RecursoGuion {
                        indice,
                        clip: r.clip.clone(),
                        efecto: r.efecto.clone().unwrap_or_default(),
                        garabato: r.garabato.clone().unwrap_or_default(),
                    })
                    .collect();
                Bloque {
                    toma: n,
                    clip: toma.clip.clone(),
                    tipo,
                    palabras,
                    duracion,
                    subtitulos,
                    recursos,
                }
            })
            .collect();
        Guion { bloques }
    }

    /// Seconds the voice line lasts.
    pub fn duracion(&self) -> f64 {
        self.bloques.iter().map(|b| b.duracion).sum()
    }
}

/// A block's words broken into rows of at most `columnas` characters, as
/// index ranges into [`Bloque::palabras`]. The interface lays words out in
/// rows and cannot measure them itself.
pub fn filas(palabras: &[PalabraGuion], columnas: usize) -> Vec<std::ops::Range<usize>> {
    let columnas = columnas.max(8);
    let mut filas = Vec::new();
    let (mut inicio, mut ancho) = (0, 0);
    for (n, p) in palabras.iter().enumerate() {
        let largo = p.txt.chars().count() + 1;
        if ancho > 0 && ancho + largo > columnas {
            filas.push(inicio..n);
            (inicio, ancho) = (n, 0);
        }
        ancho += largo;
    }
    if inicio < palabras.len() {
        filas.push(inicio..palabras.len());
    }
    filas
}

/// `ia/plan.json` as JSON, with the script pane's edits.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanCrudo(pub Value);

impl PlanCrudo {
    /// The job's plan file.
    pub fn leer(trabajo: &Path) -> Result<PlanCrudo, Error> {
        let ruta = trabajo.join("ia").join("plan.json");
        let texto = std::fs::read_to_string(&ruta).map_err(|e| Error::Leer(ruta.clone(), e))?;
        let valor = serde_json::from_str(&texto).map_err(|e| Error::Json(ruta, e))?;
        Ok(PlanCrudo(valor))
    }

    /// The plan as the layout reads it, edits and all.
    pub fn plan(&self) -> Result<crate::entrada::Plan, Error> {
        serde_json::from_value(self.0.clone())
            .map_err(|e| Error::Json(PathBuf::from("ia/plan.json"), e))
    }

    fn lista(&mut self, clave: &str) -> &mut Vec<Value> {
        let raiz = self.0.as_object_mut().expect("a plan is an object");
        raiz.entry(clave)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("the plan's lists are arrays")
    }

    /// Cuts word `i` of take `toma` out, or puts it back.
    pub fn alternar_palabra(&mut self, toma: usize, i: usize) {
        let Some(t) = self.lista("tomas").get_mut(toma) else {
            return;
        };
        let mut fuera: Vec<usize> = t["quitar"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| Some((r[0].as_u64()? as usize)..=(r[1].as_u64()? as usize)))
            .flatten()
            .collect();
        if fuera.contains(&i) {
            fuera.retain(|&w| w != i);
        } else {
            fuera.push(i);
        }
        fuera.sort_unstable();
        fuera.dedup();
        // Back into ranges, runs of neighbours joined.
        let mut rangos: Vec<[usize; 2]> = Vec::new();
        for w in fuera {
            match rangos.last_mut() {
                Some(r) if r[1] + 1 == w => r[1] = w,
                _ => rangos.push([w, w]),
            }
        }
        t["quitar"] = json!(rangos);
    }

    /// Removes take `toma`, with the captions and B-roll anchored to it.
    pub fn quitar_toma(&mut self, toma: usize) {
        let total = self.lista("tomas").len();
        if toma >= total {
            return;
        }
        let orden: Vec<Option<usize>> = (0..total)
            .map(|n| match n.cmp(&toma) {
                std::cmp::Ordering::Less => Some(n),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(n - 1),
            })
            .collect();
        self.lista("tomas").remove(toma);
        self.renumerar(&orden);
    }

    /// Moves take `toma` one place earlier (`-1`) or later (`1`).
    pub fn mover_toma(&mut self, toma: usize, hacia: i32) {
        let total = self.lista("tomas").len();
        let Some(destino) = toma
            .checked_add_signed(hacia as isize)
            .filter(|&d| d < total)
        else {
            return;
        };
        if toma >= total {
            return;
        }
        self.lista("tomas").swap(toma, destino);
        let orden: Vec<Option<usize>> = (0..total)
            .map(|n| {
                Some(if n == toma {
                    destino
                } else if n == destino {
                    toma
                } else {
                    n
                })
            })
            .collect();
        self.renumerar(&orden);
    }

    /// Points captions and B-roll at the takes' new places (`orden[old]`),
    /// drops those whose take went, and keeps the captions in playing
    /// order, which the piles count on.
    fn renumerar(&mut self, orden: &[Option<usize>]) {
        let nueva = |v: &Value| {
            v.as_u64()
                .and_then(|n| orden.get(n as usize).copied().flatten())
        };
        // Captions: renumber, drop, sort by take; remember where each went.
        let subtitulos = std::mem::take(self.lista("subtitulos"));
        let mut quedan: Vec<(usize, usize, Value)> = Vec::new();
        for (viejo, mut s) in subtitulos.into_iter().enumerate() {
            if let Some(t) = nueva(&s["toma"]) {
                s["toma"] = json!(t);
                quedan.push((t, viejo, s));
            }
        }
        quedan.sort_by_key(|(t, viejo, _)| (*t, *viejo));
        let mut sitio = vec![None; quedan.iter().map(|q| q.1 + 1).max().unwrap_or(0)];
        for (nuevo, (_, viejo, _)) in quedan.iter().enumerate() {
            sitio[*viejo] = Some(nuevo);
        }
        *self.lista("subtitulos") = quedan.into_iter().map(|(_, _, s)| s).collect();
        // B-roll: follow their take or their caption.
        let recursos = std::mem::take(self.lista("recursos"));
        let mut siguen = Vec::new();
        for mut r in recursos {
            if let Some(k) = r["subtitulo"].as_u64() {
                match sitio.get(k as usize).copied().flatten() {
                    Some(k) => r["subtitulo"] = json!(k),
                    None => continue,
                }
            }
            if !r["toma"].is_null() {
                match nueva(&r["toma"]) {
                    Some(t) => r["toma"] = json!(t),
                    None => continue,
                }
            }
            siguen.push(r);
        }
        *self.lista("recursos") = siguen;
    }

    /// Rewrites caption `k`.
    pub fn cambiar_subtitulo(&mut self, k: usize, texto: &str) {
        if let Some(s) = self.lista("subtitulos").get_mut(k) {
            s["texto"] = json!(texto);
        }
    }

    /// Removes caption `k`; B-roll that covered it goes with it.
    pub fn quitar_subtitulo(&mut self, k: usize) {
        if k >= self.lista("subtitulos").len() {
            return;
        }
        self.lista("subtitulos").remove(k);
        let recursos = std::mem::take(self.lista("recursos"));
        *self.lista("recursos") = recursos
            .into_iter()
            .filter_map(|mut r| {
                if let Some(j) = r["subtitulo"].as_u64().map(|j| j as usize) {
                    if j == k {
                        return None;
                    }
                    if j > k {
                        r["subtitulo"] = json!(j - 1);
                    }
                }
                Some(r)
            })
            .collect();
    }

    /// Removes piece of B-roll `k`.
    pub fn quitar_recurso(&mut self, k: usize) {
        let recursos = self.lista("recursos");
        if k < recursos.len() {
            recursos.remove(k);
        }
    }

    /// Writes the plan back, keeping the one it replaces as a version in
    /// `ia/versiones`, in the engine's own numbering.
    pub fn guardar(&self, trabajo: &Path, nota: &str) -> Result<(), Error> {
        let ia = trabajo.join("ia");
        let ruta = ia.join("plan.json");
        let dir = ia.join("versiones");
        let escribir = |ruta: &Path, texto: &str| {
            std::fs::write(ruta, texto).map_err(|e| Error::Escribir(ruta.into(), e))
        };
        std::fs::create_dir_all(&dir).map_err(|e| Error::Escribir(dir.clone(), e))?;
        let indice = dir.join("versiones.json");
        let mut lista: Vec<Value> = std::fs::read_to_string(&indice)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let n = lista
            .iter()
            .filter_map(|v| v["n"].as_u64())
            .max()
            .unwrap_or(0)
            + 1;
        let texto = serde_json::to_string_pretty(&self.0).expect("JSON always serialises");
        escribir(&ruta, &texto)?;
        escribir(&dir.join(format!("v{n}.json")), &texto)?;
        lista.push(json!({"n": n, "origen": "komunta", "nota": nota, "cambios": []}));
        escribir(
            &indice,
            &serde_json::to_string_pretty(&lista).expect("JSON always serialises"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> PlanCrudo {
        PlanCrudo(json!({
            "plantilla": "mensaje_a_camara",
            "tomas": [
                {"clip": "a", "desde": 0, "hasta": 9},
                {"clip": "b", "desde": 0, "hasta": 5, "quitar": [[2, 3]]},
                {"clip": "c", "desde_seg": 1.0, "dur": 2.0}
            ],
            "subtitulos": [
                {"texto": "uno", "toma": 0},
                {"texto": "dos", "toma": 1},
                {"texto": "tres", "toma": 2}
            ],
            "recursos": [
                {"clip": "r1", "subtitulo": 1},
                {"clip": "r2", "toma": 2, "seg": 0.5},
                {"clip": "r3", "subtitulo": 0}
            ]
        }))
    }

    #[test]
    fn a_word_is_struck_and_put_back() {
        let mut p = plan();
        p.alternar_palabra(1, 4);
        assert_eq!(p.0["tomas"][1]["quitar"], json!([[2, 4]]));
        p.alternar_palabra(1, 3);
        assert_eq!(p.0["tomas"][1]["quitar"], json!([[2, 2], [4, 4]]));
        p.alternar_palabra(1, 3);
        p.alternar_palabra(1, 2);
        assert_eq!(p.0["tomas"][1]["quitar"], json!([[3, 4]]));
        assert_eq!(p.0["plantilla"], "mensaje_a_camara");
    }

    #[test]
    fn removing_a_take_takes_its_captions_and_broll() {
        let mut p = plan();
        p.quitar_toma(1);
        let textos: Vec<_> = p.0["subtitulos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["texto"].clone())
            .collect();
        assert_eq!(textos, [json!("uno"), json!("tres")]);
        assert_eq!(p.0["subtitulos"][1]["toma"], 1);
        let recursos = p.0["recursos"].as_array().unwrap();
        assert_eq!(recursos.len(), 2);
        assert_eq!(recursos[0]["clip"], "r2");
        assert_eq!(recursos[0]["toma"], 1);
        assert_eq!(recursos[1]["subtitulo"], 0);
    }

    #[test]
    fn moving_a_take_keeps_captions_in_playing_order() {
        let mut p = plan();
        p.mover_toma(0, 1);
        assert_eq!(p.0["tomas"][0]["clip"], "b");
        let textos: Vec<_> = p.0["subtitulos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["texto"].clone())
            .collect();
        assert_eq!(textos, [json!("dos"), json!("uno"), json!("tres")]);
        // r1 covered "dos", now caption 0; r3 covered "uno", now caption 1.
        assert_eq!(p.0["recursos"][0]["subtitulo"], 0);
        assert_eq!(p.0["recursos"][2]["subtitulo"], 1);
        p.mover_toma(0, -1);
        assert_eq!(p.0["tomas"][0]["clip"], "b");
    }

    #[test]
    fn removing_a_caption_renumbers_the_broll_on_later_ones() {
        let mut p = plan();
        p.quitar_subtitulo(0);
        let recursos = p.0["recursos"].as_array().unwrap();
        assert_eq!(recursos.len(), 2);
        assert_eq!(recursos[0]["subtitulo"], 0);
    }

    #[test]
    fn words_wrap_into_rows() {
        let palabras: Vec<PalabraGuion> = ["uno", "dos", "tres", "cuatro", "cinco"]
            .iter()
            .enumerate()
            .map(|(i, t)| PalabraGuion {
                i,
                txt: (*t).into(),
                quitada: false,
                coletilla: false,
            })
            .collect();
        assert_eq!(filas(&palabras, 10), vec![0..2, 2..3, 3..4, 4..5]);
        assert_eq!(filas(&palabras, 80), vec![0..5]);
    }
}
