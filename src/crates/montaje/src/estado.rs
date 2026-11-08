//! A job's `estado.json`: where it stands ("analizando", "planificando",
//! "esperando_plan"...) and what was chosen for it - template, brand kit,
//! planner - as the engine's `pipeline.py` keeps it. Written whole into a
//! temporary file and moved into place, so no one reads half of it.

use std::io;
use std::path::Path;

use serde_json::{Map, Value, json};

/// The job's state; `{"estado": "nuevo"}` when it has none yet.
pub fn leer(trabajo: &Path) -> Map<String, Value> {
    std::fs::read_to_string(trabajo.join("estado.json"))
        .ok()
        .and_then(|texto| serde_json::from_str(&texto).ok())
        .unwrap_or_else(|| {
            let mut nuevo = Map::new();
            nuevo.insert("estado".into(), json!("nuevo"));
            nuevo
        })
}

/// Sets the job's state to `estado` and the fields of `extra`, keeping the
/// rest.
pub fn guardar(trabajo: &Path, estado: &str, extra: Map<String, Value>) -> io::Result<()> {
    let mut datos = leer(trabajo);
    datos.insert("estado".into(), json!(estado));
    let ahora = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    datos.insert("actualizado".into(), json!(ahora));
    datos.extend(extra);
    std::fs::create_dir_all(trabajo)?;
    let provisional = trabajo.join("estado.json.tmp");
    let texto = serde_json::to_string_pretty(&datos).map_err(io::Error::other)?;
    std::fs::write(&provisional, texto)?;
    std::fs::rename(provisional, trabajo.join("estado.json"))
}

/// Keeps the state, setting only the fields of `extra`.
pub fn anotar(trabajo: &Path, extra: Map<String, Value>) -> io::Result<()> {
    let estado = leer(trabajo)["estado"].as_str().unwrap_or("nuevo").to_owned();
    guardar(trabajo, &estado, extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_changes_and_what_was_there_stays() {
        let trabajo = std::env::temp_dir().join(format!("komunta-estado-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&trabajo);
        assert_eq!(leer(&trabajo)["estado"], "nuevo");
        let mut marca = Map::new();
        marca.insert("marca".into(), json!("andoain"));
        guardar(&trabajo, "analizando", marca).unwrap();
        guardar(&trabajo, "planificando", Map::new()).unwrap();
        let mut cerebro = Map::new();
        cerebro.insert("cerebro_usado".into(), json!("google"));
        anotar(&trabajo, cerebro).unwrap();
        let estado = leer(&trabajo);
        assert_eq!((estado["estado"].as_str(), estado["marca"].as_str()), (Some("planificando"), Some("andoain")));
        assert_eq!(estado["cerebro_usado"], "google");
        assert!(!trabajo.join("estado.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&trabajo);
    }
}
