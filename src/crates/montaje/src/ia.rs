//! Which AI does what, as the team chose in Settings - who plans, who
//! describes, which whisper hears each language - and how those choices
//! go over the montage's settings for a run ([`crate::Datos::ajustes`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::proveedores::Proveedor;

/// The AIs that can plan, by the settings' name for each.
pub const CEREBROS: [&str; 7] = [
    "claude_code",
    "anthropic",
    "google",
    "openai",
    "codex",
    "gemini_cli",
    "manual",
];

/// Who describes the clips to the planner.
pub const DESCRIPCIONES: [&str; 4] = ["auto", "google", "anthropic", "ninguno"];

/// How hard Claude thinks over the plan.
pub const ESFUERZOS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The whisper models the montage offers, by their ggml id: the ones that
/// hear Spanish and Basque well enough for a montage.
const WHISPER: [&str; 6] = [
    "tiny",
    "base",
    "small",
    "large-v3-turbo-q5_0",
    "large-v3-q5_0",
    "large-v3-eu",
];

/// The whisper that hears Spanish when none is chosen.
pub const WHISPER_ES: &str = "large-v3-q5_0";
/// The whisper that hears Basque when none is chosen: the Basque
/// fine-tune.
pub const WHISPER_EU: &str = "large-v3-eu";

/// Whether the montage offers the whisper Komunta keeps as `id`.
pub fn whisper_ofrecido(id: &str) -> bool {
    WHISPER.contains(&id)
}

/// The team's AI choices. Each None is the settings' own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ia {
    /// The planner, by its name in [`CEREBROS`].
    pub cerebro: Option<String>,
    /// Who takes over, in order, when the planner fails.
    pub respaldo: Option<Vec<String>>,
    /// The model per API provider, by the provider's id.
    pub modelos: BTreeMap<String, String>,
    /// Claude's effort, for the Anthropic API.
    pub esfuerzo: Option<String>,
    /// Who describes the clips ([`DESCRIPCIONES`]).
    pub descripcion: Option<String>,
    /// The whisper that hears Spanish, by its ggml id.
    pub transcripcion_es: Option<String>,
    /// The whisper that hears Basque, by its ggml id.
    pub transcripcion_eu: Option<String>,
}

impl Ia {
    /// The whisper models that hear Spanish and Basque, by their ggml ids:
    /// the chosen ones, or [`WHISPER_ES`] and [`WHISPER_EU`].
    pub fn whisper(&self) -> (&str, &str) {
        (
            self.transcripcion_es.as_deref().unwrap_or(WHISPER_ES),
            self.transcripcion_eu.as_deref().unwrap_or(WHISPER_EU),
        )
    }

    /// The model chosen for `proveedor`, or its default.
    pub fn modelo(&self, proveedor: Proveedor) -> &str {
        self.modelos
            .get(proveedor.id())
            .map(String::as_str)
            .unwrap_or_else(|| proveedor.modelo_por_defecto())
    }

    /// These choices as dotted settings: `{"ia.respaldo": [...],
    /// "ia.google.modelo": ...}`. The planner is not among them: it is
    /// `ia.modo`, set apart.
    pub fn cambios(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut ajustes = serde_json::Map::new();
        let mut poner = |clave: String, valor: serde_json::Value| {
            ajustes.insert(clave, valor);
        };
        if let Some(respaldo) = &self.respaldo {
            poner("ia.respaldo".into(), respaldo.clone().into());
        }
        for (proveedor, modelo) in &self.modelos {
            poner(format!("ia.{proveedor}.modelo"), modelo.clone().into());
        }
        if let Some(esfuerzo) = &self.esfuerzo {
            poner("ia.anthropic.esfuerzo".into(), esfuerzo.clone().into());
        }
        match self.descripcion.as_deref() {
            Some("ninguno") => poner("descripcion.modo".into(), "ninguno".into()),
            Some(proveedor) => {
                poner("descripcion.modo".into(), "auto".into());
                poner("descripcion.proveedor".into(), proveedor.into());
            }
            None => {}
        }
        ajustes
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn nothing_chosen_changes_nothing() {
        assert!(Ia::default().cambios().is_empty());
        assert_eq!(Ia::default().modelo(Proveedor::Anthropic), "claude-opus-5-5");
    }

    #[test]
    fn choices_become_the_settings_names() {
        let ia = Ia {
            cerebro: Some("anthropic".into()),
            respaldo: Some(vec!["google".into()]),
            modelos: BTreeMap::from([("anthropic".into(), "claude-sonnet-5-5".into())]),
            esfuerzo: Some("medium".into()),
            descripcion: Some("google".into()),
            transcripcion_es: Some("large-v3-turbo-q5_0".into()),
            transcripcion_eu: None,
        };
        assert_eq!(
            Value::Object(ia.cambios()),
            json!({
                "ia.respaldo": ["google"],
                "ia.anthropic.modelo": "claude-sonnet-5-5",
                "ia.anthropic.esfuerzo": "medium",
                "descripcion.modo": "auto",
                "descripcion.proveedor": "google"
            })
        );
        assert_eq!(ia.modelo(Proveedor::Anthropic), "claude-sonnet-5-5");
        assert_eq!(ia.whisper(), ("large-v3-turbo-q5_0", WHISPER_EU));
    }

    #[test]
    fn no_description_turns_it_off() {
        let ia = Ia {
            descripcion: Some("ninguno".into()),
            ..Ia::default()
        };
        assert_eq!(Value::Object(ia.cambios()), json!({"descripcion.modo": "ninguno"}));
    }

    #[test]
    fn an_empty_fallback_is_said_outright() {
        let ia = Ia {
            respaldo: Some(Vec::new()),
            ..Ia::default()
        };
        assert_eq!(Value::Object(ia.cambios()), json!({"ia.respaldo": []}));
    }

    #[test]
    fn english_only_whispers_are_not_offered() {
        assert!(whisper_ofrecido(WHISPER_ES));
        assert!(whisper_ofrecido(WHISPER_EU));
        assert!(!whisper_ofrecido("base.en"));
    }

    #[test]
    fn the_saved_planner_keeps_its_place_in_settings() {
        let ia: Ia = serde_json::from_str(r#"{"cerebro": "codex"}"#).unwrap();
        assert_eq!(ia.cerebro.as_deref(), Some("codex"));
        assert_eq!(ia.respaldo, None);
    }
}
