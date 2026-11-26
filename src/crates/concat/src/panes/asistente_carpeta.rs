// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The agent's tool for a whole folder of material: every video, sound
//! and picture in it brought into the project, as the Import dialog would
//! bring them, in one call and with one yes. A folder Google Drive for
//! desktop keeps on this computer (G:\…) is one like any other.

use std::path::{Path, PathBuf};

use asistente::{Clase, Herramienta, Salida, Tipo};
use concat_api::Request;
use concat_host::media::MEDIA_EXTENSIONS;
use concat_mcp::Backend;
use serde_json::{Value, json};

/// Brings a folder in: writes into the project, so it asks first.
pub const IMPORTAR: &str = "carpeta_importar";

/// The most files one call brings in: a folder past it is a library, not
/// a shoot, and wants choosing first.
const MAXIMO: usize = 300;

/// Whether `nombre` is this tool.
pub fn es_de_carpeta(nombre: &str) -> bool {
    nombre == IMPORTAR
}

/// The tool, as the agent sees it.
pub fn herramientas() -> Vec<Herramienta> {
    vec![Herramienta::nueva(
        IMPORTAR,
        "Trae al proyecto todos los vídeos, audios e imágenes de una carpeta, como el diálogo Importar: \
         quedan en Medios, no en la línea de tiempo. Sirve una carpeta de Google Drive para escritorio \
         (G:\\…). Para montar el vídeo con IA a partir de una carpeta no hace falta: pásala como \
         «material» a montaje_lanzar.",
        json!({
            "type": "object",
            "properties": {
                "carpeta": {"type": "string", "description": "la carpeta, con su ruta entera"},
                "subcarpetas": {"type": "boolean", "description": "también lo que haya en sus subcarpetas"}
            },
            "required": ["carpeta"]
        }),
        Clase::Archivo,
        "medios",
    )]
}

/// What a call to approve will do, in the person's words.
pub fn detalle(nombre: &str, argumentos: &Value) -> Option<String> {
    es_de_carpeta(nombre).then(|| {
        let (carpeta, subcarpetas) = pedido(argumentos);
        let cuantos = medios(&carpeta, subcarpetas).map_or(0, |medios| medios.len());
        crate::i18n::tf("asistente.approve.importFolder", &[&cuantos, &carpeta.display()])
    })
}

/// Brings the folder's material into the project at `ruta` through
/// `backend`, one `media.import` per file; what does not come in is
/// said, and the rest still does.
pub fn ejecutar(backend: &mut dyn Backend, ruta: &str, argumentos: &Value) -> Salida {
    let (carpeta, subcarpetas) = pedido(argumentos);
    let archivos = match medios(&carpeta, subcarpetas) {
        Ok(archivos) => archivos,
        Err(motivo) => return Salida::mal(Tipo::Error, motivo),
    };
    if archivos.is_empty() {
        return Salida::mal(Tipo::Error, format!("en {} no hay vídeos, audios ni imágenes", carpeta.display()));
    }
    if archivos.len() > MAXIMO {
        return Salida::mal(
            Tipo::Error,
            format!(
                "{} tiene {} archivos, más de {MAXIMO}: elige una carpeta más concreta",
                carpeta.display(),
                archivos.len()
            ),
        );
    }
    let mut traidos = Vec::new();
    let mut fallidos = Vec::new();
    for archivo in archivos {
        let file = archivo.display().to_string();
        let pedido = Request::MediaImport {
            path: ruta.to_owned(),
            file: file.clone(),
        };
        match backend.call(pedido) {
            Ok(_) => traidos.push(file),
            Err(error) => fallidos.push(json!({"archivo": file, "motivo": error.message})),
        }
    }
    if traidos.is_empty() {
        return Salida::mal(Tipo::Error, json!({"no_traidos": fallidos}).to_string());
    }
    Salida::bien(json!({"traidos": traidos, "no_traidos": fallidos}))
}

/// The folder a call names, and whether its subfolders count.
fn pedido(argumentos: &Value) -> (PathBuf, bool) {
    let carpeta = argumentos["carpeta"].as_str().unwrap_or_default().trim();
    (PathBuf::from(carpeta), argumentos["subcarpetas"].as_bool().unwrap_or(false))
}

/// The videos, sounds and pictures in `carpeta`, by name; with
/// `subcarpetas`, those below it too.
fn medios(carpeta: &Path, subcarpetas: bool) -> Result<Vec<PathBuf>, String> {
    let entradas = std::fs::read_dir(carpeta).map_err(|error| format!("no se puede leer {}: {error}", carpeta.display()))?;
    let mut archivos = Vec::new();
    let mut debajo = Vec::new();
    for entrada in entradas.flatten() {
        let camino = entrada.path();
        if camino.is_dir() {
            if subcarpetas {
                debajo.push(camino);
            }
        } else if es_medio(&camino) {
            archivos.push(camino);
        }
    }
    archivos.sort();
    debajo.sort();
    for carpeta in debajo {
        archivos.extend(medios(&carpeta, true)?);
    }
    Ok(archivos)
}

/// Whether the Import dialog would offer `camino`.
fn es_medio(camino: &Path) -> bool {
    camino
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .is_some_and(|extension| MEDIA_EXTENSIONS.contains(&extension.as_str()))
}

#[cfg(test)]
mod tests {
    use concat_api::ApiError;

    use super::*;

    /// A backend that takes every import but one.
    #[derive(Default)]
    struct Bin(Vec<String>);

    impl Backend for Bin {
        fn call(&mut self, request: Request) -> Result<Value, ApiError> {
            let Request::MediaImport { file, .. } = request else {
                return Err(ApiError::invalid("only imports"));
            };
            if file.ends_with("roto.mp4") {
                return Err(ApiError::failed("no se puede leer"));
            }
            self.0.push(file);
            Ok(json!({}))
        }

        fn describe(&self) -> Value {
            json!({})
        }
    }

    /// A scratch folder of its own, gone when the test ends.
    struct Prueba(PathBuf);

    impl Prueba {
        fn nueva(nombre: &str) -> Self {
            let ruta = std::env::temp_dir().join(format!("komunta-carpeta-{nombre}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&ruta);
            std::fs::create_dir_all(&ruta).unwrap();
            Self(ruta)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Prueba {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn carpeta(nombre: &str) -> Prueba {
        let carpeta = Prueba::nueva(nombre);
        for nombre in ["b.MOV", "a.jpg", "notas.txt", "roto.mp4"] {
            std::fs::write(carpeta.path().join(nombre), b"").unwrap();
        }
        std::fs::create_dir(carpeta.path().join("dentro")).unwrap();
        std::fs::write(carpeta.path().join("dentro").join("c.wav"), b"").unwrap();
        carpeta
    }

    #[test]
    fn a_folder_brings_in_its_media_and_says_what_did_not_come() {
        let carpeta = carpeta("una");
        let mut bin = Bin::default();
        let argumentos = json!({"carpeta": carpeta.path().display().to_string()});
        let salida = ejecutar(&mut bin, "/p", &argumentos);
        assert!(salida.ok());
        let nombres: Vec<String> = bin.0.iter().map(|f| Path::new(f).file_name().unwrap().to_string_lossy().into()).collect();
        assert_eq!(nombres, ["a.jpg", "b.MOV"], "by name, no text, nothing from below");
        assert_eq!(salida.valor["no_traidos"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn subfolders_come_only_when_asked() {
        let carpeta = carpeta("sub");
        let mut bin = Bin::default();
        let argumentos = json!({"carpeta": carpeta.path().display().to_string(), "subcarpetas": true});
        ejecutar(&mut bin, "/p", &argumentos);
        assert!(bin.0.iter().any(|f| f.ends_with("c.wav")));
    }

    #[test]
    fn a_folder_with_no_media_or_none_at_all_is_an_error() {
        let vacia = Prueba::nueva("vacia");
        let mut bin = Bin::default();
        assert!(!ejecutar(&mut bin, "/p", &json!({"carpeta": vacia.path().display().to_string()})).ok());
        assert!(!ejecutar(&mut bin, "/p", &json!({"carpeta": "/no/existe"})).ok());
    }
}
