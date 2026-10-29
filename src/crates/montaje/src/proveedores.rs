//! The AI providers the engine can plan and describe with through their
//! APIs: what each is called, which key it reads, the models worth
//! offering, and a cheap call that proves a key works.

use std::time::Duration;

/// An AI company the engine calls with an API key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proveedor {
    /// Claude.
    Anthropic,
    /// GPT.
    OpenAi,
    /// Gemini.
    Google,
}

/// A model worth offering before the provider has been asked for its list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recomendado {
    /// The model's id, as the API takes it.
    pub id: &'static str,
    /// What it is good for.
    pub nota: &'static str,
}

impl Proveedor {
    /// Every provider, in the order the settings list them.
    pub const TODOS: [Proveedor; 3] = [Proveedor::Anthropic, Proveedor::OpenAi, Proveedor::Google];

    /// The engine's name for it: `--ia`, `ia.respaldo`, `ia.<id>.modelo`.
    pub fn id(self) -> &'static str {
        match self {
            Proveedor::Anthropic => "anthropic",
            Proveedor::OpenAi => "openai",
            Proveedor::Google => "google",
        }
    }

    /// The provider by its engine name.
    pub fn desde_id(id: &str) -> Option<Self> {
        Self::TODOS
            .into_iter()
            .find(|proveedor| proveedor.id() == id)
    }

    /// As people call it.
    pub fn nombre(self) -> &'static str {
        match self {
            Proveedor::Anthropic => "Anthropic (Claude)",
            Proveedor::OpenAi => "OpenAI (GPT)",
            Proveedor::Google => "Google (Gemini)",
        }
    }

    /// The environment variable the engine reads its key from.
    pub fn variable(self) -> &'static str {
        match self {
            Proveedor::Anthropic => "ANTHROPIC_API_KEY",
            Proveedor::OpenAi => "OPENAI_API_KEY",
            Proveedor::Google => "GEMINI_API_KEY",
        }
    }

    /// The models to offer first, best first; the engine's own list.
    pub fn recomendados(self) -> &'static [Recomendado] {
        match self {
            Proveedor::Anthropic => &[
                Recomendado {
                    id: "claude-opus-5-5",
                    nota: "El que mejor decide el montaje (≈ 1 $ por vídeo)",
                },
                Recomendado {
                    id: "claude-sonnet-5-5",
                    nota: "Equilibrado, más barato (≈ 0,5 $ por vídeo)",
                },
                Recomendado {
                    id: "claude-haiku-5-5",
                    nota: "Muy barato; decide peor en vídeos largos",
                },
            ],
            Proveedor::OpenAi => &[Recomendado {
                id: "gpt-5.5",
                nota: "Modelo general de OpenAI",
            }],
            Proveedor::Google => &[
                Recomendado {
                    id: "gemini-flash-latest",
                    nota: "Con la clave gratuita; describe los clips",
                },
                Recomendado {
                    id: "gemini-pro-latest",
                    nota: "Más capaz; límites muy bajos en el plan gratuito",
                },
            ],
        }
    }

    /// The model used when none is chosen.
    pub fn modelo_por_defecto(self) -> &'static str {
        self.recomendados()[0].id
    }

    /// Asks the provider for its models with `clave`: the cheapest call
    /// there is, and the proof that the key works. Blocks; run it on its
    /// own thread.
    pub fn probar(self, clave: &str) -> Result<Vec<String>, String> {
        let agente = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build();
        let peticion = match self {
            Proveedor::Anthropic => agente
                .get("https://api.anthropic.com/v1/models?limit=1000")
                .set("x-api-key", clave)
                .set("anthropic-version", "2023-06-01"),
            Proveedor::OpenAi => agente
                .get("https://api.openai.com/v1/models")
                .set("Authorization", &format!("Bearer {clave}")),
            Proveedor::Google => agente
                .get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000")
                .set("x-goog-api-key", clave),
        };
        let respuesta = peticion.call().map_err(|error| explicar(self, error))?;
        let ilegible = |error: &dyn std::fmt::Display| {
            format!(
                "{} respondió algo que no se entiende: {error}",
                self.nombre()
            )
        };
        let texto = respuesta.into_string().map_err(|error| ilegible(&error))?;
        let cuerpo: serde_json::Value =
            serde_json::from_str(&texto).map_err(|error| ilegible(&error))?;
        Ok(modelos(self, &cuerpo))
    }
}

/// The models in a provider's list worth planning a montage with: those
/// that reason over text (and pictures), not the ones that draw, speak or
/// transcribe. Sorted, recommended ones first.
pub fn modelos(proveedor: Proveedor, cuerpo: &serde_json::Value) -> Vec<String> {
    let (lista, campo) = match proveedor {
        Proveedor::Google => ("models", "name"),
        _ => ("data", "id"),
    };
    let mut ids: Vec<String> = cuerpo[lista]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|modelo| proveedor != Proveedor::Google || genera_texto(modelo))
        .filter_map(|modelo| modelo[campo].as_str())
        .map(|id| id.trim_start_matches("models/").to_owned())
        .filter(|id| sirve(proveedor, id))
        .collect();
    ids.sort();
    ids.dedup();
    let recomendados: Vec<&str> = proveedor.recomendados().iter().map(|r| r.id).collect();
    ids.sort_by_key(|id| {
        recomendados
            .iter()
            .position(|r| r == id)
            .unwrap_or(usize::MAX)
    });
    ids
}

fn genera_texto(modelo: &serde_json::Value) -> bool {
    modelo["supportedGenerationMethods"]
        .as_array()
        .is_some_and(|metodos| metodos.iter().any(|m| m == "generateContent"))
}

/// Whether a model by this id can plan: no image, voice, transcription or
/// embedding models.
fn sirve(proveedor: Proveedor, id: &str) -> bool {
    const FUERA: [&str; 13] = [
        "image",
        "audio",
        "realtime",
        "tts",
        "transcribe",
        "search",
        "embedding",
        "computer-use",
        "robotics",
        "nano-banana",
        "moderation",
        "whisper",
        "dall-e",
    ];
    let prefijo = match proveedor {
        Proveedor::Anthropic => id.starts_with("claude"),
        Proveedor::OpenAi => id.starts_with("gpt") || id.starts_with("chatgpt") || es_serie_o(id),
        Proveedor::Google => id.starts_with("gemini"),
    };
    prefijo && !FUERA.iter().any(|palabra| id.contains(palabra))
}

/// OpenAI's reasoning models: o1, o3, o4-mini...
fn es_serie_o(id: &str) -> bool {
    let mut letras = id.chars();
    letras.next() == Some('o') && letras.next().is_some_and(|c| c.is_ascii_digit())
}

/// A failed call, said the way someone fixing their key needs to hear it.
fn explicar(proveedor: Proveedor, error: ureq::Error) -> String {
    let nombre = proveedor.nombre();
    match error {
        ureq::Error::Status(401 | 403, _) => {
            format!("{nombre} no acepta la clave: revisa que esté completa y activa.")
        }
        ureq::Error::Status(400, _) if proveedor == Proveedor::Google => {
            format!("{nombre} no acepta la clave: revisa que esté completa y activa.")
        }
        ureq::Error::Status(429, _) => {
            format!("{nombre} dice que se ha superado el límite: prueba en un rato.")
        }
        ureq::Error::Status(codigo, _) => format!("{nombre} respondió con un error ({codigo})."),
        ureq::Error::Transport(_) => {
            format!("No se pudo contactar con {nombre}: revisa la conexión.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn providers_go_by_the_engines_names() {
        for proveedor in Proveedor::TODOS {
            assert_eq!(Proveedor::desde_id(proveedor.id()), Some(proveedor));
        }
        assert_eq!(
            Proveedor::desde_id("claude_code"),
            None,
            "a CLI, not an API"
        );
        assert_eq!(Proveedor::Google.variable(), "GEMINI_API_KEY");
    }

    #[test]
    fn anthropic_lists_its_claudes_recommended_first() {
        let cuerpo = json!({"data": [{"id": "claude-haiku-5-5"}, {"id": "claude-opus-5-5"}, {"id": "claude-3-haiku"}]});
        assert_eq!(
            modelos(Proveedor::Anthropic, &cuerpo),
            ["claude-opus-5-5", "claude-haiku-5-5", "claude-3-haiku"]
        );
    }

    #[test]
    fn openai_keeps_only_the_models_that_reason() {
        let cuerpo = json!({"data": [
            {"id": "gpt-5.5"}, {"id": "gpt-4o-audio-preview"}, {"id": "o4-mini"}, {"id": "dall-e-3"},
            {"id": "whisper-1"}, {"id": "omni-moderation-latest"}, {"id": "gpt-image-1"}, {"id": "text-embedding-3"}
        ]});
        assert_eq!(modelos(Proveedor::OpenAi, &cuerpo), ["gpt-5.5", "o4-mini"]);
    }

    #[test]
    fn google_keeps_the_gemini_that_generate_text() {
        let cuerpo = json!({"models": [
            {"name": "models/gemini-flash-latest", "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/gemini-2.5-flash-image", "supportedGenerationMethods": ["generateContent"]},
            {"name": "models/text-embedding-004", "supportedGenerationMethods": ["embedContent"]},
            {"name": "models/gemini-embedding-001", "supportedGenerationMethods": ["embedContent"]},
            {"name": "models/gemini-2.5-pro", "supportedGenerationMethods": ["generateContent", "countTokens"]}
        ]});
        assert_eq!(
            modelos(Proveedor::Google, &cuerpo),
            ["gemini-flash-latest", "gemini-2.5-pro"]
        );
    }

    #[test]
    fn an_odd_answer_lists_nothing() {
        assert!(modelos(Proveedor::OpenAi, &json!({"error": "nope"})).is_empty());
    }
}
