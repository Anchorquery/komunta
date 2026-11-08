//! Gemini watching a contact video through Google's API: the video goes in
//! the request when small, through the files API when not, and the call
//! moves down the list of models as each runs out of its daily quota or is
//! overloaded. The engine's `_describir_google` and `con_reintentos`.

use std::path::Path;
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, FixedOffset, Utc};
use serde_json::{Value, json};

use super::cuota::{self, Cuota};

const API: &str = "https://generativelanguage.googleapis.com";
/// The largest video sent in the request itself; a larger one is
/// uploaded first, which does not count as a request.
pub const MAX_EN_LINEA: u64 = 18 << 20;

/// A failed call: the HTTP status, when there was an answer, and what it
/// said.
#[derive(Clone, Debug, PartialEq)]
pub struct Fallo {
    /// The HTTP status; None when there was no answer.
    pub codigo: Option<u16>,
    /// The answer's body, or why there was none.
    pub texto: String,
}

impl Fallo {
    fn de(error: ureq::Error) -> Self {
        match error {
            ureq::Error::Status(codigo, respuesta) => Fallo {
                codigo: Some(codigo),
                texto: respuesta.into_string().unwrap_or_default(),
            },
            ureq::Error::Transport(error) => Fallo {
                codigo: None,
                texto: error.to_string(),
            },
        }
    }
}

/// Why Gemini could not describe.
#[derive(Clone, Debug, PartialEq)]
pub enum ErrorGemini {
    /// Every model is out of today's quota; it renews then.
    CuotaAgotada(DateTime<FixedOffset>),
    /// Every model is overloaded right now.
    Saturado,
    /// Anything else: a bad key, no network...
    Otro(String),
}

impl std::fmt::Display for ErrorGemini {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorGemini::CuotaAgotada(reanudar) => write!(
                f,
                "google: se acabó la cuota diaria del plan gratuito de Gemini; se renueva sobre las {}",
                reanudar.format("%H:%M")
            ),
            ErrorGemini::Saturado => write!(f, "google: Gemini está saturado ahora mismo; reintenta en unos minutos"),
            ErrorGemini::Otro(texto) => write!(f, "google: {texto}"),
        }
    }
}

/// What a failed call means for the next.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Clase {
    /// The model's quota for today is gone: on to the next.
    LimiteDiario,
    /// Too many a minute: wait and ask the same model again.
    LimitePorMinuto,
    /// Overloaded: once more, then the next model.
    Saturado,
    /// Not worth repeating.
    Otro,
}

fn clasificar(fallo: &Fallo) -> Clase {
    let texto = &fallo.texto;
    let limite = fallo.codigo == Some(429) || texto.contains("RESOURCE_EXHAUSTED");
    let saturado = matches!(fallo.codigo, Some(500 | 502 | 503 | 504))
        || ["UNAVAILABLE", "INTERNAL", "high demand"].iter().any(|t| texto.contains(t));
    let por_dia = ["per day", "perday", "PerDay", "free_tier_requests"].iter().any(|t| texto.contains(t));
    match (limite, saturado) {
        (true, _) if por_dia => Clase::LimiteDiario,
        (true, _) => Clase::LimitePorMinuto,
        (false, true) => Clase::Saturado,
        _ => Clase::Otro,
    }
}

/// Calls `llamada` with each of `modelos` in turn until one answers,
/// counting every attempt in `cuota`; `dormir` waits. A model out of
/// today's quota is marked and skipped; a per-minute limit is waited out
/// (90 s at most); an overloaded model gets one more try. When every model
/// is overloaded, the round is repeated after 30 s and then 90 s.
pub fn con_reintentos<T>(
    cuota: &Cuota,
    modelos: &[String],
    mut llamada: impl FnMut(&str) -> Result<T, Fallo>,
    mut dormir: impl FnMut(Duration),
) -> Result<T, ErrorGemini> {
    for espera in [0, 30, 90] {
        dormir(Duration::from_secs(espera));
        let vivos: Vec<&String> = modelos.iter().filter(|m| !cuota.agotado(m)).collect();
        if vivos.is_empty() {
            break;
        }
        for modelo in vivos {
            let mut intento = 0;
            while intento < 3 {
                intento += 1;
                cuota.registrar(modelo);
                let fallo = match llamada(modelo) {
                    Ok(respuesta) => return Ok(respuesta),
                    Err(fallo) => fallo,
                };
                log::warn!("gemini {modelo}, try {intento}: {:?} {}", fallo.codigo, recortar(&fallo.texto));
                match clasificar(&fallo) {
                    Clase::Otro => return Err(ErrorGemini::Otro(recortar(&fallo.texto))),
                    Clase::LimiteDiario => {
                        cuota.marcar_agotado(modelo, &fallo.texto);
                        break;
                    }
                    Clase::LimitePorMinuto => dormir(cuota::espera_por_minuto(&fallo.texto)),
                    Clase::Saturado if intento == 1 => dormir(Duration::from_secs(8)),
                    Clase::Saturado => break,
                }
            }
        }
    }
    if !modelos.iter().all(|m| cuota.agotado(m)) {
        return Err(ErrorGemini::Saturado);
    }
    let renovacion = modelos.iter().filter_map(|m| cuota.hasta(m)).min();
    Err(ErrorGemini::CuotaAgotada(
        renovacion.unwrap_or_else(|| cuota::proxima_renovacion(Utc::now())),
    ))
}

fn recortar(texto: &str) -> String {
    texto.chars().take(300).collect()
}

/// Google's API with a key.
pub struct Gemini {
    clave: String,
    agente: ureq::Agent,
}

impl Gemini {
    /// The API with `clave`.
    pub fn new(clave: String) -> Self {
        let agente = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout(Duration::from_secs(15 * 60))
            .build();
        Self { clave, agente }
    }

    /// The video at `ruta` as a request part, at `fps` frames a second:
    /// inline when small, else uploaded.
    pub fn parte_de_video(&self, ruta: &Path, fps: u32) -> Result<Value, Fallo> {
        let tamano = std::fs::metadata(ruta).map_err(disco)?.len();
        let video = if tamano <= MAX_EN_LINEA {
            let datos = std::fs::read(ruta).map_err(disco)?;
            json!({"inlineData": {"mimeType": "video/mp4",
                "data": base64::engine::general_purpose::STANDARD.encode(datos)}})
        } else {
            json!({"fileData": {"mimeType": "video/mp4", "fileUri": self.subir(ruta, tamano)?}})
        };
        let mut parte = video;
        parte["videoMetadata"] = json!({"fps": fps});
        Ok(parte)
    }

    /// Asks `modelo` about `partes` for JSON, the video seen at low
    /// resolution; returns the text it answered.
    pub fn generar(&self, modelo: &str, partes: &[Value]) -> Result<String, Fallo> {
        let turno = json!({"role": "user", "parts": partes});
        let ajustes = json!({"responseMimeType": "application/json", "mediaResolution": "MEDIA_RESOLUTION_LOW"});
        Ok(texto_de(&self.conversar(modelo, &[turno], ajustes)?))
    }

    /// Asks `modelo` to go on with the conversation `turnos` - each a
    /// `{"role", "parts"}` - under `generationConfig` `ajustes`; returns
    /// Google's whole answer ([`texto_de`] reads it).
    pub fn conversar(&self, modelo: &str, turnos: &[Value], ajustes: Value) -> Result<Value, Fallo> {
        let cuerpo = json!({"contents": turnos, "generationConfig": ajustes});
        self.agente
            .post(&format!("{API}/v1beta/models/{modelo}:generateContent"))
            .set("x-goog-api-key", &self.clave)
            .send_json(cuerpo)
            .map_err(Fallo::de)?
            .into_json()
            .map_err(disco)
    }

    /// Uploads the video at `ruta` through the files API and waits until
    /// Google has processed it; returns its URI.
    fn subir(&self, ruta: &Path, tamano: u64) -> Result<String, Fallo> {
        let inicio = self
            .agente
            .post(&format!("{API}/upload/v1beta/files"))
            .set("x-goog-api-key", &self.clave)
            .set("X-Goog-Upload-Protocol", "resumable")
            .set("X-Goog-Upload-Command", "start")
            .set("X-Goog-Upload-Header-Content-Length", &tamano.to_string())
            .set("X-Goog-Upload-Header-Content-Type", "video/mp4")
            .send_json(json!({"file": {"display_name": "contacto"}}))
            .map_err(Fallo::de)?;
        let destino = inicio.header("x-goog-upload-url").map(str::to_owned).ok_or_else(|| Fallo {
            codigo: None,
            texto: "Google no dio dónde subir el vídeo".into(),
        })?;
        let mut archivo: Value = self
            .agente
            .post(&destino)
            .set("X-Goog-Upload-Offset", "0")
            .set("X-Goog-Upload-Command", "upload, finalize")
            .send_bytes(&std::fs::read(ruta).map_err(disco)?)
            .map_err(Fallo::de)?
            .into_json()
            .map_err(disco)?;
        archivo = archivo["file"].take();
        while archivo["state"] == "PROCESSING" {
            std::thread::sleep(Duration::from_secs(3));
            let nombre = archivo["name"].as_str().unwrap_or_default().to_owned();
            archivo = self
                .agente
                .get(&format!("{API}/v1beta/{nombre}"))
                .set("x-goog-api-key", &self.clave)
                .call()
                .map_err(Fallo::de)?
                .into_json()
                .map_err(disco)?;
        }
        archivo["uri"].as_str().map(str::to_owned).ok_or_else(|| Fallo {
            codigo: None,
            texto: format!("Google no procesó el vídeo: {}", archivo["state"]),
        })
    }
}

/// The text of a `generateContent` answer: its first candidate's parts.
pub fn texto_de(respuesta: &Value) -> String {
    respuesta["candidates"][0]["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|parte| parte["text"].as_str())
        .collect()
}

fn disco(error: std::io::Error) -> Fallo {
    Fallo {
        codigo: None,
        texto: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn cuota(nombre: &str) -> Cuota {
        let carpeta = std::env::temp_dir().join(format!("analisis-gemini-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        Cuota::en(carpeta)
    }

    fn fallo(codigo: u16, texto: &str) -> Fallo {
        Fallo { codigo: Some(codigo), texto: texto.into() }
    }

    #[test]
    fn google_s_errors_are_told_apart() {
        assert_eq!(clasificar(&fallo(429, "free_tier_requests per day")), Clase::LimiteDiario);
        assert_eq!(clasificar(&fallo(429, "per minute")), Clase::LimitePorMinuto);
        assert_eq!(clasificar(&fallo(503, "UNAVAILABLE")), Clase::Saturado);
        assert_eq!(clasificar(&fallo(400, "API key not valid")), Clase::Otro);
    }

    #[test]
    fn a_model_out_of_quota_gives_way_to_the_next() {
        let cuota = cuota("siguiente");
        let modelos = ["flash".to_owned(), "pro".to_owned()];
        let pedidos = RefCell::new(Vec::new());

        let respuesta = con_reintentos(
            &cuota,
            &modelos,
            |modelo| {
                pedidos.borrow_mut().push(modelo.to_owned());
                match modelo {
                    "flash" => Err(fallo(429, "RESOURCE_EXHAUSTED per day limit: 20")),
                    _ => Ok("hecho"),
                }
            },
            |_| {},
        );

        assert_eq!(respuesta, Ok("hecho"));
        assert_eq!(*pedidos.borrow(), ["flash", "pro"]);
        assert!(cuota.agotado("flash"));
    }

    #[test]
    fn when_every_model_is_out_the_quota_s_renewal_is_told() {
        let cuota = cuota("agotada");
        let modelos = ["flash".to_owned()];
        let respuesta: Result<(), _> =
            con_reintentos(&cuota, &modelos, |_| Err(fallo(429, "per day")), |_| {});
        assert!(matches!(respuesta, Err(ErrorGemini::CuotaAgotada(_))));
    }

    #[test]
    fn an_overloaded_model_is_tried_twice_a_round_then_given_up() {
        let cuota = cuota("saturado");
        let modelos = ["flash".to_owned()];
        let intentos = RefCell::new(0);
        let esperas = RefCell::new(Vec::new());
        let respuesta: Result<(), _> = con_reintentos(
            &cuota,
            &modelos,
            |_| {
                *intentos.borrow_mut() += 1;
                Err(fallo(503, "high demand"))
            },
            |espera| esperas.borrow_mut().push(espera.as_secs()),
        );
        assert_eq!(respuesta, Err(ErrorGemini::Saturado));
        assert_eq!(*intentos.borrow(), 6);
        assert_eq!(*esperas.borrow(), [0, 8, 30, 8, 90, 8]);
    }

    #[test]
    fn an_answer_s_text_is_its_parts_joined() {
        let respuesta = json!({"candidates": [{"content": {"parts": [{"text": "{\"clips\""}, {"text": ": []}"}]}}]});
        assert_eq!(texto_de(&respuesta), "{\"clips\": []}");
        assert_eq!(texto_de(&json!({})), "");
    }
}
