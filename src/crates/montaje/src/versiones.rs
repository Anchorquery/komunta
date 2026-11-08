//! The plan's versions: every plan the job has had, kept in `ia/versiones`
//! as `v1.json`, `v2.json`... with `versiones.json` saying where each came
//! from, so any of them can be brought back. The engine's `edicion.py`
//! numbering, which jobs it made still carry.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One version of the plan.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Version {
    /// Its number, from 1.
    pub n: u32,
    /// ia, segunda_pasada, edicion, manual, deshacer or komunta.
    pub origen: String,
    /// What it was, for the team.
    pub nota: Option<String>,
    /// What changed, line by line.
    pub cambios: Vec<String>,
    /// When, to the second, in local time.
    pub fecha: Option<String>,
}

fn carpeta(trabajo: &Path) -> PathBuf {
    trabajo.join("ia").join("versiones")
}

fn plan(trabajo: &Path) -> PathBuf {
    trabajo.join("ia").join("plan.json")
}

/// The job's versions, oldest first; none when it has none.
pub fn versiones(trabajo: &Path) -> Vec<Version> {
    std::fs::read_to_string(carpeta(trabajo).join("versiones.json"))
        .ok()
        .and_then(|texto| serde_json::from_str(&texto).ok())
        .unwrap_or_default()
}

/// Keeps the job's `ia/plan.json` as a new version and returns its number:
/// the last one's when the plan has not changed since, 0 when there is no
/// plan.
pub fn guardar(trabajo: &Path, origen: &str, nota: Option<&str>, cambios: &[String]) -> io::Result<u32> {
    let Ok(texto) = std::fs::read_to_string(plan(trabajo)) else {
        return Ok(0);
    };
    let dir = carpeta(trabajo);
    std::fs::create_dir_all(&dir)?;
    let mut lista = versiones(trabajo);
    if let Some(ultima) = lista.last() {
        let anterior = std::fs::read_to_string(dir.join(format!("v{}.json", ultima.n))).unwrap_or_default();
        if anterior == texto {
            return Ok(ultima.n);
        }
    }
    let n = lista.last().map_or(1, |ultima| ultima.n + 1);
    std::fs::write(dir.join(format!("v{n}.json")), &texto)?;
    lista.push(Version {
        n,
        origen: origen.into(),
        nota: nota.map(str::to_owned),
        cambios: cambios.to_vec(),
        fecha: Some(chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
    });
    let indice = serde_json::to_string_pretty(&lista).map_err(io::Error::other)?;
    std::fs::write(dir.join("versiones.json"), indice)?;
    Ok(n)
}

/// Brings version `n` back as the job's plan.
pub fn restaurar(trabajo: &Path, n: u32) -> io::Result<()> {
    let ruta = carpeta(trabajo).join(format!("v{n}.json"));
    let texto = std::fs::read_to_string(&ruta)
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, format!("No existe la versión {n}")))?;
    std::fs::write(plan(trabajo), texto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_plan_is_not_kept_twice_and_any_version_comes_back() {
        let trabajo = std::env::temp_dir().join(format!("komunta-versiones-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&trabajo);
        std::fs::create_dir_all(trabajo.join("ia")).unwrap();
        assert_eq!(guardar(&trabajo, "ia", None, &[]).unwrap(), 0);

        std::fs::write(plan(&trabajo), "{\"a\": 1}").unwrap();
        assert_eq!(guardar(&trabajo, "ia", Some("Plan inicial"), &[]).unwrap(), 1);
        assert_eq!(guardar(&trabajo, "manual", None, &[]).unwrap(), 1);
        std::fs::write(plan(&trabajo), "{\"a\": 2}").unwrap();
        assert_eq!(guardar(&trabajo, "edicion", Some("otra"), &["x".into()]).unwrap(), 2);

        let lista = versiones(&trabajo);
        assert_eq!(lista.iter().map(|v| v.origen.as_str()).collect::<Vec<_>>(), ["ia", "edicion"]);
        restaurar(&trabajo, 1).unwrap();
        assert_eq!(std::fs::read_to_string(plan(&trabajo)).unwrap(), "{\"a\": 1}");
        assert!(restaurar(&trabajo, 9).is_err());
        let _ = std::fs::remove_dir_all(&trabajo);
    }
}
