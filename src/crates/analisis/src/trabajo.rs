//! A job's clips analysed into its `analisis.json`: the videos the engine's
//! ingest listed in `inventario.json` that the file does not hold yet, each
//! heard ([`crate::clip::escuchar`]) and looked at
//! ([`crate::mirada::mirar`]). The file is written after every clip, so a
//! stopped run resumes where it stopped; the engine reads it back and only
//! analyses what is missing - a clip that failed here, say.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};

/// The engine's `analisis.fotogramas_por_clip`.
pub const FOTOGRAMAS_POR_CLIP: usize = 3;

/// A video as the ingest lists it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Video {
    /// Its name in the job, unique.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// Seconds long.
    pub duracion: f64,
    /// It has a sound stream.
    pub audio: bool,
}

/// The videos the ingest listed in `<trabajo>/inventario.json`.
pub fn videos(trabajo: &Path) -> io::Result<Vec<Video>> {
    #[derive(Deserialize)]
    struct Inventario {
        videos: Vec<Video>,
    }
    let texto = std::fs::read_to_string(trabajo.join("inventario.json"))?;
    let inventario: Inventario = serde_json::from_str(&texto).map_err(io::Error::other)?;
    Ok(inventario.videos)
}

/// A job's `analisis.json`, as far as it has got. Whatever else the engine
/// keeps in it is left as it was.
#[derive(Debug)]
pub struct Archivo {
    ruta: PathBuf,
    datos: Map<String, Value>,
}

impl Archivo {
    /// The one in `trabajo`, or an empty one when there is none yet.
    pub fn abrir(trabajo: &Path) -> io::Result<Self> {
        let ruta = trabajo.join("analisis.json");
        let datos = match std::fs::read_to_string(&ruta) {
            Ok(texto) => serde_json::from_str(&texto).map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Map::new(),
            Err(error) => return Err(error),
        };
        Ok(Self { ruta, datos })
    }

    /// The ids of the clips it holds.
    pub fn analizados(&self) -> HashSet<&str> {
        self.clips()
            .iter()
            .filter_map(|clip| clip.get("id")?.as_str())
            .collect()
    }

    /// The `videos` it does not hold yet, in their order.
    pub fn pendientes<'v>(&self, videos: &'v [Video]) -> Vec<&'v Video> {
        let analizados = self.analizados();
        videos
            .iter()
            .filter(|video| !analizados.contains(video.id.as_str()))
            .collect()
    }

    /// Adds a clip's entry and writes the file - whole, through a
    /// temporary one, so the engine never reads half of it.
    pub fn anadir(&mut self, clip: Value) -> io::Result<()> {
        let clips = self
            .datos
            .entry("clips")
            .or_insert_with(|| Value::Array(Vec::new()));
        if !clips.is_array() {
            *clips = Value::Array(Vec::new());
        }
        if let Value::Array(clips) = clips {
            clips.push(clip);
        }
        let temporal = self.ruta.with_extension("json.tmp");
        std::fs::write(
            &temporal,
            serde_json::to_vec(&self.datos).map_err(io::Error::other)?,
        )?;
        std::fs::rename(&temporal, &self.ruta)
    }

    fn clips(&self) -> &[Value] {
        self.datos
            .get("clips")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }
}

#[cfg(all(feature = "escucha", feature = "imagen"))]
pub use analizar::{Analista, Avance, ErrorTrabajo, Resumen, analizar};

#[cfg(all(feature = "escucha", feature = "imagen"))]
mod analizar {
    use std::path::Path;

    use serde::Serialize;

    use super::{Archivo, Video, videos};
    use crate::caras::BuscaCaras;
    use crate::clip::{self, Ajustes, Escucha};
    use crate::escucha::Oido;
    use crate::mirada::{self, Mirada};

    /// What a run uses: whisper, the face detector if there is one, and the
    /// engine's settings.
    pub struct Analista<'a> {
        /// whisper and the voice detector.
        pub oido: &'a mut Oido,
        /// None leaves the frames without faces.
        pub caras: Option<&'a mut dyn BuscaCaras>,
        /// The sound half's settings.
        pub ajustes: Ajustes,
        /// Frames kept per clip.
        pub fotogramas: usize,
    }

    /// Where a run is: about to analyse clip `n` (from 0) of `de`.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Avance<'a> {
        /// The clip about to be analysed.
        pub n: usize,
        /// The clips this run analyses.
        pub de: usize,
        /// The clip's id.
        pub id: &'a str,
    }

    /// How a run went.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct Resumen {
        /// The clips analysed and written.
        pub analizados: usize,
        /// The clips that could not be, with why; left for the engine.
        pub fallidos: Vec<(String, String)>,
    }

    /// Why a run could not go on.
    #[derive(Debug, thiserror::Error)]
    pub enum ErrorTrabajo {
        /// The inventory or the analysis file could not be read or written.
        #[error("{}: {error}", ruta.display())]
        Archivo {
            /// The file.
            ruta: std::path::PathBuf,
            /// What happened.
            error: std::io::Error,
        },
        /// `seguir` said stop.
        #[error("cancelado")]
        Cancelado,
    }

    /// A clip's entry in `analisis.json`: the engine's names, flat.
    #[derive(Serialize)]
    struct ClipAnalizado<'a> {
        id: &'a str,
        duracion: f64,
        #[serde(flatten)]
        escucha: Escucha,
        #[serde(flatten)]
        mirada: Mirada,
    }

    /// Analyses the clips of the job in `trabajo` that its `analisis.json`
    /// lacks, writing each as it is done. `seguir` hears before each clip
    /// and stops the run by answering false. A clip that fails is skipped
    /// and told in the [`Resumen`]: the engine analyses it after.
    pub fn analizar(
        trabajo: &Path,
        analista: &mut Analista,
        mut seguir: impl FnMut(Avance) -> bool,
    ) -> Result<Resumen, ErrorTrabajo> {
        let archivo_de = |nombre: &str| {
            let ruta = trabajo.join(nombre);
            move |error| ErrorTrabajo::Archivo { ruta, error }
        };
        let videos = videos(trabajo).map_err(archivo_de("inventario.json"))?;
        let mut archivo = Archivo::abrir(trabajo).map_err(archivo_de("analisis.json"))?;
        let pendientes: Vec<Video> = archivo.pendientes(&videos).into_iter().cloned().collect();
        let mut resumen = Resumen::default();
        for (n, video) in pendientes.iter().enumerate() {
            let avance = Avance {
                n,
                de: pendientes.len(),
                id: &video.id,
            };
            if !seguir(avance) {
                return Err(ErrorTrabajo::Cancelado);
            }
            match uno(analista, video, trabajo) {
                Ok(clip) => {
                    archivo.anadir(clip).map_err(archivo_de("analisis.json"))?;
                    resumen.analizados += 1;
                }
                Err(motivo) => {
                    log::warn!("{}: {motivo}", video.id);
                    resumen.fallidos.push((video.id.clone(), motivo));
                }
            }
        }
        Ok(resumen)
    }

    /// One clip's entry, or why there is none.
    fn uno(
        analista: &mut Analista,
        video: &Video,
        trabajo: &Path,
    ) -> Result<serde_json::Value, String> {
        let escucha = clip::escuchar(
            analista.oido,
            &video.ruta,
            video.duracion,
            video.audio,
            &analista.ajustes,
        )
        .map_err(|error| error.to_string())?;
        let caras = analista
            .caras
            .as_mut()
            .map(|d| &mut **d as &mut dyn BuscaCaras);
        let mirada = mirada::mirar(
            caras,
            &video.ruta,
            &video.id,
            video.duracion,
            analista.fotogramas,
            trabajo,
        )
        .map_err(|error| error.to_string())?;
        let clip = ClipAnalizado {
            id: &video.id,
            duracion: video.duracion,
            escucha,
            mirada,
        };
        serde_json::to_value(clip).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carpeta(nombre: &str) -> PathBuf {
        let carpeta =
            std::env::temp_dir().join(format!("analisis-trabajo-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    fn video(id: &str) -> Video {
        Video {
            id: id.into(),
            ruta: format!("{id}.mov").into(),
            duracion: 2.0,
            audio: true,
        }
    }

    #[test]
    fn the_inventory_s_videos_are_read() {
        let trabajo = carpeta("inventario");
        std::fs::write(
            trabajo.join("inventario.json"),
            r#"{"entrada": "x", "videos": [{"id": "A", "ruta": "a.mov", "duracion": 1.5, "audio": false, "fps": 30.0}], "imagenes": []}"#,
        )
        .unwrap();
        assert_eq!(
            videos(&trabajo).unwrap(),
            [Video {
                id: "A".into(),
                ruta: "a.mov".into(),
                duracion: 1.5,
                audio: false
            }]
        );
    }

    #[test]
    fn with_no_file_every_video_is_pending() {
        let trabajo = carpeta("vacio");
        let archivo = Archivo::abrir(&trabajo).unwrap();
        let todos = [video("A"), video("B")];
        assert_eq!(archivo.pendientes(&todos), [&todos[0], &todos[1]]);
    }

    #[test]
    fn clips_are_added_one_by_one_and_the_rest_is_kept() {
        let trabajo = carpeta("anadir");
        std::fs::write(
            trabajo.join("analisis.json"),
            r#"{"clips": [{"id": "A", "duracion": 2.0}], "imagenes": [{"id": "foto"}]}"#,
        )
        .unwrap();
        let mut archivo = Archivo::abrir(&trabajo).unwrap();
        let todos = [video("A"), video("B"), video("C")];
        assert_eq!(archivo.pendientes(&todos), [&todos[1], &todos[2]]);

        archivo
            .anadir(serde_json::json!({"id": "B", "duracion": 2.0}))
            .unwrap();
        let releido = Archivo::abrir(&trabajo).unwrap();
        assert_eq!(releido.pendientes(&todos), [&todos[2]]);
        assert_eq!(releido.datos["imagenes"][0]["id"], "foto");
        assert!(!trabajo.join("analisis.json.tmp").exists());
    }

    #[test]
    fn a_broken_file_is_an_error_not_an_empty_one() {
        let trabajo = carpeta("roto");
        std::fs::write(trabajo.join("analisis.json"), "{\"clips\": [").unwrap();
        assert!(Archivo::abrir(&trabajo).is_err());
    }
}
