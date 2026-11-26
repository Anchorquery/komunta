// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Gemini as the assistant's AI, by API key, with function calling. The
//! models are tried in order as the description and the plan try them:
//! one out of today's quota or overloaded gives way to the next, and every
//! call is counted in the quota they share - but at a chat's pace, a
//! person waiting: an overloaded model is tried again after a second and
//! three, then left.
//!
//! What Gemini asks of a caller, as its documentation had it in November
//! 2026: calls kept to their schemas (`VALIDATED`); tools declared all the
//! time and narrowed by `functionCallingConfig`, so the cached prefix
//! holds; pictures inside a function's response from Gemini 3 on; the
//! first call of each step signed, a call an older model made carrying the
//! value that stands for a signature; no temperature.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use analisis::descripcion::cuota::Cuota;
use analisis::descripcion::google::{self, Fallo, Ritmo, con_ritmo};
use base64::Engine;
use serde_json::{Map, Value, json};

use crate::conversacion::{Mensaje, Parte, Rol};
use crate::herramienta::{Herramienta, Imagen};
use crate::proveedor::{Esfuerzo, Modo, Pedido, Proveedor};

/// How long one answer may take before the model counts as silent and
/// the next is asked. A chat answer with little thinking takes seconds; a
/// model Google has too much demand for says nothing at all (3.8 Flash on
/// the free tier, November 2026), and is better left soon.
const ESPERA: Duration = Duration::from_secs(30);

/// How long one call may take from sending to the last byte. Google keeps
/// a call to a model it has too much demand for waiting before it answers
/// 503 - 12 s, 71 s, 244 s in November 2026 - and the person is looking at
/// «Pensando…» meanwhile: past this the next model is asked.
const PLAZO: Duration = Duration::from_secs(45);

/// What Gemini 3 takes in place of the signature of a call it did not
/// sign - one an older model made, before the turn moved to a newer one.
const SIN_FIRMA: &str = "skip_thought_signature_validator";

/// Gemini, with the models to try in order.
pub struct Gemini {
    cliente: google::Gemini,
    modelos: Vec<String>,
    cuota: Cuota,
    esfuerzo: Esfuerzo,
}

impl Gemini {
    /// Gemini with `clave`, trying `modelos` in order, its calls counted
    /// in the quota kept in `cuota`.
    pub fn new(clave: String, modelos: Vec<String>, cuota: PathBuf) -> Self {
        Self {
            cliente: google::Gemini::new(clave, ESPERA).con_plazo(PLAZO),
            modelos,
            cuota: Cuota::en(cuota),
            esfuerzo: Esfuerzo::Bajo,
        }
    }

    /// The same, thinking this much before each answer.
    pub fn con_esfuerzo(mut self, esfuerzo: Esfuerzo) -> Self {
        self.esfuerzo = esfuerzo;
        self
    }

    /// One request to `modelo`, sent again once when the model wrote a
    /// call wrong, and once without the tool whose schema Google refused -
    /// a net under a schema to fix, not a way of working.
    fn pedir(&self, pedido: &Pedido<'_>, modelo: &str) -> Result<Value, Fallo> {
        let mut herramientas: Vec<Herramienta> = pedido.herramientas.to_vec();
        let mut reintentado = false;
        loop {
            let cuerpo = con_pensamiento(&cuerpo(pedido, &herramientas, modelo), modelo, self.esfuerzo);
            let inicio = Instant::now();
            let respuesta = match self.cliente.pedir(modelo, &cuerpo) {
                Ok(respuesta) => respuesta,
                Err(fallo) => match herramienta_rechazada(&fallo, &herramientas) {
                    Some(n) if !reintentado => {
                        log::warn!(
                            "asistente: Google refused the schema of {}; asked without it",
                            herramientas[n].nombre
                        );
                        herramientas.remove(n);
                        reintentado = true;
                        continue;
                    }
                    _ => return Err(fallo),
                },
            };
            log::info!("asistente: {modelo} {} ms ({})", inicio.elapsed().as_millis(), uso(&respuesta));
            if !reintentado && respuesta["candidates"][0]["finishReason"] == "MALFORMED_FUNCTION_CALL" {
                log::warn!("asistente: {modelo} wrote a malformed call; asked again");
                reintentado = true;
                continue;
            }
            return Ok(respuesta);
        }
    }
}

impl Proveedor for Gemini {
    fn nombre(&self) -> String {
        match self.modelos.first() {
            Some(modelo) => format!("Gemini ({modelo})"),
            None => "Gemini".to_owned(),
        }
    }

    fn responder(&mut self, pedido: &Pedido<'_>) -> Result<Mensaje, String> {
        let respuesta = con_ritmo(
            &self.cuota,
            &self.modelos,
            Ritmo::CONVERSACION,
            |modelo| self.pedir(pedido, modelo),
            std::thread::sleep,
        )
        .map_err(|error| error.to_string())?;
        mensaje_de(&respuesta)
    }
}

/// Whether `modelo` is of the generations that take pictures inside a
/// function's response and check the signatures of calls: every one after
/// Gemini 2.
fn moderno(modelo: &str) -> bool {
    !modelo.starts_with("gemini-2") && !modelo.starts_with("gemini-1")
}

/// `cuerpo` for `modelo`, thinking as much as `esfuerzo` says. Little is
/// the default: a chat that edits a timeline needs seconds, not the long
/// reasoning a plan gets. Gemini 2 takes a budget of thought tokens - at
/// the least none for Flash, 128 for Pro; later models take a level, and
/// "low" is the least every one of them accepts (3.8 Flash refuses
/// "minimal", and a refusal fails the whole turn). No temperature: Gemini
/// 3.6 and later ignore it, and the next will refuse it.
fn con_pensamiento(cuerpo: &Value, modelo: &str, esfuerzo: Esfuerzo) -> Value {
    let pensamiento = if modelo.starts_with("gemini-2") {
        let minimo = if modelo.contains("-pro") { 128 } else { 0 };
        let presupuesto = match esfuerzo {
            Esfuerzo::Bajo => minimo,
            Esfuerzo::Medio => 2048,
            Esfuerzo::Alto => 8192,
        };
        json!({"thinkingBudget": presupuesto})
    } else {
        json!({"thinkingLevel": esfuerzo.id()})
    };
    let mut cuerpo = cuerpo.clone();
    cuerpo["generationConfig"] = json!({"thinkingConfig": pensamiento});
    cuerpo
}

/// The tokens an answer took, for the log: the cached ones say whether
/// the prefix held.
fn uso(respuesta: &Value) -> String {
    let uso = &respuesta["usageMetadata"];
    let n = |campo: &str| uso[campo].as_u64().unwrap_or(0);
    format!(
        "prompt {}, cached {}, thoughts {}, answer {}",
        n("promptTokenCount"),
        n("cachedContentTokenCount"),
        n("thoughtsTokenCount"),
        n("candidatesTokenCount")
    )
}

/// Which of `herramientas` Google's refusal names, when it refused one's
/// schema: "function_declarations[3]".
fn herramienta_rechazada(fallo: &Fallo, herramientas: &[Herramienta]) -> Option<usize> {
    if fallo.codigo != Some(400) {
        return None;
    }
    let texto = fallo.texto.replace("functionDeclarations", "function_declarations");
    let marca = "function_declarations[";
    let resto = &texto[texto.find(marca)? + marca.len()..];
    let n: usize = resto[..resto.find(']')?].parse().ok()?;
    (n < herramientas.len()).then_some(n)
}

/// The `generateContent` request for `pedido` to `modelo`, declaring
/// `herramientas`.
fn cuerpo(pedido: &Pedido<'_>, herramientas: &[Herramienta], modelo: &str) -> Value {
    let mut cuerpo = json!({
        "systemInstruction": {"parts": [{"text": pedido.sistema}]},
        "contents": turnos(pedido.mensajes, modelo),
    });
    if !herramientas.is_empty() {
        let declaraciones: Vec<Value> = herramientas.iter().map(declaracion).collect();
        cuerpo["tools"] = json!([{"functionDeclarations": declaraciones}]);
        cuerpo["toolConfig"] = json!({"functionCallingConfig": llamadas_permitidas(&pedido.modo, herramientas)});
    }
    cuerpo
}

/// Which declared tools may be called. VALIDATED keeps the calls to their
/// schemas, words still allowed; NONE leaves the tools declared, so the
/// prefix stays cached, and asks for words.
fn llamadas_permitidas(modo: &Modo, herramientas: &[Herramienta]) -> Value {
    match modo {
        Modo::Libre => json!({"mode": "VALIDATED"}),
        Modo::SoloLeer(nombres) => {
            let declaradas: Vec<&String> = nombres
                .iter()
                .filter(|n| herramientas.iter().any(|h| &h.nombre == *n))
                .collect();
            if declaradas.is_empty() {
                json!({"mode": "NONE"})
            } else {
                json!({"mode": "VALIDATED", "allowedFunctionNames": declaradas})
            }
        }
        Modo::Ninguna => json!({"mode": "NONE"}),
    }
}

/// A tool as Gemini declares it. Its schema goes as JSON Schema, which
/// Gemini reads with references and alternatives; the older `parameters`
/// field takes only a subset of OpenAPI.
fn declaracion(herramienta: &Herramienta) -> Value {
    json!({
        "name": herramienta.nombre,
        "description": herramienta.descripcion,
        "parametersJsonSchema": herramienta.esquema,
    })
}

/// The messages as Gemini's turns for `modelo`. Two in a row from the same
/// side - the results that ended a turn, then the person's next words - go
/// as one.
fn turnos(mensajes: &[Mensaje], modelo: &str) -> Vec<Value> {
    let mut turnos: Vec<Value> = Vec::new();
    for turno in mensajes.iter().map(|m| turno_de(m, modelo)) {
        match turnos.last_mut() {
            Some(anterior) if anterior["role"] == turno["role"] => {
                let partes = turno["parts"].as_array().cloned().unwrap_or_default();
                if let Some(previas) = anterior["parts"].as_array_mut() {
                    previas.extend(partes);
                }
            }
            _ => turnos.push(turno),
        }
    }
    turnos
}

/// A message as a Gemini turn for `modelo`. One of Gemini's own goes back
/// exactly as it came, signatures and all; a newer model is handed, for a
/// call no model signed, the value it takes in place of a signature.
fn turno_de(mensaje: &Mensaje, modelo: &str) -> Value {
    // Another provider's raw form is not Gemini's to send.
    let propio = mensaje.crudo.as_ref().filter(|crudo| crudo.get("parts").is_some());
    let mut turno = match propio {
        Some(crudo) => crudo.clone(),
        None => {
            let rol = match mensaje.rol {
                Rol::Usuario => "user",
                Rol::Modelo => "model",
            };
            json!({"role": rol, "parts": partes_de(&mensaje.partes, modelo)})
        }
    };
    if mensaje.rol == Rol::Modelo && moderno(modelo) {
        firmar_primera_llamada(&mut turno);
    }
    turno
}

/// Gemini 3 checks the signature of the first call of each step; one an
/// older model made has none.
fn firmar_primera_llamada(turno: &mut Value) {
    let Some(partes) = turno["parts"].as_array_mut() else {
        return;
    };
    if let Some(llamada) = partes.iter_mut().find(|p| p.get("functionCall").is_some())
        && llamada.get("thoughtSignature").is_none()
    {
        llamada["thoughtSignature"] = json!(SIN_FIRMA);
    }
}

/// The parts of a message. Pictures go inside their function's response to
/// a model that takes them there; to an older one, after every response -
/// the responses to one step's calls go together.
fn partes_de(partes: &[Parte], modelo: &str) -> Vec<Value> {
    let dentro = moderno(modelo);
    let mut fuera = Vec::new();
    let mut todas: Vec<Value> = partes
        .iter()
        .map(|parte| {
            if let (false, Parte::Resultado { imagen: Some(imagen), .. }) = (dentro, parte) {
                fuera.push(json!({"inlineData": datos_de(imagen)}));
            }
            parte_de(parte, dentro)
        })
        .collect();
    todas.extend(fuera);
    todas
}

fn parte_de(parte: &Parte, imagen_dentro: bool) -> Value {
    match parte {
        Parte::Texto(texto) => json!({"text": texto}),
        Parte::Llamada {
            id,
            nombre,
            argumentos,
        } => {
            let mut llamada = json!({"name": nombre, "args": argumentos});
            con_id(&mut llamada, id);
            json!({"functionCall": llamada})
        }
        Parte::Resultado {
            id,
            nombre,
            valor,
            error,
            imagen,
        } => {
            let mut respuesta = json!({"name": nombre, "response": respuesta_de(valor, *error)});
            con_id(&mut respuesta, id);
            if let (true, Some(imagen)) = (imagen_dentro, imagen) {
                let nombre_imagen = format!("fotograma-{id}");
                respuesta["response"]["imagen"] = json!({"$ref": nombre_imagen});
                let mut datos = datos_de(imagen);
                datos["displayName"] = json!(nombre_imagen);
                respuesta["parts"] = json!([{"inlineData": datos}]);
            }
            json!({"functionResponse": respuesta})
        }
    }
}

fn datos_de(imagen: &Imagen) -> Value {
    json!({
        "mimeType": imagen.mime,
        "data": base64::engine::general_purpose::STANDARD.encode(&imagen.datos),
    })
}

/// Gemini takes a function's response as an object only.
fn respuesta_de(valor: &Value, error: bool) -> Value {
    match (valor, error) {
        (Value::Object(_), _) => valor.clone(),
        (_, true) => json!({"error": valor}),
        (otro, false) => json!({"result": otro}),
    }
}

fn con_id(objeto: &mut Value, id: &str) {
    if !id.is_empty() {
        objeto["id"] = json!(id);
    }
}

/// Gemini's answer as a message: its words - not its thoughts - and its
/// calls, with the answer itself kept to be sent back.
fn mensaje_de(respuesta: &Value) -> Result<Mensaje, String> {
    let candidato = &respuesta["candidates"][0];
    let contenido = &candidato["content"];
    let Some(partes_crudas) = contenido["parts"].as_array().filter(|p| !p.is_empty()) else {
        return Err(sin_respuesta(respuesta));
    };
    let partes: Vec<Parte> = partes_crudas.iter().filter_map(parte_leida).collect();
    let mut crudo = contenido.clone();
    crudo["role"] = json!("model");
    Ok(Mensaje {
        rol: Rol::Modelo,
        partes,
        crudo: Some(crudo),
    })
}

fn parte_leida(parte: &Value) -> Option<Parte> {
    if let Some(llamada) = parte.get("functionCall") {
        return Some(Parte::Llamada {
            id: llamada["id"].as_str().unwrap_or_default().to_owned(),
            nombre: llamada["name"].as_str().unwrap_or_default().to_owned(),
            argumentos: match &llamada["args"] {
                Value::Null => Value::Object(Map::new()),
                args => args.clone(),
            },
        });
    }
    let pensamiento = parte["thought"].as_bool().unwrap_or(false);
    match parte["text"].as_str() {
        Some(texto) if !pensamiento => Some(Parte::Texto(texto.to_owned())),
        _ => None,
    }
}

/// Why Gemini gave nothing to read, in a sentence.
fn sin_respuesta(respuesta: &Value) -> String {
    let motivo = respuesta["promptFeedback"]["blockReason"]
        .as_str()
        .or_else(|| respuesta["candidates"][0]["finishReason"].as_str())
        .unwrap_or("sin motivo");
    format!("Gemini no dio respuesta ({motivo}).")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herramienta::{Clase, NUCLEO};

    const NUEVO: &str = "gemini-3.8-flash";
    const VIEJO: &str = "gemini-2.5-flash";

    fn herramientas() -> Vec<Herramienta> {
        vec![
            Herramienta::nueva(
                "clips_split",
                "Corta clips",
                json!({"type": "object", "properties": {"time": {"type": "number"}}}),
                Clase::Edicion,
                NUCLEO,
            ),
            Herramienta::nueva("timeline_read", "Lee", json!({"type": "object"}), Clase::Lectura, NUCLEO),
        ]
    }

    fn pedido<'a>(mensajes: &'a [Mensaje], herramientas: &'a [Herramienta], modo: Modo) -> Pedido<'a> {
        Pedido {
            sistema: "Eres el asistente",
            mensajes,
            herramientas,
            modo,
        }
    }

    fn resultado(id: &str, valor: Value, imagen: Option<Imagen>) -> Parte {
        Parte::Resultado {
            id: id.into(),
            nombre: "ver_fotograma".into(),
            valor,
            error: false,
            imagen,
        }
    }

    fn foto() -> Imagen {
        Imagen {
            mime: "image/jpeg".into(),
            datos: vec![1, 2, 3],
            nota: "t=2 s".into(),
        }
    }

    #[test]
    fn the_request_carries_the_instruction_the_turns_the_tools_and_validated_calls() {
        let mensajes = [Mensaje::del_usuario("corta en 2 s")];
        let herramientas = herramientas();
        let cuerpo = cuerpo(&pedido(&mensajes, &herramientas, Modo::Libre), &herramientas, NUEVO);
        assert_eq!(cuerpo["systemInstruction"]["parts"][0]["text"], "Eres el asistente");
        assert_eq!(cuerpo["contents"][0], json!({"role": "user", "parts": [{"text": "corta en 2 s"}]}));
        let declarada = &cuerpo["tools"][0]["functionDeclarations"][0];
        assert_eq!(declarada["name"], "clips_split");
        assert_eq!(declarada["parametersJsonSchema"]["properties"]["time"]["type"], "number");
        assert_eq!(cuerpo["toolConfig"]["functionCallingConfig"], json!({"mode": "VALIDATED"}));
        assert!(cuerpo.get("generationConfig").is_none() && cuerpo.get("temperature").is_none());
    }

    #[test]
    fn near_the_end_the_tools_stay_declared_and_only_what_may_be_called_narrows() {
        let herramientas = herramientas();
        let leer = cuerpo(
            &pedido(&[], &herramientas, Modo::SoloLeer(vec!["timeline_read".into()])),
            &herramientas,
            NUEVO,
        );
        assert_eq!(leer["tools"][0]["functionDeclarations"].as_array().unwrap().len(), 2);
        assert_eq!(
            leer["toolConfig"]["functionCallingConfig"],
            json!({"mode": "VALIDATED", "allowedFunctionNames": ["timeline_read"]})
        );
        let cerrar = cuerpo(&pedido(&[], &herramientas, Modo::Ninguna), &herramientas, NUEVO);
        assert_eq!(cerrar["tools"][0]["functionDeclarations"].as_array().unwrap().len(), 2);
        assert_eq!(cerrar["toolConfig"]["functionCallingConfig"], json!({"mode": "NONE"}));
        let nada = cuerpo(&pedido(&[], &[], Modo::Libre), &[], NUEVO);
        assert!(nada.get("tools").is_none() && nada.get("toolConfig").is_none());
    }

    #[test]
    fn each_model_is_asked_to_think_as_little_as_it_can() {
        let nivel = |modelo, esfuerzo| {
            con_pensamiento(&json!({}), modelo, esfuerzo)["generationConfig"]["thinkingConfig"].clone()
        };
        assert_eq!(nivel("gemini-2.5-flash", Esfuerzo::Bajo), json!({"thinkingBudget": 0}));
        assert_eq!(nivel("gemini-2.5-pro", Esfuerzo::Bajo), json!({"thinkingBudget": 128}));
        assert_eq!(nivel("gemini-3.8-flash", Esfuerzo::Bajo), json!({"thinkingLevel": "low"}));
        assert_eq!(nivel("gemini-flash-latest", Esfuerzo::Bajo), json!({"thinkingLevel": "low"}));
    }

    #[test]
    fn more_effort_is_more_thought() {
        let nivel = |modelo, esfuerzo| {
            con_pensamiento(&json!({}), modelo, esfuerzo)["generationConfig"]["thinkingConfig"].clone()
        };
        assert_eq!(nivel("gemini-2.5-flash", Esfuerzo::Medio), json!({"thinkingBudget": 2048}));
        assert_eq!(nivel("gemini-2.5-pro", Esfuerzo::Alto), json!({"thinkingBudget": 8192}));
        assert_eq!(nivel("gemini-3.8-flash", Esfuerzo::Alto), json!({"thinkingLevel": "high"}));
    }

    #[test]
    fn an_answer_with_a_call_is_read_and_kept_to_be_sent_back_signed() {
        let respuesta = json!({"candidates": [{"content": {"role": "model", "parts": [
            {"text": "pensando...", "thought": true},
            {"functionCall": {"name": "timeline_read", "args": {}, "id": "call_1"}, "thoughtSignature": "firma"}
        ]}}]});
        let mensaje = mensaje_de(&respuesta).unwrap();
        assert_eq!(mensaje.texto(), "", "a thought is not said");
        let llamadas: Vec<_> = mensaje.llamadas().collect();
        assert_eq!(llamadas, [("call_1", "timeline_read", &json!({}))]);
        let devuelto = turno_de(&mensaje, NUEVO);
        assert_eq!(devuelto["parts"][1]["thoughtSignature"], "firma");
    }

    #[test]
    fn a_call_an_older_model_made_is_handed_to_a_newer_one_with_the_stand_in_signature() {
        let respuesta = json!({"candidates": [{"content": {"role": "model", "parts": [
            {"functionCall": {"name": "timeline_read", "args": {}}},
            {"functionCall": {"name": "clips_split", "args": {}}}
        ]}}]});
        let mensaje = mensaje_de(&respuesta).unwrap();
        let al_nuevo = turno_de(&mensaje, NUEVO);
        assert_eq!(al_nuevo["parts"][0]["thoughtSignature"], SIN_FIRMA);
        assert!(al_nuevo["parts"][1].get("thoughtSignature").is_none(), "only the first");
        assert!(turno_de(&mensaje, VIEJO)["parts"][0].get("thoughtSignature").is_none());
    }

    #[test]
    fn a_frame_goes_inside_its_response_to_gemini_3_and_after_the_responses_to_gemini_2() {
        let mensaje = Mensaje {
            rol: Rol::Usuario,
            partes: vec![
                resultado("c1", json!({"ok": true}), Some(foto())),
                resultado("c2", json!({"ok": true}), None),
            ],
            crudo: None,
        };
        let nuevo = turno_de(&mensaje, NUEVO);
        let respuesta = &nuevo["parts"][0]["functionResponse"];
        assert_eq!(respuesta["response"]["imagen"], json!({"$ref": "fotograma-c1"}));
        assert_eq!(respuesta["parts"][0]["inlineData"]["displayName"], "fotograma-c1");
        assert_eq!(respuesta["parts"][0]["inlineData"]["data"], "AQID");
        assert_eq!(nuevo["parts"].as_array().unwrap().len(), 2);

        let viejo = turno_de(&mensaje, VIEJO);
        let partes = viejo["parts"].as_array().unwrap();
        assert_eq!(partes.len(), 3);
        assert!(partes[0]["functionResponse"].get("parts").is_none());
        assert!(partes[1].get("functionResponse").is_some(), "the responses together");
        assert_eq!(partes[2]["inlineData"]["mimeType"], "image/jpeg");
    }

    #[test]
    fn results_go_back_as_function_responses_with_their_call_s_id() {
        let mensaje = Mensaje {
            rol: Rol::Usuario,
            partes: vec![
                Parte::Resultado {
                    id: "call_1".into(),
                    nombre: "timeline_read".into(),
                    valor: json!({"ok": true, "datos": {"clips": 3}}),
                    error: false,
                    imagen: None,
                },
                Parte::Resultado {
                    id: String::new(),
                    nombre: "clips_split".into(),
                    valor: json!("no such clip"),
                    error: true,
                    imagen: None,
                },
            ],
            crudo: None,
        };
        let turno = turno_de(&mensaje, NUEVO);
        assert_eq!(turno["role"], "user");
        assert_eq!(
            turno["parts"][0],
            json!({"functionResponse": {"name": "timeline_read", "id": "call_1", "response": {"ok": true, "datos": {"clips": 3}}}})
        );
        assert_eq!(
            turno["parts"][1],
            json!({"functionResponse": {"name": "clips_split", "response": {"error": "no such clip"}}})
        );
    }

    #[test]
    fn the_results_that_ended_a_turn_and_the_next_words_go_as_one_turn() {
        let resultado = Mensaje {
            rol: Rol::Usuario,
            partes: vec![resultado("c1", json!({"ok": true}), None)],
            crudo: None,
        };
        let turnos = turnos(&[resultado, Mensaje::del_usuario("y otro")], NUEVO);
        assert_eq!(turnos.len(), 1);
        assert_eq!(turnos[0]["parts"].as_array().unwrap().len(), 2);
        assert_eq!(turnos[0]["parts"][1], json!({"text": "y otro"}));
    }

    #[test]
    fn a_refused_schema_names_its_tool() {
        let herramientas = herramientas();
        let fallo = |codigo, texto: &str| Fallo {
            codigo: Some(codigo),
            texto: texto.into(),
        };
        let rechazo = r#"{"error": {"message": "* GenerateContentRequest.tools[0].function_declarations[1].parameters: bad"}}"#;
        assert_eq!(herramienta_rechazada(&fallo(400, rechazo), &herramientas), Some(1));
        assert_eq!(herramienta_rechazada(&fallo(503, rechazo), &herramientas), None);
        assert_eq!(herramienta_rechazada(&fallo(400, "API key not valid"), &herramientas), None);
    }

    #[test]
    fn an_empty_answer_says_why() {
        let bloqueada = json!({"promptFeedback": {"blockReason": "SAFETY"}});
        assert_eq!(mensaje_de(&bloqueada).unwrap_err(), "Gemini no dio respuesta (SAFETY).");
        let vacia = json!({"candidates": [{"finishReason": "MAX_TOKENS", "content": {}}]});
        assert!(mensaje_de(&vacia).unwrap_err().contains("MAX_TOKENS"));
    }
}
