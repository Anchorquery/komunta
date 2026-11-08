//! Gemini planning by API key. The conversation is kept here, not by
//! Google, so it can go on with another model when the chosen one runs
//! out of today's quota or is overloaded: each model has a quota of its
//! own, counted in the cache shared with the description. The engine's
//! `Google` in `cerebros/apis.py`.

use std::path::{Path, PathBuf};

use analisis::descripcion::cuota::Cuota;
use analisis::descripcion::google::{ErrorGemini, Gemini, con_reintentos, texto_de};
use base64::Engine;
use serde_json::{Value, json};

use super::cadena::Entorno;
use super::cerebro::{Conector, Fallo};

const MODELO: &str = "gemini-flash-latest";

/// Gemini by API key.
pub struct Google {
    gemini: Option<Gemini>,
    modelos: Vec<String>,
    cuota: Cuota,
    historial: Vec<Value>,
    uso: Vec<Value>,
}

impl Google {
    /// Gemini as `ia.google` sets it, with key `clave`.
    pub fn new(entorno: &Entorno, clave: Option<String>) -> Self {
        Self {
            gemini: clave.map(Gemini::new),
            modelos: modelos(entorno.opciones("google")),
            cuota: Cuota::en(entorno.cuota.clone()),
            historial: Vec::new(),
            uso: Vec::new(),
        }
    }

    fn enviar(&mut self, partes: Vec<Value>) -> Result<String, Fallo> {
        let gemini = self.gemini.as_ref().ok_or_else(|| Fallo::Otro("google: no hay clave de API".into()))?;
        let turno = json!({"role": "user", "parts": partes});
        let mut turnos = self.historial.clone();
        turnos.push(turno.clone());
        let ajustes = json!({"responseMimeType": "application/json"});
        let respuesta = con_reintentos(
            &self.cuota,
            &self.modelos,
            |modelo| gemini.conversar(modelo, &turnos, ajustes.clone()),
            std::thread::sleep,
        )
        .map_err(|error| match error {
            ErrorGemini::CuotaAgotada(renovacion) => Fallo::Cuota(renovacion),
            otro => Fallo::Otro(otro.to_string()),
        })?;
        let texto = texto_de(&respuesta);
        self.historial.push(turno);
        self.historial.push(json!({"role": "model", "parts": [{"text": texto}]}));
        self.uso.push(respuesta["usageMetadata"].clone());
        Ok(texto)
    }
}

impl Conector for Google {
    fn nombre(&self) -> &'static str {
        "google"
    }

    fn primera(&mut self, _: &Path, pedido: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        let mut partes = Vec::new();
        for hoja in hojas {
            let datos = base64::engine::general_purpose::STANDARD.encode(std::fs::read(hoja)?);
            partes.push(json!({"inlineData": {"mimeType": "image/jpeg", "data": datos}}));
        }
        partes.push(json!({"text": pedido}));
        self.historial.clear();
        self.enviar(partes)
    }

    fn corregir(&mut self, _: &Path, mensaje: &str) -> Result<String, Fallo> {
        self.enviar(vec![json!({"text": mensaje})])
    }

    fn uso(&self) -> &[Value] {
        &self.uso
    }
}

/// The model chosen and, behind it, `respaldo`'s, once each: the engine's
/// `modelos_google`.
fn modelos(opciones: &Value) -> Vec<String> {
    let principal = opciones["modelo"].as_str().unwrap_or(MODELO);
    let respaldo = opciones["respaldo"].as_array().into_iter().flatten().filter_map(Value::as_str);
    let mut modelos: Vec<String> = Vec::new();
    for modelo in std::iter::once(principal).chain(respaldo) {
        if !modelo.is_empty() && !modelos.iter().any(|m| m == modelo) {
            modelos.push(modelo.to_owned());
        }
    }
    modelos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chosen_model_goes_first_then_the_backups_once() {
        let opciones = json!({"modelo": "b", "respaldo": ["a", "b", ""]});
        assert_eq!(modelos(&opciones), ["b", "a"]);
        assert_eq!(modelos(&Value::Null), [MODELO]);
    }
}
