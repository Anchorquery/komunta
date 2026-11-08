//! What an AI sees in each clip and still, in words, for the planner: with
//! no description it chooses the B-roll from three frames a clip, almost
//! blind. The engine's `motor/descripcion.py`.
//!
//! Gemini watches video: every clip not yet described goes in one "contact
//! video" - the clips one after the other, small, silent, each with its id
//! written on top - and one request describes them all, which is what a
//! free key's few requests a day allow. Claude sees pictures: a strip of
//! frames a clip, each with its second under it. The descriptions are kept
//! in `analisis.json` and in the cache, so a clip is never sent twice.
//!
//! This module is what both share: what is left to describe, how it is cut
//! into requests, and what an answer says ([`respuesta`]).

#[cfg(feature = "describir")]
pub mod anthropic;
#[cfg(feature = "imagen")]
pub mod contacto;
pub mod cuota;
#[cfg(feature = "describir")]
pub mod describir;
#[cfg(feature = "gemini")]
pub mod google;
pub mod respuesta;
#[cfg(feature = "imagen")]
pub mod tiras;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::inventario::Inventario;

/// The longest stretch of a B-roll clip the contact video shows, in
/// seconds; of a clip with speech, whose face is all there is to see; and
/// how long a still stays on.
pub const MAX_SEG_RECURSO: f64 = 12.0;
/// See [`MAX_SEG_RECURSO`].
pub const MAX_SEG_VOZ: f64 = 6.0;
/// See [`MAX_SEG_RECURSO`].
pub const MAX_SEG_FOTO: f64 = 2.0;
/// The longest a contact video - one request - runs: 15 minutes.
pub const MAX_SEG_LOTE: f64 = 900.0;

/// What an AI said a clip or still shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Descripcion {
    /// A sentence of 10-25 words.
    pub descripcion: Option<String>,
    /// general, medio, primer_plano or detalle.
    pub plano: Option<String>,
    /// Keywords: place, people, objects, action.
    pub etiquetas: Vec<String>,
    /// Shaky, out of focus, dark...
    pub problemas: Vec<String>,
    /// toma_a_camara, recurso or no_usar.
    pub uso: Option<String>,
    /// Its useful stretches, in seconds of the clip.
    pub momentos: Vec<Momento>,
    /// The clip was watched only up to this second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visto_hasta: Option<f64>,
    /// "fotogramas" when it was seen in frames, not watched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visto_por: Option<String>,
}

/// A useful stretch of a clip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Momento {
    /// Its start, in seconds of the clip.
    pub desde: f64,
    /// Its end.
    pub hasta: f64,
    /// What happens in it.
    pub que: Option<String>,
}

impl Descripcion {
    /// The line the planner reads for the clip: the engine's
    /// `texto_descripcion`.
    pub fn texto(&self) -> String {
        let mut partes = vec![self.descripcion.clone().unwrap_or_default()];
        if let Some(plano) = self.plano.as_deref().filter(|p| !p.is_empty()) {
            partes.push(format!("plano {plano}"));
        }
        if let Some(uso) = self.uso.as_deref().filter(|u| !u.is_empty()) {
            partes.push(format!("uso: {uso}"));
        }
        if !self.problemas.is_empty() {
            partes.push(format!("problemas: {}", self.problemas.join(", ")));
        }
        let momentos: Vec<String> = self
            .momentos
            .iter()
            .map(|m| {
                let que = m.que.as_deref().unwrap_or("None");
                format!("{}-{}s {que}", numero(m.desde), numero(m.hasta))
            })
            .collect();
        if !momentos.is_empty() {
            partes.push(format!("momentos: {}", momentos.join("; ")));
        }
        if let Some(hasta) = self.visto_hasta.filter(|h| *h != 0.0) {
            partes.push(format!("(visto solo hasta {}s)", numero(hasta)));
        }
        partes.retain(|parte| !parte.is_empty());
        partes.join(" · ")
    }
}

/// A number as Python prints a float: `12.0`, `2.5`.
pub fn numero(valor: f64) -> String {
    if valor.fract() == 0.0 && valor.abs() < 1e16 {
        format!("{valor:.1}")
    } else {
        valor.to_string()
    }
}

/// A clip or still no AI has described yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Pendiente {
    /// Its id in the job.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// A clip, or a still.
    pub tipo: Tipo,
}

/// What a [`Pendiente`] is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tipo {
    /// A clip, `duracion` seconds long; `con_voz` when someone speaks in
    /// it.
    Clip {
        /// Seconds long.
        duracion: f64,
        /// Someone speaks in it.
        con_voz: bool,
    },
    /// A still.
    Foto,
}

impl Pendiente {
    /// How long the contact video shows it: a clip up to its cap, a still
    /// [`MAX_SEG_FOTO`].
    pub fn segundos_en_contacto(&self) -> f64 {
        match self.tipo {
            Tipo::Clip { duracion, con_voz } => {
                let tope = if con_voz { MAX_SEG_VOZ } else { MAX_SEG_RECURSO };
                redondear(duracion.min(tope), 2)
            }
            Tipo::Foto => MAX_SEG_FOTO,
        }
    }

    /// Seconds long; a still, none.
    pub fn duracion(&self) -> f64 {
        match self.tipo {
            Tipo::Clip { duracion, .. } => duracion,
            Tipo::Foto => 0.0,
        }
    }
}

/// The clips and stills of `analisis` (an `analisis.json`) with no
/// description, clips first, each with its file from `inventario`. One the
/// inventory does not list is left out: there is nothing to show.
pub fn pendientes(analisis: &Map<String, Value>, inventario: &Inventario) -> Vec<Pendiente> {
    let lista = |clave: &str| {
        analisis
            .get(clave)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|entrada| sin_describir(entrada))
            .filter_map(|entrada| entrada.get("id")?.as_str())
            .collect::<Vec<_>>()
    };
    let clips = lista("clips").into_iter().filter_map(|id| {
        let video = inventario.videos.iter().find(|v| v.id == id)?;
        let entrada = entrada(analisis, "clips", id)?;
        let con_voz = entrada.get("transcripcion").is_some_and(lleno);
        let duracion = entrada.get("duracion").and_then(Value::as_f64).unwrap_or(video.duracion);
        Some(Pendiente {
            id: id.to_owned(),
            ruta: video.ruta.clone(),
            tipo: Tipo::Clip { duracion, con_voz },
        })
    });
    let fotos = lista("imagenes").into_iter().filter_map(|id| {
        let imagen = inventario.imagenes.iter().find(|i| i.id == id)?;
        Some(Pendiente {
            id: id.to_owned(),
            ruta: imagen.ruta.clone(),
            tipo: Tipo::Foto,
        })
    });
    clips.chain(fotos).collect()
}

fn sin_describir(entrada: &Value) -> bool {
    !entrada.get("descripcion").is_some_and(lleno)
}

/// Whether Python would take `valor` as true: not null, false, zero or
/// empty.
fn lleno(valor: &Value) -> bool {
    match valor {
        Value::Null => false,
        Value::Bool(verdad) => *verdad,
        Value::Number(numero) => numero.as_f64() != Some(0.0),
        Value::String(texto) => !texto.is_empty(),
        Value::Array(lista) => !lista.is_empty(),
        Value::Object(campos) => !campos.is_empty(),
    }
}

fn entrada<'a>(analisis: &'a Map<String, Value>, clave: &str, id: &str) -> Option<&'a Value> {
    analisis
        .get(clave)?
        .as_array()?
        .iter()
        .find(|entrada| entrada.get("id").and_then(Value::as_str) == Some(id))
}

/// `pendientes` cut into contact videos of at most [`MAX_SEG_LOTE`]
/// seconds, in order. One longer than that on its own still gets its own.
pub fn lotes(pendientes: Vec<Pendiente>) -> Vec<Vec<Pendiente>> {
    let mut lotes = Vec::new();
    let (mut actual, mut total) = (Vec::new(), 0.0);
    for pendiente in pendientes {
        let segundos = pendiente.segundos_en_contacto();
        if !actual.is_empty() && total + segundos > MAX_SEG_LOTE {
            lotes.push(std::mem::take(&mut actual));
            total = 0.0;
        }
        actual.push(pendiente);
        total += segundos;
    }
    if !actual.is_empty() {
        lotes.push(actual);
    }
    lotes
}

/// `valor` to `decimales` places.
pub(crate) fn redondear(valor: f64, decimales: i32) -> f64 {
    let escala = 10f64.powi(decimales);
    (valor * escala).round() / escala
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::inventario::{Imagen, Video};

    #[test]
    fn the_planner_s_line_is_the_engine_s() {
        let descripcion = Descripcion {
            descripcion: Some("Una mujer habla en la plaza".into()),
            plano: Some("medio".into()),
            uso: Some("toma_a_camara".into()),
            problemas: vec!["movido".into(), "oscuro".into()],
            momentos: vec![Momento {
                desde: 1.0,
                hasta: 3.5,
                que: Some("saluda".into()),
            }],
            visto_hasta: Some(6.0),
            ..Descripcion::default()
        };
        assert_eq!(
            descripcion.texto(),
            "Una mujer habla en la plaza · plano medio · uso: toma_a_camara · \
             problemas: movido, oscuro · momentos: 1.0-3.5s saluda · (visto solo hasta 6.0s)"
        );
        assert_eq!(Descripcion::default().texto(), "");
    }

    #[test]
    fn what_is_left_is_what_has_no_description_and_a_file() {
        let analisis = json!({
            "clips": [
                {"id": "A", "duracion": 30.0, "transcripcion": {"idioma": "eu", "texto": "kaixo"}},
                {"id": "B", "duracion": 8.0, "transcripcion": "", "descripcion": {"descripcion": "x"}},
                {"id": "C", "duracion": 20.0, "transcripcion": null},
                {"id": "fantasma", "duracion": 3.0},
            ],
            "imagenes": [{"id": "F", "caras": []}],
        });
        let inventario = Inventario {
            videos: ["A", "B", "C"]
                .map(|id| Video {
                    id: id.into(),
                    ruta: format!("{id}.mov").into(),
                    ..Video::default()
                })
                .to_vec(),
            imagenes: vec![Imagen {
                id: "F".into(),
                ruta: "F.jpg".into(),
                ..Imagen::default()
            }],
            ..Inventario::default()
        };

        let pendientes = pendientes(analisis.as_object().unwrap(), &inventario);

        let ids: Vec<&str> = pendientes.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["A", "C", "F"]);
        let segundos: Vec<f64> = pendientes.iter().map(Pendiente::segundos_en_contacto).collect();
        assert_eq!(segundos, [MAX_SEG_VOZ, MAX_SEG_RECURSO, MAX_SEG_FOTO]);
    }

    #[test]
    fn a_contact_video_runs_fifteen_minutes_at_most() {
        let clip = |n: usize| Pendiente {
            id: format!("c{n}"),
            ruta: PathBuf::new(),
            tipo: Tipo::Clip {
                duracion: 400.0,
                con_voz: false,
            },
        };
        let largos: Vec<Pendiente> = (0..80).map(clip).collect();
        let lotes = lotes(largos);
        assert_eq!(lotes.len(), 2);
        assert_eq!(lotes[0].len(), 75);
        assert_eq!(lotes[1].len(), 5);
        assert!(super::lotes(Vec::new()).is_empty());
    }
}
