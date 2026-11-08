//! Clips already analysed in another job, kept by what the file holds and
//! not by its name: the same footage redone, renamed or copied elsewhere is
//! not heard and looked at again. Each entry is the clip's line of
//! `analisis.json` and its frames, under a key made of the file's
//! fingerprint and every setting the analysis depends on - change a model
//! and the old entries stop matching.
//!
//! The layout is the engine's `motor/analisis.py` cache (`<clave>.json`,
//! `<clave>_<n>.jpg`), in the same folder, so the engine's description
//! step still adds what the AI saw to the entries made here. The keys are
//! this analysis's own: whisper.cpp does not hear exactly what the
//! engine's whisper heard, so neither takes the other's entries.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha1::{Digest, Sha1};

/// Raise when what is kept for a clip changes: every old entry is missed.
pub const VERSION: u32 = 1;

/// The bytes of each end of a file the fingerprint reads.
const EXTREMO: u64 = 4 << 20;

/// The cache in a folder, for one set of analysis settings.
#[derive(Clone, Debug)]
pub struct Cache {
    carpeta: PathBuf,
    ajustes: Value,
}

impl Cache {
    /// The entries in `carpeta` for the analysis `ajustes` - the models,
    /// the thresholds, whether faces are looked for: anything that changes
    /// what a clip's entry says.
    pub fn new(carpeta: PathBuf, ajustes: Value) -> Self {
        Self { carpeta, ajustes }
    }

    /// The key of the file at `ruta` under these settings.
    pub fn clave(&self, ruta: &Path) -> io::Result<String> {
        let parametros = json!(["rust", VERSION, huella(ruta)?, self.ajustes]);
        Ok(hex(&Sha1::digest(parametros.to_string())))
    }

    /// The entry under `clave`, as the clip `id` of the job in `trabajo`:
    /// its frames copied into the job and renamed for it. None when there
    /// is no entry, or a frame of it is gone.
    pub fn leer(&self, clave: &str, id: &str, trabajo: &Path) -> Option<Value> {
        let texto = std::fs::read_to_string(self.json(clave)).ok()?;
        let Ok(Value::Object(mut clip)) = serde_json::from_str(&texto) else {
            return None;
        };
        let carpeta = trabajo.join("fotogramas");
        std::fs::create_dir_all(&carpeta).ok()?;
        if let Some(Value::Array(fotogramas)) = clip.get_mut("fotogramas") {
            for (n, fotograma) in fotogramas.iter_mut().enumerate() {
                let nombre = format!("{id}_{n}.jpg");
                std::fs::copy(self.fotograma(clave, n), carpeta.join(&nombre)).ok()?;
                fotograma["ruta"] = format!("fotogramas/{nombre}").into();
            }
        }
        let mut entrada = Map::new();
        entrada.insert("id".into(), id.into());
        entrada.append(&mut clip);
        entrada.insert("de_cache".into(), true.into());
        Some(Value::Object(entrada))
    }

    /// Keeps `clip`, an entry of the job in `trabajo` with its
    /// `clave_cache`, for the jobs after it. Frames already kept stay.
    pub fn guardar(&self, trabajo: &Path, clip: &Value) -> io::Result<()> {
        let Some(clave) = clip.get("clave_cache").and_then(Value::as_str) else {
            return Ok(());
        };
        std::fs::create_dir_all(&self.carpeta)?;
        let fotogramas = clip.get("fotogramas").and_then(Value::as_array);
        for (n, fotograma) in fotogramas.into_iter().flatten().enumerate() {
            let destino = self.fotograma(clave, n);
            let origen = fotograma.get("ruta").and_then(Value::as_str).map(|r| trabajo.join(r));
            if let Some(origen) = origen.filter(|o| o.is_file())
                && !destino.exists()
            {
                std::fs::copy(origen, destino)?;
            }
        }
        let mut datos = clip.as_object().cloned().unwrap_or_default();
        datos.remove("id");
        datos.remove("de_cache");
        let ruta = self.json(clave);
        let temporal = ruta.with_extension("json.tmp");
        std::fs::write(&temporal, Value::Object(datos).to_string())?;
        std::fs::rename(temporal, ruta)
    }

    fn json(&self, clave: &str) -> PathBuf {
        self.carpeta.join(format!("{clave}.json"))
    }

    fn fotograma(&self, clave: &str, n: usize) -> PathBuf {
        self.carpeta.join(format!("{clave}_{n}.jpg"))
    }
}

/// What the file at `ruta` holds, without reading all of it: its size, its
/// first 4 MiB and, past 8 MiB, its last 4 MiB - the engine's `huella`.
pub fn huella(ruta: &Path) -> io::Result<String> {
    let mut archivo = File::open(ruta)?;
    let tamano = archivo.metadata()?.len();
    let mut sha = Sha1::new();
    sha.update(tamano.to_string());
    let mut trozo = Vec::new();
    (&mut archivo).take(EXTREMO).read_to_end(&mut trozo)?;
    sha.update(&trozo);
    if tamano > 2 * EXTREMO {
        trozo.clear();
        archivo.seek(SeekFrom::End(-(EXTREMO as i64)))?;
        archivo.read_to_end(&mut trozo)?;
        sha.update(&trozo);
    }
    Ok(hex(&sha.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carpeta(nombre: &str) -> PathBuf {
        let carpeta =
            std::env::temp_dir().join(format!("analisis-cache-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    #[test]
    fn the_fingerprint_is_the_engine_s() {
        let ruta = carpeta("huella").join("clip.mov");
        std::fs::write(&ruta, b"hola").unwrap();
        // hashlib.sha1(b"4" + b"hola").hexdigest()
        assert_eq!(huella(&ruta).unwrap(), "dee4513a2b615cb210990856b56f727e1ee61750");
    }

    #[test]
    fn a_big_file_is_known_by_its_ends() {
        let carpeta = carpeta("extremos");
        let (a, b) = (carpeta.join("a.mov"), carpeta.join("b.mov"));
        let mut contenido = vec![0u8; (3 * EXTREMO) as usize];
        std::fs::write(&a, &contenido).unwrap();
        contenido[EXTREMO as usize + 10] = 1; // the middle: not read
        std::fs::write(&b, &contenido).unwrap();
        assert_eq!(huella(&a).unwrap(), huella(&b).unwrap());
        let ultimo = contenido.len() - 1;
        contenido[ultimo] = 1;
        std::fs::write(&b, &contenido).unwrap();
        assert_ne!(huella(&a).unwrap(), huella(&b).unwrap());
    }

    #[test]
    fn the_same_file_under_other_settings_has_another_key() {
        let ruta = carpeta("claves").join("clip.mov");
        std::fs::write(&ruta, b"material").unwrap();
        let una = Cache::new(PathBuf::new(), json!({"modelo": "large-v3"}));
        let otra = Cache::new(PathBuf::new(), json!({"modelo": "large-v3-eu"}));
        assert_eq!(una.clave(&ruta).unwrap(), una.clave(&ruta).unwrap());
        assert_ne!(una.clave(&ruta).unwrap(), otra.clave(&ruta).unwrap());
    }

    #[test]
    fn a_clip_kept_in_one_job_is_found_in_the_next() {
        let raiz = carpeta("viaje");
        let cache = Cache::new(raiz.join("cache"), json!({}));
        let primero = raiz.join("trabajo1");
        std::fs::create_dir_all(primero.join("fotogramas")).unwrap();
        std::fs::write(primero.join("fotogramas/A_0.jpg"), b"jpeg").unwrap();
        let clip = json!({"id": "A", "duracion": 2.0, "clave_cache": "k",
            "fotogramas": [{"t": 1.0, "ruta": "fotogramas/A_0.jpg", "caras": []}]});
        cache.guardar(&primero, &clip).unwrap();

        let segundo = raiz.join("trabajo2");
        let visto = cache.leer("k", "Toma_1", &segundo).unwrap();

        assert_eq!(visto["id"], "Toma_1");
        assert_eq!(visto["de_cache"], true);
        assert_eq!(visto["duracion"], 2.0);
        assert_eq!(visto["fotogramas"][0]["ruta"], "fotogramas/Toma_1_0.jpg");
        assert_eq!(std::fs::read(segundo.join("fotogramas/Toma_1_0.jpg")).unwrap(), b"jpeg");
        let guardado: Value =
            serde_json::from_str(&std::fs::read_to_string(raiz.join("cache/k.json")).unwrap()).unwrap();
        assert!(guardado.get("id").is_none());
    }

    #[test]
    fn an_entry_missing_a_frame_is_not_used() {
        let raiz = carpeta("sin-fotograma");
        let cache = Cache::new(raiz.join("cache"), json!({}));
        std::fs::create_dir_all(raiz.join("cache")).unwrap();
        std::fs::write(
            raiz.join("cache/k.json"),
            r#"{"duracion": 2.0, "fotogramas": [{"t": 1.0, "ruta": "x", "caras": []}]}"#,
        )
        .unwrap();
        assert!(cache.leer("k", "A", &raiz.join("trabajo")).is_none());
        assert!(cache.leer("otra", "A", &raiz.join("trabajo")).is_none());
    }
}
