//! OpenAI planning through the Responses API; the correction follows on
//! from the last answer (`previous_response_id`). The engine's `OpenAI` in
//! `cerebros/apis.py`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use super::cadena::Entorno;
use super::cerebro::{Conector, Fallo};

const API: &str = "https://api.openai.com/v1/responses";
const MODELO: &str = "gpt-5.5";

/// OpenAI by API key.
pub struct OpenAi {
    clave: Option<String>,
    modelo: String,
    razonamiento: Option<String>,
    agente: ureq::Agent,
    previa: Option<String>,
    uso: Vec<Value>,
}

impl OpenAi {
    /// OpenAI as `ia.openai` sets it, with key `clave`.
    pub fn new(entorno: &Entorno, clave: Option<String>) -> Self {
        let opciones = entorno.opciones("openai");
        let agente = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout(entorno.limite())
            .build();
        Self {
            clave,
            modelo: opciones["modelo"].as_str().unwrap_or(MODELO).to_owned(),
            razonamiento: opciones["razonamiento"].as_str().filter(|r| !r.is_empty()).map(str::to_owned),
            agente,
            previa: None,
            uso: Vec::new(),
        }
    }

    fn llamar(&mut self, entrada: Value) -> Result<String, Fallo> {
        let clave = self.clave.as_deref().ok_or_else(|| Fallo::Otro("openai: no hay clave de API".into()))?;
        let mut cuerpo = json!({"model": self.modelo, "input": entrada,
            "text": {"format": {"type": "json_object"}}});
        if let Some(previa) = &self.previa {
            cuerpo["previous_response_id"] = previa.clone().into();
        }
        if let Some(razonamiento) = &self.razonamiento {
            cuerpo["reasoning"] = json!({"effort": razonamiento});
        }
        let respuesta: Value = self
            .agente
            .post(API)
            .set("Authorization", &format!("Bearer {clave}"))
            .send_json(cuerpo)
            .map_err(fallo)?
            .into_json()
            .map_err(|e| Fallo::Otro(format!("openai: {e}")))?;
        self.previa = respuesta["id"].as_str().map(str::to_owned);
        self.uso.push(respuesta["usage"].clone());
        Ok(texto_de(&respuesta))
    }
}

impl Conector for OpenAi {
    fn nombre(&self) -> &'static str {
        "openai"
    }

    fn primera(&mut self, _: &Path, pedido: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        let mut contenido = vec![json!({"type": "input_text", "text": pedido})];
        for hoja in hojas {
            let datos = base64::engine::general_purpose::STANDARD.encode(std::fs::read(hoja)?);
            contenido.push(json!({"type": "input_image", "image_url": format!("data:image/jpeg;base64,{datos}")}));
        }
        self.previa = None;
        self.llamar(json!([{"role": "user", "content": contenido}]))
    }

    fn corregir(&mut self, _: &Path, mensaje: &str) -> Result<String, Fallo> {
        self.llamar(json!([{"role": "user", "content": mensaje}]))
    }

    fn uso(&self) -> &[Value] {
        &self.uso
    }
}

/// What the model wrote: the text of its output messages (the SDK's
/// `output_text`).
fn texto_de(respuesta: &Value) -> String {
    let partes = respuesta["output"].as_array().into_iter().flatten();
    partes
        .filter(|parte| parte["type"] == "message")
        .flat_map(|mensaje| mensaje["content"].as_array().into_iter().flatten())
        .filter(|contenido| contenido["type"] == "output_text")
        .filter_map(|contenido| contenido["text"].as_str())
        .collect()
}

fn fallo(error: ureq::Error) -> Fallo {
    match error {
        ureq::Error::Status(codigo, respuesta) => {
            let cuerpo = respuesta.into_string().unwrap_or_default();
            let mensaje = serde_json::from_str::<Value>(&cuerpo)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or(cuerpo);
            Fallo::Otro(format!("openai: {codigo}: {}", mensaje.chars().take(300).collect::<String>()))
        }
        ureq::Error::Transport(error) => Fallo::Otro(format!("openai: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_the_text_of_its_messages() {
        let respuesta = json!({"output": [
            {"type": "reasoning", "summary": []},
            {"type": "message", "content": [{"type": "output_text", "text": "{\"a\""},
                                            {"type": "output_text", "text": ": 1}"}]},
        ]});
        assert_eq!(texto_de(&respuesta), "{\"a\": 1}");
    }
}
