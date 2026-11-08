//! The planner chosen and, when it fails - quota spent, overloaded, no
//! network, a plan it cannot get right - the ones in `ia.respaldo`, in
//! order. An API planner only joins the chain when it has a key. The
//! engine's `Cadena` and `crear`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use super::anthropic::Anthropic;
use super::cerebro::{Conector, Fallo, Material, planificar};
use super::clis::{ClaudeCode, Codex, GeminiCli};
use super::google::Google;
use super::openai::OpenAi;

/// Every `ia.modo`, as the error lists them.
const MODOS: [&str; 7] = ["manual", "claude_code", "codex", "gemini_cli", "anthropic", "openai", "google"];
/// The planners that need a key, by their `ia.modo`.
const CON_CLAVE: [&str; 3] = ["anthropic", "openai", "google"];

/// What a planner may need besides its own settings.
#[derive(Clone, Debug, Default)]
pub struct Entorno {
    /// The engine's settings, the team's choices on top.
    pub config: Value,
    /// The API keys, by provider: "anthropic", "openai", "google".
    pub claves: HashMap<String, String>,
    /// Where Gemini's daily quota is counted.
    pub cuota: PathBuf,
    /// Set when the montage is cancelled: a command-line AI is stopped.
    pub parada: Arc<AtomicBool>,
}

impl Entorno {
    /// `ia.<modo>` in the settings: the planner's own options.
    pub fn opciones(&self, modo: &str) -> &Value {
        &self.config["ia"][modo]
    }

    /// How long a planner may take: `ia.timeout_seg`, 20 minutes unless
    /// it says.
    pub fn limite(&self) -> Duration {
        Duration::from_secs(self.config["ia"]["timeout_seg"].as_u64().unwrap_or(1200))
    }

    fn clave(&self, modo: &str) -> Option<String> {
        self.claves.get(modo).filter(|c| !c.trim().is_empty()).cloned()
    }
}

/// The planners to try, in order, and the one that made the plan.
pub struct Cadena {
    conectores: Vec<Box<dyn Conector>>,
    activo: Option<usize>,
    /// Why each one that was tried failed, `nombre: motivo`.
    pub fallos: Vec<String>,
}

impl Cadena {
    /// The planners `ia.modo` and `ia.respaldo` name; None for "manual",
    /// which waits for someone to write `ia/plan.json`.
    pub fn crear(entorno: &Entorno) -> Result<Option<Cadena>, Fallo> {
        let modo = match entorno.config["ia"]["modo"].as_str().unwrap_or("manual") {
            "api" => "anthropic",
            otro => otro,
        };
        if !MODOS.contains(&modo) {
            return Err(Fallo::Otro(format!("ia.modo «{modo}» desconocido. Opciones: {}", MODOS.join(", "))));
        }
        if modo == "manual" {
            return Ok(None);
        }
        let mut modos = vec![modo.to_owned()];
        for respaldo in entorno.config["ia"]["respaldo"].as_array().into_iter().flatten() {
            let Some(respaldo) = respaldo.as_str() else { continue };
            let vale = MODOS.contains(&respaldo) && respaldo != "manual";
            let con_clave = !CON_CLAVE.contains(&respaldo) || entorno.clave(respaldo).is_some();
            if vale && con_clave && !modos.iter().any(|m| m == respaldo) {
                modos.push(respaldo.to_owned());
            }
        }
        let conectores = modos.iter().map(|m| conector(m, entorno)).collect();
        Ok(Some(Cadena::new(conectores)))
    }

    /// A chain of these planners, in order.
    pub fn new(conectores: Vec<Box<dyn Conector>>) -> Self {
        Self { conectores, activo: None, fallos: Vec::new() }
    }

    /// The planner that made the plan, or the first one.
    pub fn nombre(&self) -> &'static str {
        self.conectores[self.activo.unwrap_or(0)].nombre()
    }

    /// Asks each planner in turn until one gives a valid plan, which is
    /// written as the job's `ia/plan.json`.
    pub fn plan(&mut self, trabajo: &Path, pedido: &str, hojas: &[PathBuf], material: Material) -> Result<Value, Fallo> {
        let mut errores = Vec::new();
        for k in self.orden() {
            let conector = &mut self.conectores[k];
            match planificar(conector.as_mut(), trabajo, pedido, hojas, material) {
                Ok(plan) => {
                    self.activo = Some(k);
                    return Ok(plan);
                }
                Err(fallo) => {
                    let motivo: String = fallo.to_string().chars().take(200).collect();
                    log::warn!("montaje: planner {} failed: {motivo}", conector.nombre());
                    self.fallos.push(format!("{}: {motivo}", conector.nombre()));
                    errores.push(fallo);
                }
            }
        }
        Err(self.resumir(errores))
    }

    /// The one that made the plan first, then the rest in order.
    fn orden(&self) -> Vec<usize> {
        let resto = (0..self.conectores.len()).filter(|k| Some(*k) != self.activo);
        self.activo.into_iter().chain(resto).collect()
    }

    /// Why no planner could: the quota when one ran out (it is worth
    /// waiting for), the failure when there was only one.
    fn resumir(&self, mut errores: Vec<Fallo>) -> Fallo {
        let cuota = errores.iter().find(|e| matches!(e, Fallo::Cuota(_))).cloned();
        if let Some(cuota) = cuota.filter(|_| errores.len() == self.conectores.len()) {
            return cuota;
        }
        if errores.len() == 1 {
            return errores.remove(0);
        }
        let ultimos = &self.fallos[self.fallos.len() - errores.len()..];
        Fallo::Otro(format!("Ningún cerebro pudo hacerlo:\n- {}", ultimos.join("\n- ")))
    }
}

fn conector(modo: &str, entorno: &Entorno) -> Box<dyn Conector> {
    match modo {
        "claude_code" => Box::new(ClaudeCode::new(entorno)),
        "codex" => Box::new(Codex::new(entorno)),
        "gemini_cli" => Box::new(GeminiCli::new(entorno)),
        "anthropic" => Box::new(Anthropic::new(entorno, entorno.clave("anthropic"))),
        "openai" => Box::new(OpenAi::new(entorno, entorno.clave("openai"))),
        _ => Box::new(Google::new(entorno, entorno.clave("google"))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::cerebro::pruebas::*;
    use super::*;
    use chrono::DateTime;
    use serde_json::json;

    fn entorno(ia: Value) -> Entorno {
        Entorno { config: json!({"ia": ia}), ..Entorno::default() }
    }

    fn nombres(cadena: &Cadena) -> Vec<&'static str> {
        cadena.conectores.iter().map(|c| c.nombre()).collect()
    }

    #[test]
    fn the_backups_join_only_with_a_key_and_once() {
        let mut con_claves = entorno(json!({"modo": "claude_code",
            "respaldo": ["anthropic", "openai", "claude_code", "manual", "nada", "codex", "anthropic"]}));
        con_claves.claves.insert("anthropic".into(), "clave".into());
        let cadena = Cadena::crear(&con_claves).unwrap().unwrap();
        assert_eq!(nombres(&cadena), ["claude_code", "anthropic", "codex"]);

        assert!(Cadena::crear(&entorno(json!({"modo": "manual"}))).unwrap().is_none());
        let viejo = Cadena::crear(&entorno(json!({"modo": "api"}))).unwrap().unwrap();
        assert_eq!(nombres(&viejo), ["anthropic"]);
        let Err(Fallo::Otro(error)) = Cadena::crear(&entorno(json!({"modo": "otro"}))) else { panic!() };
        assert!(error.starts_with("ia.modo «otro» desconocido. Opciones: manual, claude_code"));
    }

    #[test]
    fn a_failing_planner_gives_way_to_the_next_which_stays_first() {
        let trabajo = trabajo("cadena");
        let (analisis, inventario) = material();
        let material = Material { analisis: &analisis, inventario: &inventario };
        let caido = Guionizado::new("caido", vec![Err(Fallo::Otro("sin red".into()))]);
        let bueno = Guionizado::new("bueno", vec![Ok(BUENO)]);
        let mut cadena = Cadena::new(vec![Box::new(caido), Box::new(bueno)]);
        assert!(cadena.plan(&trabajo, "p", &[], material).is_ok());
        assert_eq!(cadena.nombre(), "bueno");
        assert_eq!(cadena.fallos, ["caido: sin red"]);
        assert_eq!(cadena.orden(), [1, 0]);
        let _ = std::fs::remove_dir_all(&trabajo);
    }

    #[test]
    fn when_none_can_the_quota_or_every_reason_is_told() {
        let trabajo = trabajo("nadie");
        let (analisis, inventario) = material();
        let material = Material { analisis: &analisis, inventario: &inventario };
        let renovacion = DateTime::parse_from_rfc3339("2026-11-08T09:00:00+01:00").unwrap();
        let cuota = Guionizado::new("google", vec![Err(Fallo::Cuota(renovacion))]);
        let roto = Guionizado::new("codex", vec![Err(Fallo::Otro("falló".into()))]);
        let mut cadena = Cadena::new(vec![Box::new(cuota), Box::new(roto)]);
        assert_eq!(cadena.plan(&trabajo, "p", &[], material), Err(Fallo::Cuota(renovacion)));

        let uno = Guionizado::new("a", vec![Err(Fallo::Otro("x".into()))]);
        let dos = Guionizado::new("b", vec![Err(Fallo::Otro("y".into()))]);
        let mut cadena = Cadena::new(vec![Box::new(uno), Box::new(dos)]);
        let fallo = cadena.plan(&trabajo, "p", &[], material).unwrap_err();
        assert_eq!(fallo, Fallo::Otro("Ningún cerebro pudo hacerlo:\n- a: x\n- b: y".into()));
        let _ = std::fs::remove_dir_all(&trabajo);
    }
}
