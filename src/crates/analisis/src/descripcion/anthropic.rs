//! Claude looking at the strips of frames through Anthropic's Messages
//! API: the sheets as images, the table of ids as text, the answer read
//! as it streams. Thinking stays adaptive at low effort - describing needs
//! little - and a request a safety classifier declines is retried on the
//! model Anthropic recommends (`fallbacks: "default"`). The engine's
//! `_describir_anthropic`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

const API: &str = "https://api.anthropic.com/v1/messages";
/// Asked, with no body, why a request was turned down.
const MODELOS: &str = "https://api.anthropic.com/v1/models?limit=1";
const VERSION: &str = "2023-06-01";
/// Server-side fallback by refusal category.
const BETA_RESPALDO: &str = "server-side-fallback-2026-07-01";
/// Room for every clip's description in one answer.
const MAX_TOKENS: u32 = 32_000;
/// The API takes up to 100 images a request; a few are kept free.
pub const HOJAS_POR_PETICION: usize = 80;

/// Why Claude gave no description.
#[derive(Clone, Debug, PartialEq)]
pub enum ErrorClaude {
    /// The request was declined, the fallback included.
    Rechazo,
    /// The answer stopped at `max_tokens`.
    Cortado,
    /// The API failed: its status, when it answered, and what it said.
    Api(Option<u16>, String),
}

impl std::fmt::Display for ErrorClaude {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorClaude::Rechazo => write!(f, "anthropic: el modelo rechazó describir el material"),
            ErrorClaude::Cortado => write!(f, "anthropic: la respuesta se cortó por larga"),
            ErrorClaude::Api(Some(codigo), texto) => write!(f, "anthropic: {codigo}: {texto}"),
            ErrorClaude::Api(None, texto) => write!(f, "anthropic: {texto}"),
        }
    }
}

/// Anthropic's API with a key, and the model to ask.
pub struct Claude {
    clave: String,
    modelo: String,
    agente: ureq::Agent,
}

impl Claude {
    /// The API with `clave`, asking `modelo`.
    pub fn new(clave: String, modelo: String) -> Self {
        let agente = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(5 * 60))
            .build();
        Self { clave, modelo, agente }
    }

    /// Shows Claude the JPEG `hojas` and asks `texto`; returns what it
    /// wrote.
    pub fn describir(&self, hojas: &[PathBuf], texto: &str) -> Result<String, ErrorClaude> {
        let mut contenido = hojas
            .iter()
            .map(|hoja| {
                let datos = std::fs::read(hoja).map_err(|e| ErrorClaude::Api(None, e.to_string()))?;
                Ok(json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg",
                    "data": base64::engine::general_purpose::STANDARD.encode(datos)}}))
            })
            .collect::<Result<Vec<_>, ErrorClaude>>()?;
        contenido.push(json!({"type": "text", "text": texto}));
        let cuerpo = json!({
            "model": self.modelo,
            "max_tokens": MAX_TOKENS,
            "stream": true,
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": "low"},
            "fallbacks": "default",
            "messages": [{"role": "user", "content": contenido}],
        });
        let respuesta = self
            .agente
            .post(API)
            .set("x-api-key", &self.clave)
            .set("anthropic-version", VERSION)
            .set("anthropic-beta", BETA_RESPALDO)
            .send_json(cuerpo)
            .map_err(|error| self.por_que(fallo(error)))?;
        let mut flujo = Flujo::default();
        for linea in BufReader::new(respuesta.into_reader()).lines() {
            let linea = linea.map_err(|e| ErrorClaude::Api(None, e.to_string()))?;
            if let Some(datos) = linea.strip_prefix("data:") {
                flujo.suceso(datos.trim())?;
            }
        }
        flujo.final_()
    }

    /// The API turns a request down - a bad key, say - before the sheets
    /// are all sent, and closes the connection under them: what came back
    /// is then only a broken connection. A request with no body asks why;
    /// `error` stays when that one goes through, or breaks too.
    fn por_que(&self, error: ErrorClaude) -> ErrorClaude {
        if !matches!(error, ErrorClaude::Api(None, _)) {
            return error;
        }
        let prueba = self
            .agente
            .get(MODELOS)
            .set("x-api-key", &self.clave)
            .set("anthropic-version", VERSION)
            .call();
        match prueba {
            Err(rechazo @ ureq::Error::Status(..)) => fallo(rechazo),
            _ => error,
        }
    }
}

/// What has streamed so far.
#[derive(Debug, Default)]
struct Flujo {
    texto: String,
    parada: Option<String>,
}

impl Flujo {
    /// One server-sent event's data.
    fn suceso(&mut self, datos: &str) -> Result<(), ErrorClaude> {
        let Ok(suceso) = serde_json::from_str::<Value>(datos) else {
            return Ok(());
        };
        match suceso["type"].as_str().unwrap_or_default() {
            // A declined model's partial answer is dropped; the fallback's
            // follows the switch.
            "content_block_start" if suceso["content_block"]["type"] == "fallback" => self.texto.clear(),
            "content_block_delta" if suceso["delta"]["type"] == "text_delta" => {
                self.texto.push_str(suceso["delta"]["text"].as_str().unwrap_or_default());
            }
            "message_delta" => {
                if let Some(parada) = suceso["delta"]["stop_reason"].as_str() {
                    self.parada = Some(parada.to_owned());
                }
            }
            "error" => {
                let mensaje = suceso["error"]["message"].as_str().unwrap_or("error");
                return Err(ErrorClaude::Api(None, mensaje.to_owned()));
            }
            _ => {}
        }
        Ok(())
    }

    fn final_(self) -> Result<String, ErrorClaude> {
        match self.parada.as_deref() {
            Some("refusal") => Err(ErrorClaude::Rechazo),
            Some("max_tokens") => Err(ErrorClaude::Cortado),
            _ => Ok(self.texto),
        }
    }
}

fn fallo(error: ureq::Error) -> ErrorClaude {
    match error {
        ureq::Error::Status(codigo, respuesta) => {
            let cuerpo = respuesta.into_string().unwrap_or_default();
            let mensaje = serde_json::from_str::<Value>(&cuerpo)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or(cuerpo);
            ErrorClaude::Api(Some(codigo), mensaje.chars().take(300).collect())
        }
        ureq::Error::Transport(error) => ErrorClaude::Api(None, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flujo(sucesos: &[Value]) -> Result<String, ErrorClaude> {
        let mut flujo = Flujo::default();
        for suceso in sucesos {
            flujo.suceso(&suceso.to_string())?;
        }
        flujo.final_()
    }

    fn texto(t: &str) -> Value {
        json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": t}})
    }

    fn parada(motivo: &str) -> Value {
        json!({"type": "message_delta", "delta": {"stop_reason": motivo}})
    }

    #[test]
    fn the_text_is_put_together_from_its_deltas() {
        let sucesos = [
            json!({"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": "x"}}),
            texto("{\"clips\""),
            texto(": []}"),
            parada("end_turn"),
        ];
        assert_eq!(flujo(&sucesos).unwrap(), "{\"clips\": []}");
    }

    #[test]
    fn what_a_declined_model_wrote_gives_way_to_its_fallback() {
        let sucesos = [
            texto("a medias"),
            json!({"type": "content_block_start", "content_block": {"type": "fallback"}}),
            texto("{}"),
            parada("end_turn"),
        ];
        assert_eq!(flujo(&sucesos).unwrap(), "{}");
    }

    #[test]
    fn a_refusal_a_cut_and_an_error_are_failures() {
        assert_eq!(flujo(&[parada("refusal")]), Err(ErrorClaude::Rechazo));
        assert_eq!(flujo(&[texto("{"), parada("max_tokens")]), Err(ErrorClaude::Cortado));
        let error = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        assert_eq!(flujo(&[error]), Err(ErrorClaude::Api(None, "Overloaded".into())));
    }
}
