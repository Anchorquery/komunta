//! Where the montage keeps what is its own, inside the app's data folder:
//! the jobs (`trabajos/`), the clips analysed in any of them and Gemini's
//! quota (`cache/`), the brand kits (`marcas/`), and the settings over the
//! ones it comes with (`config.yaml`, `ajustes.json`).
//!
//! The Python engine kept the same things in its own folder. The first
//! time the app runs, [`Datos::traer_del_motor`] copies them over, so the
//! kits and the analysed clips carry on; the old jobs stay where they are,
//! since the projects made from them point there.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ia::Ia;

/// The settings the montage comes with.
const CONFIG_DE_SERIE: &str = include_str!("config.yaml");

/// A file in the folder that says the old engine's things were brought.
const TRAIDO: &str = ".traido-del-motor";

/// The montage's folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Datos {
    /// `montaje/` in the app's data folder, or, for a project made by the
    /// Python engine, that engine's folder.
    pub carpeta: PathBuf,
}

impl Datos {
    /// The montage's folder inside the app's data folder `datos_app`.
    pub fn en(datos_app: &Path) -> Datos {
        Datos {
            carpeta: datos_app.join("montaje"),
        }
    }

    /// One folder per job.
    pub fn trabajos(&self) -> PathBuf {
        self.carpeta.join("trabajos")
    }

    /// The cache shared by every job: the clips and Gemini's quota.
    pub fn carpeta_cache(&self) -> PathBuf {
        self.carpeta.join("cache")
    }

    /// The clips analysed in any job, by their fingerprint.
    pub fn cache(&self) -> PathBuf {
        self.carpeta_cache().join("clips")
    }

    /// The brand kits, one folder each, and `_comun` with the shared fonts.
    pub fn carpeta_marcas(&self) -> PathBuf {
        self.carpeta.join("marcas")
    }

    /// The settings: the ones it comes with, the folder's `config.yaml`
    /// over them.
    pub fn config(&self) -> Value {
        let mut config: Value = serde_yaml::from_str(CONFIG_DE_SERIE).expect("the built-in settings are YAML");
        if let Some(propio) = leer_yaml(&self.carpeta.join("config.yaml")) {
            crate::ajustes::fusionar(&mut config, propio);
        }
        config
    }

    /// The settings for a run with the team's choices `ia`: [`Self::config`],
    /// `ajustes.json` over it, then `ia` - the planner included - over both.
    pub fn ajustes(&self, ia: &Ia) -> Value {
        let mut config = self.config();
        let cambiados = std::fs::read_to_string(self.carpeta.join("ajustes.json"))
            .ok()
            .and_then(|texto| serde_json::from_str(&texto).ok());
        if let Some(cambiados) = cambiados {
            crate::ajustes::fusionar(&mut config, cambiados);
        }
        crate::ajustes::aplicar(&mut config, &Value::Object(ia.cambios()));
        if let Some(cerebro) = &ia.cerebro {
            crate::ajustes::poner(&mut config, "ia.modo", cerebro.clone().into());
        }
        config
    }

    /// The warm flare's strength where a kit does not set its own.
    pub fn destello(&self) -> f64 {
        self.config()["video"]["destello"].as_f64().unwrap_or(0.45)
    }

    /// The music of the brand kit `marca`, when it has one.
    pub fn musica(&self, marca: &str) -> Option<PathBuf> {
        crate::kit::Kit::cargar(&self.carpeta_marcas(), marca, 0.0)
            .ok()?
            .musica
    }

    /// The brand kits, by folder name; `_comun` is not one.
    pub fn marcas(&self) -> Vec<String> {
        let mut marcas: Vec<String> = std::fs::read_dir(self.carpeta_marcas())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("kit.yaml").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('_'))
            .collect();
        marcas.sort();
        marcas
    }

    /// Copies the Python engine's kits, cache and settings from its folder
    /// `motor`, once: what the folder already has is kept, and nothing is
    /// looked for again after the first time. True when something came.
    pub fn traer_del_motor(&self, motor: &Path) -> io::Result<bool> {
        let marca = self.carpeta.join(TRAIDO);
        if marca.exists() {
            return Ok(false);
        }
        std::fs::create_dir_all(&self.carpeta)?;
        let rutas = leer_yaml(&motor.join("config.yaml")).unwrap_or_default();
        let de_motor = |clave: &str| motor.join(rutas["rutas"][clave].as_str().unwrap_or(clave));
        let mut traido = false;
        traido |= traer(&de_motor("marcas"), &self.carpeta_marcas())?;
        traido |= traer(&de_motor("cache"), &self.carpeta_cache())?;
        for archivo in ["config.yaml", "ajustes.json"] {
            traido |= traer(&motor.join(archivo), &self.carpeta.join(archivo))?;
        }
        std::fs::write(&marca, motor.display().to_string())?;
        Ok(traido)
    }
}

/// A YAML file as JSON; None when it is not there or not YAML.
fn leer_yaml(ruta: &Path) -> Option<Value> {
    let texto = std::fs::read_to_string(ruta).ok()?;
    serde_yaml::from_str(&texto).ok()
}

/// Copies the file or folder `desde` to `hasta` unless `hasta` is there
/// already. True when it copied.
fn traer(desde: &Path, hasta: &Path) -> io::Result<bool> {
    if hasta.exists() || !desde.exists() {
        return Ok(false);
    }
    if desde.is_dir() {
        copiar_carpeta(desde, hasta)?;
    } else {
        std::fs::copy(desde, hasta)?;
    }
    Ok(true)
}

fn copiar_carpeta(desde: &Path, hasta: &Path) -> io::Result<()> {
    std::fs::create_dir_all(hasta)?;
    for entrada in std::fs::read_dir(desde)? {
        let entrada = entrada?;
        let destino = hasta.join(entrada.file_name());
        if entrada.file_type()?.is_dir() {
            copiar_carpeta(&entrada.path(), &destino)?;
        } else {
            std::fs::copy(entrada.path(), destino)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carpeta(nombre: &str) -> PathBuf {
        let carpeta = std::env::temp_dir().join(format!("montaje-datos-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    #[test]
    fn the_folders_are_inside_the_app_s() {
        let datos = Datos::en(Path::new("D:/datos"));
        assert_eq!(datos.trabajos(), Path::new("D:/datos/montaje/trabajos"));
        assert_eq!(datos.cache(), Path::new("D:/datos/montaje/cache/clips"));
        assert_eq!(datos.carpeta_marcas(), Path::new("D:/datos/montaje/marcas"));
    }

    #[test]
    fn the_folder_s_settings_go_over_the_built_in_ones() {
        let datos = Datos { carpeta: carpeta("config") };
        assert_eq!(datos.config()["ia"]["modo"], "claude_code");
        assert_eq!(datos.destello(), 0.45);
        std::fs::write(datos.carpeta.join("config.yaml"), "ia:\n  modo: google\nvideo:\n  destello: 0.2\n").unwrap();
        std::fs::write(datos.carpeta.join("ajustes.json"), r#"{"ia": {"google": {"modelo": "x"}}}"#).unwrap();
        let ajustes = datos.ajustes(&Ia::default());
        assert_eq!(ajustes["ia"]["modo"], "google");
        assert_eq!(ajustes["ia"]["google"]["modelo"], "x");
        // What the folder does not say stays as it comes.
        assert_eq!(ajustes["ia"]["timeout_seg"], 1200);
        assert_eq!(datos.destello(), 0.2);
        let ia = Ia {
            cerebro: Some("anthropic".into()),
            ..Ia::default()
        };
        assert_eq!(datos.ajustes(&ia)["ia"]["modo"], "anthropic");
    }

    #[test]
    fn the_engine_s_things_are_brought_once() {
        let motor = carpeta("motor");
        std::fs::create_dir_all(motor.join("marcas/fiesta")).unwrap();
        std::fs::write(motor.join("marcas/fiesta/kit.yaml"), "nombre: Fiesta\n").unwrap();
        std::fs::create_dir_all(motor.join("otra_cache/clips")).unwrap();
        std::fs::write(motor.join("otra_cache/clips/abc.json"), "{}").unwrap();
        std::fs::write(motor.join("config.yaml"), "rutas:\n  cache: otra_cache\n").unwrap();
        std::fs::create_dir_all(motor.join("trabajos/uno")).unwrap();

        let datos = Datos { carpeta: carpeta("app").join("montaje") };
        assert!(datos.traer_del_motor(&motor).unwrap());
        assert_eq!(datos.marcas(), ["fiesta"]);
        assert!(datos.cache().join("abc.json").is_file());
        assert!(datos.carpeta.join("config.yaml").is_file());
        // The old jobs stay where the projects made from them point.
        assert!(!datos.trabajos().exists());

        // Not again, even with more there.
        std::fs::create_dir_all(motor.join("marcas/otra")).unwrap();
        std::fs::write(motor.join("marcas/otra/kit.yaml"), "nombre: Otra\n").unwrap();
        std::fs::remove_dir_all(datos.carpeta_marcas()).unwrap();
        assert!(!datos.traer_del_motor(&motor).unwrap());
        assert!(datos.marcas().is_empty());
    }

    #[test]
    fn with_no_engine_nothing_is_brought() {
        let datos = Datos { carpeta: carpeta("sin-motor") };
        assert!(!datos.traer_del_motor(Path::new("Z:/no/hay/motor")).unwrap());
        assert!(datos.carpeta.join(TRAIDO).is_file());
    }
}
