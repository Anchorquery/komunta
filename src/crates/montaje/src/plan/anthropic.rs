//! Claude planning through Anthropic's Messages API: adaptive thinking,
//! the prompt cached for an hour - the correction and every later request
//! repeat it and are charged a tenth for it - and the model Anthropic
//! recommends when a safety classifier declines (`fallbacks: "default"`).
//! The answer streams in; the whole of it, thinking included, goes back
//! into the conversation. The engine's `Anthropic` in `cerebros/apis.py`.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use super::cadena::Entorno;
use super::cerebro::{Conector, Fallo};

const API: &str = "https://api.anthropic.com/v1/messages";
/// Asked, with no body, why a request was turned down.
const MODELOS: &str = "https://api.anthropic.com/v1/models?limit=1";
const VERSION: &str = "2023-06-01";
const BETA_RESPALDO: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 64_000;
const MODELO: &str = "claude-opus-5-5";
/// Where an edit's prompt leaves the original one (`edicion.py`): the
/// original is cached on its own.
const MARCA_EDICION: &str = "\n\n## Plan actual (ya montado";

/// Claude by API key.
pub struct Anthropic {
    clave: Option<String>,
    modelo: String,
    esfuerzo: String,
    agente: ureq::Agent,
    mensajes: Vec<Value>,
    uso: Vec<Value>,
}

impl Anthropic {
    /// Claude as `ia.anthropic` sets it, with key `clave`.
    pub fn new(entorno: &Entorno, clave: Option<String>) -> Self {
        let opciones = entorno.opciones("anthropic");
        let agente = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(entorno.limite())
            .build();
        Self {
            clave,
            modelo: opciones["modelo"].as_str().unwrap_or(MODELO).to_owned(),
            esfuerzo: opciones["esfuerzo"].as_str().unwrap_or("high").to_owned(),
            agente,
            mensajes: Vec::new(),
            uso: Vec::new(),
        }
    }

    fn llamar(&mut self) -> Result<String, Fallo> {
        let clave = self.clave.as_deref().ok_or_else(|| Fallo::Otro("anthropic: no hay clave de API".into()))?;
        let cuerpo = json!({
            "model": self.modelo,
            "max_tokens": MAX_TOKENS,
            "stream": true,
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": self.esfuerzo},
            "fallbacks": "default",
            // The growing conversation is reused too, with the prompt's
            // own lifetime: on the first request both mark the same block.
            "cache_control": {"type": "ephemeral", "ttl": "1h"},
            "messages": self.mensajes,
        });
        let respuesta = self
            .agente
            .post(API)
            .set("x-api-key", clave)
            .set("anthropic-version", VERSION)
            .set("anthropic-beta", BETA_RESPALDO)
            .send_json(cuerpo)
            .map_err(|error| self.por_que(error, clave))?;
        let mut mensaje = Mensaje::default();
        for linea in BufReader::new(respuesta.into_reader()).lines() {
            let linea = linea.map_err(|e| Fallo::Otro(format!("anthropic: {e}")))?;
            if let Some(datos) = linea.strip_prefix("data:") {
                mensaje.suceso(datos.trim())?;
            }
        }
        self.uso.push(mensaje.uso());
        if mensaje.parada.as_deref() == Some("refusal") {
            return Err(Fallo::Otro(format!(
                "anthropic: el modelo rechazó el pedido ({})",
                crate::python::texto(&mensaje.detalles)
            )));
        }
        let texto = mensaje.texto();
        self.mensajes.push(json!({"role": "assistant", "content": mensaje.bloques}));
        Ok(texto)
    }
}

impl Anthropic {
    /// The API turns a request down - a bad key, say - before the sheets
    /// are all sent, and closes the connection under them: a request with
    /// no body then asks why.
    fn por_que(&self, error: ureq::Error, clave: &str) -> Fallo {
        if matches!(error, ureq::Error::Status(..)) {
            return fallo(error);
        }
        let prueba = self.agente.get(MODELOS).set("x-api-key", clave).set("anthropic-version", VERSION).call();
        match prueba {
            Err(rechazo @ ureq::Error::Status(..)) => fallo(rechazo),
            _ => fallo(error),
        }
    }
}

impl Conector for Anthropic {
    fn nombre(&self) -> &'static str {
        "anthropic"
    }

    fn primera(&mut self, _: &Path, pedido: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        let mut contenido = Vec::new();
        for hoja in hojas {
            let datos = std::fs::read(hoja)?;
            let nombre = hoja.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            contenido.push(json!({"type": "text", "text": format!("Hoja {nombre}:")}));
            contenido.push(json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg",
                "data": base64::engine::general_purpose::STANDARD.encode(datos)}}));
        }
        contenido.extend(bloques_pedido(pedido));
        self.mensajes = vec![json!({"role": "user", "content": contenido})];
        self.llamar()
    }

    fn corregir(&mut self, _: &Path, mensaje: &str) -> Result<String, Fallo> {
        self.mensajes.push(json!({"role": "user", "content": mensaje}));
        self.llamar()
    }

    fn uso(&self) -> &[Value] {
        &self.uso
    }
}

/// The prompt as text blocks: the original one cached for an hour, and an
/// edit's request after it, not cached.
fn bloques_pedido(pedido: &str) -> Vec<Value> {
    let (base, resto) = match pedido.find(MARCA_EDICION) {
        Some(corte) => (&pedido[..corte], Some(&pedido[corte..])),
        None => (pedido, None),
    };
    let mut bloques = vec![json!({"type": "text", "text": base,
        "cache_control": {"type": "ephemeral", "ttl": "1h"}})];
    if let Some(resto) = resto {
        bloques.push(json!({"type": "text", "text": resto.trim_start_matches('\n')}));
    }
    bloques
}

/// The answer as it streams in: its content blocks, why it stopped and
/// what it cost.
#[derive(Debug, Default)]
struct Mensaje {
    bloques: Vec<Value>,
    parada: Option<String>,
    detalles: Value,
    consumo: serde_json::Map<String, Value>,
}

impl Mensaje {
    /// One server-sent event's data.
    fn suceso(&mut self, datos: &str) -> Result<(), Fallo> {
        let Ok(suceso) = serde_json::from_str::<Value>(datos) else {
            return Ok(());
        };
        match suceso["type"].as_str().unwrap_or_default() {
            "message_start" => self.sumar(&suceso["message"]["usage"]),
            // A declined model's partial answer is dropped; the fallback's
            // follows the switch.
            "content_block_start" if suceso["content_block"]["type"] == "fallback" => self.bloques.clear(),
            "content_block_start" => self.bloques.push(suceso["content_block"].clone()),
            "content_block_delta" => self.anadir(&suceso["delta"]),
            "message_delta" => {
                if let Some(parada) = suceso["delta"]["stop_reason"].as_str() {
                    self.parada = Some(parada.to_owned());
                }
                self.detalles = suceso["delta"]["stop_details"].clone();
                self.sumar(&suceso["usage"]);
            }
            "error" => {
                let mensaje = suceso["error"]["message"].as_str().unwrap_or("error");
                return Err(Fallo::Otro(format!("anthropic: {mensaje}")));
            }
            _ => {}
        }
        Ok(())
    }

    /// A delta into the last block.
    fn anadir(&mut self, delta: &Value) {
        let Some(bloque) = self.bloques.last_mut() else {
            return;
        };
        let (campo, trozo) = match delta["type"].as_str().unwrap_or_default() {
            "text_delta" => ("text", &delta["text"]),
            "thinking_delta" => ("thinking", &delta["thinking"]),
            "signature_delta" => {
                bloque["signature"] = delta["signature"].clone();
                return;
            }
            _ => return,
        };
        let antes = bloque[campo].as_str().unwrap_or_default();
        bloque[campo] = Value::from(format!("{antes}{}", trozo.as_str().unwrap_or_default()));
    }

    fn sumar(&mut self, uso: &Value) {
        if let Some(uso) = uso.as_object() {
            self.consumo.extend(uso.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }

    fn texto(&self) -> String {
        self.bloques
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect()
    }

    fn uso(&self) -> Value {
        let mut uso = self.consumo.clone();
        uso.insert("stop".into(), self.parada.clone().into());
        Value::Object(uso)
    }
}

/// A failed request in a sentence: the API's own message when it answered.
fn fallo(error: ureq::Error) -> Fallo {
    match error {
        ureq::Error::Status(codigo, respuesta) => {
            let cuerpo = respuesta.into_string().unwrap_or_default();
            let mensaje = serde_json::from_str::<Value>(&cuerpo)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or(cuerpo);
            Fallo::Otro(format!("anthropic: {codigo}: {}", mensaje.chars().take(300).collect::<String>()))
        }
        ureq::Error::Transport(error) => Fallo::Otro(format!("anthropic: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flujo(sucesos: &[Value]) -> Mensaje {
        let mut mensaje = Mensaje::default();
        for suceso in sucesos {
            mensaje.suceso(&suceso.to_string()).unwrap();
        }
        mensaje
    }

    #[test]
    fn the_whole_answer_is_kept_thinking_included() {
        let mensaje = flujo(&[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 10}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "pien"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "so"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "f"}}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "{\"a\""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": ": 1}"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 5}}),
        ]);
        assert_eq!(mensaje.texto(), "{\"a\": 1}");
        assert_eq!(mensaje.bloques[0], json!({"type": "thinking", "thinking": "pienso", "signature": "f"}));
        assert_eq!(mensaje.uso(), json!({"input_tokens": 10, "output_tokens": 5, "stop": "end_turn"}));
    }

    #[test]
    fn a_declined_model_s_blocks_give_way_to_the_fallback_s() {
        let mensaje = flujo(&[
            json!({"type": "content_block_start", "content_block": {"type": "text", "text": "medio"}}),
            json!({"type": "content_block_start", "content_block": {"type": "fallback"}}),
            json!({"type": "content_block_start", "content_block": {"type": "text", "text": "bien"}}),
        ]);
        assert_eq!(mensaje.texto(), "bien");
    }

    #[test]
    fn an_edit_s_request_is_kept_out_of_the_cached_prompt() {
        let bloques = bloques_pedido("pedido\n\n## Plan actual (ya montado; x)\nquita esto");
        assert_eq!(bloques[0]["text"], "pedido");
        assert_eq!(bloques[0]["cache_control"]["ttl"], "1h");
        assert_eq!(bloques[1], json!({"type": "text", "text": "## Plan actual (ya montado; x)\nquita esto"}));
        assert_eq!(bloques_pedido("solo").len(), 1);
    }
}
