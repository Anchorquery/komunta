//! Who describes the material to the planner, as the team chose in
//! Settings and the engine's `config.yaml` says otherwise: none, or the
//! providers with a key, in the order to try them, each with its models.
//! The engine's `proveedores` and `modelos_google`; the describing itself
//! is `analisis::descripcion`.

use std::path::Path;

use serde_json::Value;

use crate::ia::Ia;
use crate::llavero::{self, Llavero};
use crate::proveedores::Proveedor;

/// A provider that can describe, with what it needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor {
    /// Google or Anthropic.
    pub proveedor: Proveedor,
    /// Its API key.
    pub clave: String,
    /// The model and, after it, the ones to fall back to.
    pub modelos: Vec<String>,
    /// Requests a day per model on the free plan; None when paid.
    pub limite_diario: Option<u32>,
}

/// The providers that describe for `ia` under the engine's `config`, in
/// order, those with a key in `llavero`, the engine's `.env` at
/// `env_motor` or the environment; none when the description is off.
pub fn descriptores(ia: &Ia, config: &Value, llavero: &impl Llavero, env_motor: &Path) -> Vec<Descriptor> {
    elegir(ia, config, |proveedor| llavero::clave(proveedor, llavero, env_motor))
}

/// [`descriptores`], with `clave` giving each provider's key.
fn elegir(ia: &Ia, config: &Value, clave: impl Fn(Proveedor) -> Option<String>) -> Vec<Descriptor> {
    let Some(preferido) = preferido(ia, config) else {
        return Vec::new();
    };
    let cerebro = ia.cerebro.as_deref().or_else(|| config["ia"]["modo"].as_str());
    orden(&preferido, cerebro)
        .into_iter()
        .filter_map(|proveedor| {
            Some(Descriptor {
                proveedor,
                clave: clave(proveedor)?,
                modelos: modelos(proveedor, ia, config),
                limite_diario: limite_diario(proveedor, config),
            })
        })
        .collect()
}

/// Who the team prefers ("auto", "google", "anthropic"); None when the
/// description is off.
fn preferido(ia: &Ia, config: &Value) -> Option<String> {
    let opciones = &config["descripcion"];
    let elegido = ia.descripcion.as_deref().map(str::to_owned).or_else(|| {
        if opciones["modo"].as_str() == Some("ninguno") {
            return Some("ninguno".into());
        }
        opciones["proveedor"].as_str().map(str::to_owned)
    });
    match elegido.as_deref() {
        Some("ninguno") => None,
        Some(otro) => Some(otro.to_owned()),
        None => Some("auto".into()),
    }
}

/// `preferido` first; "auto" prefers the planner's own provider when it
/// can look at material - Claude, when `cerebro` is Anthropic's API or
/// Claude Code - and Gemini otherwise.
fn orden(preferido: &str, cerebro: Option<&str>) -> [Proveedor; 2] {
    let claude_primero = match preferido {
        "auto" => matches!(cerebro, Some("anthropic" | "claude_code")),
        otro => otro == "anthropic",
    };
    if claude_primero {
        [Proveedor::Anthropic, Proveedor::Google]
    } else {
        [Proveedor::Google, Proveedor::Anthropic]
    }
}

/// The models to describe with: the team's choice, else the engine's -
/// for Gemini, `descripcion.modelo` or `ia.google.modelo`, then
/// `ia.google.respaldo`, each with a quota of its own.
fn modelos(proveedor: Proveedor, ia: &Ia, config: &Value) -> Vec<String> {
    let del_equipo = ia.modelos.get(proveedor.id()).cloned();
    let del_motor = config["ia"][proveedor.id()]["modelo"].as_str().map(str::to_owned);
    if proveedor != Proveedor::Google {
        let modelo = del_equipo.or(del_motor).unwrap_or_else(|| proveedor.modelo_por_defecto().into());
        return vec![modelo];
    }
    let principal = del_equipo
        .or_else(|| config["descripcion"]["modelo"].as_str().map(str::to_owned))
        .or(del_motor)
        .unwrap_or_else(|| proveedor.modelo_por_defecto().into());
    let respaldo = config["ia"]["google"]["respaldo"].as_array().into_iter().flatten();
    let mut modelos = vec![principal];
    for modelo in respaldo.filter_map(Value::as_str) {
        if !modelos.iter().any(|m| m == modelo) {
            modelos.push(modelo.to_owned());
        }
    }
    modelos
}

/// Gemini's free requests a day, `ia.google.limite_diario`; 0 is a paid
/// key, with no limit.
fn limite_diario(proveedor: Proveedor, config: &Value) -> Option<u32> {
    let limite = config["ia"][proveedor.id()]["limite_diario"].as_u64()?;
    u32::try_from(limite).ok().filter(|l| *l > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Value {
        const CONFIG: &str = "
ia:
  modo: claude_code
  google:
    modelo: gemini-flash-latest
    respaldo: [gemini-2.5-flash, gemini-flash-latest]
    limite_diario: 20
descripcion:
  modo: auto
";
        serde_yaml::from_str(CONFIG).unwrap()
    }

    fn ambas(_: Proveedor) -> Option<String> {
        Some("clave".into())
    }

    #[test]
    fn claude_code_planning_puts_claude_first() {
        let descriptores = elegir(&Ia::default(), &config(), ambas);
        let orden: Vec<Proveedor> = descriptores.iter().map(|d| d.proveedor).collect();
        assert_eq!(orden, [Proveedor::Anthropic, Proveedor::Google]);
        assert_eq!(descriptores[0].modelos, ["claude-opus-5-5"]);
        assert_eq!(descriptores[1].modelos, ["gemini-flash-latest", "gemini-2.5-flash"]);
        assert_eq!(descriptores[1].limite_diario, Some(20));
    }

    #[test]
    fn the_team_s_choices_win() {
        let mut ia = Ia {
            descripcion: Some("google".into()),
            ..Ia::default()
        };
        ia.modelos.insert("google".into(), "gemini-pro-latest".into());
        let descriptores = elegir(&ia, &config(), ambas);
        assert_eq!(descriptores[0].proveedor, Proveedor::Google);
        assert_eq!(descriptores[0].modelos[0], "gemini-pro-latest");

        let apagado = Ia {
            descripcion: Some("ninguno".into()),
            ..Ia::default()
        };
        assert!(elegir(&apagado, &config(), ambas).is_empty());
    }

    #[test]
    fn a_provider_without_a_key_is_left_out_and_a_paid_key_has_no_limit() {
        let mut config = config();
        config["ia"]["modo"] = Value::from("openai");
        config["ia"]["google"]["limite_diario"] = Value::from(0);
        let solo_google = |p: Proveedor| (p == Proveedor::Google).then(|| "clave".to_owned());
        let descriptores = elegir(&Ia::default(), &config, solo_google);
        assert_eq!(descriptores.len(), 1);
        assert_eq!(descriptores[0].proveedor, Proveedor::Google);
        assert_eq!(descriptores[0].limite_diario, None);
    }
}
