//! Which AI does what, as the team chose in Settings - who plans, who
//! describes, which whisper hears each language - and how a run of the
//! engine is told: `--ia` for the planner, and the rest as the engine's
//! own settings in `MOTOR_AJUSTES`, for that run only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::llavero::{self, Llavero};
use crate::proveedores::Proveedor;

/// The engine's variable for settings that hold for one run.
pub const VARIABLE_AJUSTES: &str = "MOTOR_AJUSTES";

/// The AIs the engine can plan with, as its `--ia` names them.
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

/// The whisper models Komunta keeps (by their ggml id) and the name the
/// engine's faster-whisper knows each by.
const WHISPER: [(&str, &str); 6] = [
    ("tiny", "tiny"),
    ("base", "base"),
    ("small", "small"),
    ("large-v3-turbo-q5_0", "large-v3-turbo"),
    ("large-v3-q5_0", "large-v3"),
    ("large-v3-eu", "xezpeleta/whisper-large-v3-eu-ct2"),
];

/// The whisper the engine hears Spanish with when none is chosen.
pub const WHISPER_ES: &str = "large-v3-q5_0";
/// The whisper the engine hears Basque with when none is chosen: the
/// Basque fine-tune.
pub const WHISPER_EU: &str = "large-v3-eu";

/// The engine's name for the whisper Komunta keeps as `id`.
pub fn whisper_del_motor(id: &str) -> Option<&'static str> {
    WHISPER
        .iter()
        .find(|(nuestro, _)| *nuestro == id)
        .map(|(_, motor)| *motor)
}

/// The team's AI choices. Each None is the engine's own setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ia {
    /// The planner, by its `--ia` name.
    pub cerebro: Option<String>,
    /// Who takes over, in order, when the planner fails.
    pub respaldo: Option<Vec<String>>,
    /// The model per API provider, by the provider's engine name.
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
    /// The model chosen for `proveedor`, or its default.
    pub fn modelo(&self, proveedor: Proveedor) -> &str {
        self.modelos
            .get(proveedor.id())
            .map(String::as_str)
            .unwrap_or_else(|| proveedor.modelo_por_defecto())
    }

    /// The engine's settings for a run, as `MOTOR_AJUSTES` holds them;
    /// None when nothing differs from the engine's own.
    pub fn para_motor(&self) -> Option<String> {
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
        let whisper = |id: &Option<String>| id.as_deref().and_then(whisper_del_motor);
        if let Some(modelo) = whisper(&self.transcripcion_es) {
            poner("transcripcion.modelo_es".into(), modelo.into());
        }
        if let Some(modelo) = whisper(&self.transcripcion_eu) {
            poner("transcripcion.modelo_eu".into(), modelo.into());
        }
        (!ajustes.is_empty()).then(|| serde_json::Value::Object(ajustes).to_string())
    }

    /// What a run of the engine adds to its environment: the kept keys,
    /// and these choices for that run.
    pub fn entorno(&self, llavero: &impl Llavero) -> Vec<(String, String)> {
        let claves = llavero::entorno(llavero)
            .into_iter()
            .map(|(variable, clave)| (variable.to_owned(), clave));
        let ajustes = self
            .para_motor()
            .map(|ajustes| (VARIABLE_AJUSTES.to_owned(), ajustes));
        claves.chain(ajustes).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_chosen_tells_the_engine_nothing() {
        assert_eq!(Ia::default().para_motor(), None);
        assert_eq!(
            Ia::default().modelo(Proveedor::Anthropic),
            "claude-opus-5-5"
        );
    }

    #[test]
    fn choices_reach_the_engine_under_its_own_names() {
        let ia = Ia {
            cerebro: Some("anthropic".into()),
            respaldo: Some(vec!["google".into()]),
            modelos: BTreeMap::from([("anthropic".into(), "claude-sonnet-5-5".into())]),
            esfuerzo: Some("medium".into()),
            descripcion: Some("google".into()),
            transcripcion_es: Some("large-v3-turbo-q5_0".into()),
            transcripcion_eu: None,
        };
        let ajustes: serde_json::Value = serde_json::from_str(&ia.para_motor().unwrap()).unwrap();
        assert_eq!(
            ajustes,
            serde_json::json!({
                "ia.respaldo": ["google"],
                "ia.anthropic.modelo": "claude-sonnet-5-5",
                "ia.anthropic.esfuerzo": "medium",
                "descripcion.modo": "auto",
                "descripcion.proveedor": "google",
                "transcripcion.modelo_es": "large-v3-turbo"
            })
        );
        assert_eq!(ia.modelo(Proveedor::Anthropic), "claude-sonnet-5-5");
    }

    #[test]
    fn no_description_turns_it_off() {
        let ia = Ia {
            descripcion: Some("ninguno".into()),
            ..Ia::default()
        };
        assert_eq!(
            ia.para_motor().unwrap(),
            r#"{"descripcion.modo":"ninguno"}"#
        );
    }

    #[test]
    fn whispers_reach_the_engine_by_its_names() {
        assert_eq!(
            whisper_del_motor(WHISPER_EU),
            Some("xezpeleta/whisper-large-v3-eu-ct2")
        );
        assert_eq!(whisper_del_motor(WHISPER_ES), Some("large-v3"));
        assert_eq!(
            whisper_del_motor("base.en"),
            None,
            "English-only models hear no Basque"
        );
        let ia = Ia {
            transcripcion_eu: Some("nadie".into()),
            ..Ia::default()
        };
        assert_eq!(
            ia.para_motor(),
            None,
            "a model the engine does not know is not passed on"
        );
    }

    #[test]
    fn an_empty_fallback_is_said_outright() {
        let ia = Ia {
            respaldo: Some(Vec::new()),
            ..Ia::default()
        };
        assert_eq!(ia.para_motor().unwrap(), r#"{"ia.respaldo":[]}"#);
    }

    #[test]
    fn a_run_gets_the_keys_and_the_choices() {
        let llavero = crate::llavero::prueba::LlaveroEnMemoria::default();
        llavero
            .guardar(Proveedor::Anthropic, "sk-ant-api03-example")
            .unwrap();
        let ia = Ia {
            esfuerzo: Some("high".into()),
            ..Ia::default()
        };
        let entorno = ia.entorno(&llavero);
        assert_eq!(
            entorno[0],
            (
                "ANTHROPIC_API_KEY".to_owned(),
                "sk-ant-api03-example".to_owned()
            )
        );
        assert_eq!(entorno[1].0, "MOTOR_AJUSTES");
        assert!(
            Ia::default()
                .entorno(&crate::llavero::prueba::LlaveroEnMemoria::default())
                .is_empty()
        );
    }

    #[test]
    fn the_saved_planner_keeps_its_place_in_settings() {
        let ia: Ia = serde_json::from_str(r#"{"cerebro": "codex"}"#).unwrap();
        assert_eq!(ia.cerebro.as_deref(), Some("codex"));
        assert_eq!(ia.respaldo, None);
    }
}
