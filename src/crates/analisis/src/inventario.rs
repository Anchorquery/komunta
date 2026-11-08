//! A job's `inventario.json`: what the material folder holds - videos,
//! stills, sound files and documents, each with the id the analysis and
//! the plan know it by - and what was set aside because it would not open.
//! The engine's `motor/ingesta.py` format, field for field.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file's name in a job folder.
pub const ARCHIVO: &str = "inventario.json";

/// A material folder, listed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventario {
    /// The material folder.
    pub entrada: PathBuf,
    /// Footage.
    pub videos: Vec<Video>,
    /// Stills.
    pub imagenes: Vec<Imagen>,
    /// Sound files: music, or a voice recorded apart.
    pub audios: Vec<Audio>,
    /// Scripts, captions and notes.
    pub documentos: Vec<Documento>,
    /// Files of a known kind that would not open.
    pub descartados: Vec<Descartado>,
}

/// A video.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    /// Its name in the job, unique.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// Seconds long.
    pub duracion: f64,
    /// Displayed width (a quarter turn already applied).
    pub ancho: u32,
    /// Displayed height.
    pub alto: u32,
    /// Frames per second.
    pub fps: f64,
    /// It has a sound stream.
    pub audio: bool,
}

/// A still.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Imagen {
    /// Its name in the job, unique.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// Width, the EXIF orientation applied.
    pub ancho: u32,
    /// Height, the EXIF orientation applied.
    pub alto: u32,
}

/// A sound file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Its name in the job, unique.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// Seconds long.
    pub duracion: f64,
}

/// A document, as its non-empty lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Documento {
    /// Its name in the job, unique.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// What it is for.
    pub rol: Rol,
    /// Its lines, trimmed, without the empty ones.
    pub lineas: Vec<String>,
}

/// What a document is for, by its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rol {
    /// What is to be said.
    Guion,
    /// The captions, written.
    Subtitulos,
    /// Anything else.
    #[default]
    Notas,
}

/// A file set aside, and why, in words for the team.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Descartado {
    /// Its name, without the folder.
    pub archivo: String,
    /// Why.
    pub motivo: String,
}

impl Inventario {
    /// The one in the job folder `trabajo`.
    pub fn leer(trabajo: &Path) -> io::Result<Self> {
        let texto = std::fs::read_to_string(trabajo.join(ARCHIVO))?;
        serde_json::from_str(&texto).map_err(io::Error::other)
    }

    /// Writes it into the job folder `trabajo`, creating the folder - whole,
    /// through a temporary file, so no reader sees half of it.
    pub fn escribir(&self, trabajo: &Path) -> io::Result<()> {
        std::fs::create_dir_all(trabajo)?;
        let ruta = trabajo.join(ARCHIVO);
        let temporal = ruta.with_extension("json.tmp");
        let texto = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        std::fs::write(&temporal, texto)?;
        std::fs::rename(&temporal, &ruta)
    }

    /// The ids it gives, for a new entry not to take one of them.
    pub fn ids(&self) -> Ids {
        let videos = self.videos.iter().map(|v| &v.id);
        let imagenes = self.imagenes.iter().map(|i| &i.id);
        let audios = self.audios.iter().map(|a| &a.id);
        let documentos = self.documentos.iter().map(|d| &d.id);
        Ids(videos
            .chain(imagenes)
            .chain(audios)
            .chain(documentos)
            .cloned()
            .collect())
    }
}

/// The ids given so far in a job, each once.
#[derive(Clone, Debug, Default)]
pub struct Ids(HashSet<String>);

impl Ids {
    /// A new id for the file named `nombre`: its stem with every run of
    /// characters other than ASCII letters, digits, `_` and `-` made one
    /// `_`, numbered `_2`, `_3`... when that is taken.
    pub fn nuevo(&mut self, nombre: &str) -> String {
        let base = base(nombre);
        let mut candidato = base.clone();
        let mut n = 2;
        while self.0.contains(&candidato) {
            candidato = format!("{base}_{n}");
            n += 1;
        }
        self.0.insert(candidato.clone());
        candidato
    }
}

/// A file name's stem in id characters, or `archivo` when none are left.
fn base(nombre: &str) -> String {
    let tallo = Path::new(nombre)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut limpio = String::with_capacity(tallo.len());
    let mut en_hueco = false;
    for c in tallo.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            limpio.push(c);
            en_hueco = false;
        } else if !en_hueco {
            limpio.push('_');
            en_hueco = true;
        }
    }
    let limpio = limpio.trim_matches('_');
    if limpio.is_empty() {
        "archivo".to_owned()
    } else {
        limpio.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_the_stem_in_plain_characters() {
        let mut ids = Ids::default();
        assert_eq!(ids.nuevo("Entrevista José (1).MOV"), "Entrevista_Jos_1");
        assert_eq!(ids.nuevo("clip.final.mp4"), "clip_final");
        assert_eq!(ids.nuevo("__a-b__.mov"), "a-b");
        assert_eq!(ids.nuevo("ñ.jpg"), "archivo");
    }

    #[test]
    fn a_taken_id_is_numbered_from_two() {
        let mut ids = Ids::default();
        assert_eq!(ids.nuevo("clip.mov"), "clip");
        assert_eq!(ids.nuevo("clip.jpg"), "clip_2");
        assert_eq!(ids.nuevo("clip.txt"), "clip_3");
    }

    #[test]
    fn the_engine_s_inventory_reads_and_writes_back() {
        let texto = r#"{"entrada": "M", "videos": [{"id": "A", "ruta": "M/a.mov", "duracion": 1.5,
            "ancho": 1080, "alto": 1920, "fps": 30.0, "audio": true}],
            "imagenes": [], "audios": [], "documentos": [{"id": "g", "ruta": "M/g.txt",
            "rol": "guion", "lineas": ["Hola"]}], "descartados": [{"archivo": "x.mov", "motivo": "m"}]}"#;
        let inventario: Inventario = serde_json::from_str(texto).unwrap();
        assert_eq!(inventario.videos[0].alto, 1920);
        assert_eq!(inventario.documentos[0].rol, Rol::Guion);

        let trabajo = std::env::temp_dir().join(format!("analisis-inv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&trabajo);
        inventario.escribir(&trabajo).unwrap();
        assert_eq!(Inventario::leer(&trabajo).unwrap(), inventario);
        assert!(!trabajo.join("inventario.json.tmp").exists());
    }

    #[test]
    fn the_ids_of_an_inventory_are_not_given_again() {
        let inventario = Inventario {
            videos: vec![Video {
                id: "guion".into(),
                ..Video::default()
            }],
            ..Inventario::default()
        };
        assert_eq!(inventario.ids().nuevo("guion.docx"), "guion_2");
    }
}
