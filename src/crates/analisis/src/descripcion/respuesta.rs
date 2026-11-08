//! What an AI answers when asked to describe the material: a JSON object
//! with a `clips` list, one entry per id - often wrapped in a Markdown
//! fence, sometimes with the `//` comments of the example it was shown.

use std::collections::HashMap;

use serde_json::Value;

use super::{Descripcion, Momento, redondear};

/// The JSON object in an AI's answer: inside its ```json fence when it has
/// one, else from its first `{` to its last `}`, without `//` comments.
/// None when there is no object to read.
pub fn extraer_json(texto: &str) -> Option<Value> {
    leer_json(texto).ok()
}

/// [`extraer_json`], saying why there is no object: what an AI is told
/// when its answer has to be corrected.
pub fn leer_json(texto: &str) -> Result<Value, String> {
    let cuerpo = dentro_de_cerco(texto).unwrap_or(texto);
    let objeto = match (cuerpo.find('{'), cuerpo.rfind('}')) {
        (Some(desde), Some(hasta)) if desde < hasta => &cuerpo[desde..=hasta],
        _ => return Err("no hay ningún objeto JSON en la respuesta".into()),
    };
    serde_json::from_str(&sin_comentarios(objeto)).map_err(|e| e.to_string())
}

/// What lies between the first ``` fence and the last.
fn dentro_de_cerco(texto: &str) -> Option<&str> {
    let abre = texto.find("```")? + 3;
    let cierra = texto.rfind("```")?;
    (cierra > abre).then(|| &texto[abre..cierra])
}

/// `json` without its `//` comments, which a model copies from the
/// example in the prompt; a `//` inside a string - a link - stays.
fn sin_comentarios(json: &str) -> String {
    let mut salida = String::with_capacity(json.len());
    let (mut en_texto, mut escapado) = (false, false);
    let mut letras = json.chars().peekable();
    while let Some(letra) = letras.next() {
        if en_texto {
            salida.push(letra);
            match letra {
                _ if escapado => escapado = false,
                '\\' => escapado = true,
                '"' => en_texto = false,
                _ => {}
            }
        } else if letra == '/' && letras.peek() == Some(&'/') {
            while letras.next_if(|l| *l != '\n').is_some() {}
        } else {
            en_texto = letra == '"';
            salida.push(letra);
        }
    }
    salida
}

/// The entries of an answer's `clips` list, by their id.
pub fn por_id(respuesta: &Value) -> HashMap<String, Value> {
    respuesta
        .get("clips")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entrada| Some((entrada.get("id")?.as_str()?.to_owned(), entrada.clone())))
        .collect()
}

/// Where a clip was in what the AI saw, in seconds of that: `desde` to
/// `hasta` in a contact video, or 0 to its length in a strip of frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tramo {
    /// Its start.
    pub desde: f64,
    /// Its end.
    pub hasta: f64,
}

/// The description in an answer's `entrada` for a clip `duracion` seconds
/// long (0 for a still) that the AI saw at `tramo`: its useful stretches
/// moved to the clip's own seconds and kept inside it.
pub fn descripcion(entrada: &Value, tramo: Tramo, duracion: f64) -> Descripcion {
    Descripcion {
        descripcion: texto(entrada, "descripcion"),
        plano: texto(entrada, "plano"),
        etiquetas: lista(entrada, "etiquetas"),
        problemas: lista(entrada, "problemas"),
        uso: texto(entrada, "uso"),
        momentos: momentos(entrada.get("momentos"), tramo, duracion),
        visto_hasta: None,
        visto_por: None,
    }
}

fn texto(entrada: &Value, clave: &str) -> Option<String> {
    match entrada.get(clave)? {
        Value::String(texto) => Some(texto.clone()),
        Value::Null => None,
        otro => Some(otro.to_string()),
    }
}

fn lista(entrada: &Value, clave: &str) -> Vec<String> {
    let cosas = entrada.get(clave).and_then(Value::as_array);
    cosas
        .into_iter()
        .flatten()
        .map(|cosa| cosa.as_str().map_or_else(|| cosa.to_string(), str::to_owned))
        .collect()
}

/// The stretches in `momentos`, from the seconds of what the AI saw to the
/// clip's: one that starts within a second of `tramo` is taken to be in
/// the contact video's time, any other in the clip's own. Each is kept
/// inside the clip, and dropped when nothing is left.
fn momentos(momentos: Option<&Value>, tramo: Tramo, duracion: f64) -> Vec<Momento> {
    let dentro = |t: f64| t.clamp(0.0, duracion.max(0.0));
    momentos
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|momento| {
            let numero = |clave| segundos(momento.get(clave));
            let (mut desde, mut hasta) = (numero("desde")?, numero("hasta")?);
            if tramo.desde - 1.0 <= desde && desde <= tramo.hasta + 1.0 {
                (desde, hasta) = (desde - tramo.desde, hasta - tramo.desde);
            }
            let (desde, hasta) = (dentro(desde), dentro(hasta));
            (hasta > desde).then(|| Momento {
                desde: redondear(desde, 1),
                hasta: redondear(hasta, 1),
                que: texto(momento, "que"),
            })
        })
        .collect()
}

/// A second as a model writes it: a number, a numeral in a string, or
/// missing (0, as the engine reads it).
fn segundos(valor: Option<&Value>) -> Option<f64> {
    match valor {
        None | Some(Value::Null) => Some(0.0),
        Some(Value::Number(numero)) => numero.as_f64(),
        Some(Value::String(texto)) => texto.trim().parse().ok(),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_object_is_found_in_a_fence_with_the_example_s_comments() {
        let texto = "Aquí tienes:\n```json\n{\"clips\": [{\"id\": \"A\", // el id\n \
                     \"descripcion\": \"ver https://x.eus\"}]}\n```\nListo.";
        let json = extraer_json(texto).unwrap();
        assert_eq!(json["clips"][0]["descripcion"], "ver https://x.eus");
        assert_eq!(extraer_json("{\"a\": 1}").unwrap(), json!({"a": 1}));
        assert!(extraer_json("sin nada").is_none());
    }

    #[test]
    fn entries_are_found_by_id_and_odd_ones_skipped() {
        let respuesta = json!({"clips": [{"id": "A"}, {"sin": "id"}, "texto", {"id": "B"}]});
        let mut ids: Vec<String> = por_id(&respuesta).into_keys().collect();
        ids.sort();
        assert_eq!(ids, ["A", "B"]);
        assert!(por_id(&json!({})).is_empty());
    }

    #[test]
    fn stretches_move_from_the_contact_video_to_the_clip() {
        let entrada = json!({
            "descripcion": "Plaza con gente",
            "plano": "general",
            "etiquetas": ["plaza", 3],
            "problemas": null,
            "uso": "recurso",
            "momentos": [
                {"desde": 101.0, "hasta": 104.26, "que": "pasa un coche"},
                {"desde": "2", "hasta": 50.0, "que": "en segundos del clip"},
                {"desde": 120.0, "hasta": 119.0},
                "nada",
            ],
        });
        let tramo = Tramo { desde: 100.0, hasta: 112.0 };

        let descripcion = descripcion(&entrada, tramo, 20.0);

        assert_eq!(descripcion.descripcion.as_deref(), Some("Plaza con gente"));
        assert_eq!(descripcion.etiquetas, ["plaza", "3"]);
        assert!(descripcion.problemas.is_empty());
        assert_eq!(
            descripcion.momentos,
            [
                Momento { desde: 1.0, hasta: 4.3, que: Some("pasa un coche".into()) },
                Momento { desde: 2.0, hasta: 20.0, que: Some("en segundos del clip".into()) },
            ]
        );
    }
}
