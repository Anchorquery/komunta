//! A job's clips analysed into its `analisis.json`: the videos the ingest
//! ([`crate::ingesta`]) listed in `inventario.json` that the file does not
//! hold yet, each heard ([`crate::clip::escuchar`]) and looked at
//! ([`crate::mirada::mirar`]). The file is written after every clip, so a
//! stopped run resumes where it stopped; the engine reads it back and only
//! analyses what is missing - a clip that failed here, say.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::inventario::Inventario;
pub use crate::inventario::Video;

/// The engine's `analisis.fotogramas_por_clip`.
pub const FOTOGRAMAS_POR_CLIP: usize = 3;

/// The videos the ingest listed in `<trabajo>/inventario.json`.
pub fn videos(trabajo: &Path) -> io::Result<Vec<Video>> {
    Inventario::leer(trabajo).map(|inventario| inventario.videos)
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

    /// Adds a clip's entry and writes the file.
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
        self.escribir()
    }

    /// Sets `clave` - `imagenes`, `audios` - to `valor` and writes the file.
    pub fn poner(&mut self, clave: &str, valor: Value) -> io::Result<()> {
        self.datos.insert(clave.to_owned(), valor);
        self.escribir()
    }

    /// Writes the file whole, through a temporary one, so the engine never
    /// reads half of it.
    fn escribir(&self) -> io::Result<()> {
        let temporal = self.ruta.with_extension("json.tmp");
        std::fs::write(
            &temporal,
            serde_json::to_vec(&self.datos).map_err(io::Error::other)?,
        )?;
        std::fs::rename(&temporal, &self.ruta)
    }

    /// What the file holds.
    pub fn datos(&self) -> &Map<String, Value> {
        &self.datos
    }

    /// Sets the `descripcion` of each clip or still in `descripciones`, by
    /// id, and writes the file once. Returns the clips' entries as they now
    /// are, for the cache; an id the file does not hold is skipped.
    pub fn describir(&mut self, descripciones: Vec<(String, Value)>) -> io::Result<Vec<Value>> {
        let mut clips = Vec::new();
        for (id, descripcion) in descripciones {
            for clave in ["clips", "imagenes"] {
                let entrada = self
                    .datos
                    .get_mut(clave)
                    .and_then(Value::as_array_mut)
                    .and_then(|lista| lista.iter_mut().find(|e| e.get("id").and_then(Value::as_str) == Some(&id)));
                if let Some(entrada) = entrada {
                    entrada["descripcion"] = descripcion.clone();
                    if clave == "clips" {
                        clips.push(entrada.clone());
                    }
                    break;
                }
            }
        }
        self.escribir()?;
        Ok(clips)
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
    use std::path::{Path, PathBuf};

    use serde::Serialize;
    use serde_json::Value;

    use super::{Archivo, Video};
    use crate::cache::Cache;
    use crate::caras::BuscaCaras;
    use crate::clip::{self, Ajustes, Escucha};
    use crate::escucha::Oido;
    use crate::inventario::Inventario;
    use crate::material::{self, MUSICA_KIT, Sonido};
    use crate::mirada::{self, Mirada};

    /// What a run uses: whisper, the face detector if there is one, the
    /// engine's settings, the clips other jobs analysed, and the brand
    /// kit's music.
    pub struct Analista<'a> {
        /// whisper and the voice detector.
        pub oido: &'a mut Oido,
        /// None leaves the frames without faces.
        pub caras: Option<&'a mut dyn BuscaCaras>,
        /// The sound half's settings.
        pub ajustes: Ajustes,
        /// Frames kept per clip.
        pub fotogramas: usize,
        /// None analyses every clip afresh and keeps none.
        pub cache: Option<&'a Cache>,
        /// The music of the job's brand kit, analysed beside the job's
        /// own sound files.
        pub musica_kit: Option<PathBuf>,
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
        /// Of those, the ones another job had analysed.
        pub de_cache: usize,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        clave_cache: Option<String>,
        #[serde(flatten)]
        escucha: Escucha,
        #[serde(flatten)]
        mirada: Mirada,
    }

    /// Analyses the clips of the job in `trabajo` that its `analisis.json`
    /// lacks, writing each as it is done, then its stills and sound files.
    /// `seguir` hears before each clip and stops the run by answering
    /// false. A clip that fails is skipped and told in the [`Resumen`]: the
    /// engine analyses it after.
    pub fn analizar(
        trabajo: &Path,
        analista: &mut Analista,
        mut seguir: impl FnMut(Avance) -> bool,
    ) -> Result<Resumen, ErrorTrabajo> {
        let archivo_de = |nombre: &str| {
            let ruta = trabajo.join(nombre);
            move |error| ErrorTrabajo::Archivo { ruta, error }
        };
        let inventario = Inventario::leer(trabajo).map_err(archivo_de("inventario.json"))?;
        let mut archivo = Archivo::abrir(trabajo).map_err(archivo_de("analisis.json"))?;
        let pendientes: Vec<Video> = archivo
            .pendientes(&inventario.videos)
            .into_iter()
            .cloned()
            .collect();
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
            let clave = analista.cache.and_then(|cache| {
                cache
                    .clave(&video.ruta)
                    .inspect_err(|error| log::warn!("{}: not cached: {error}", video.id))
                    .ok()
            });
            let visto = analista
                .cache
                .zip(clave.as_deref())
                .and_then(|(cache, clave)| cache.leer(clave, &video.id, trabajo));
            if let Some(clip) = visto {
                archivo.anadir(clip).map_err(archivo_de("analisis.json"))?;
                resumen.analizados += 1;
                resumen.de_cache += 1;
                continue;
            }
            match uno(analista, video, clave, trabajo) {
                Ok(clip) => {
                    archivo
                        .anadir(clip.clone())
                        .map_err(archivo_de("analisis.json"))?;
                    if let Some(cache) = analista.cache
                        && let Err(error) = cache.guardar(trabajo, &clip)
                    {
                        log::warn!("{}: not cached: {error}", video.id);
                    }
                    resumen.analizados += 1;
                }
                Err(motivo) => {
                    log::warn!("{}: {motivo}", video.id);
                    resumen.fallidos.push((video.id.clone(), motivo));
                }
            }
        }
        let imagenes = material::imagenes(
            analista
                .caras
                .as_mut()
                .map(|d| &mut **d as &mut dyn BuscaCaras),
            &inventario.imagenes,
        );
        let sonidos = sonidos(&inventario, analista.musica_kit.as_deref());
        let audios = material::audios(analista.oido, &sonidos);
        for (clave, valor) in [("imagenes", json(imagenes)), ("audios", json(audios))] {
            archivo
                .poner(clave, valor)
                .map_err(archivo_de("analisis.json"))?;
        }
        Ok(resumen)
    }

    /// The job's sound files, and the kit's music when there is one that
    /// can be read.
    fn sonidos(inventario: &Inventario, musica_kit: Option<&Path>) -> Vec<Sonido> {
        let mut sonidos: Vec<Sonido> = inventario.audios.iter().map(Sonido::from).collect();
        if let Some(ruta) = musica_kit {
            match crate::ingesta::sondear(ruta) {
                Ok(medio) => sonidos.push(Sonido {
                    id: MUSICA_KIT.into(),
                    ruta: ruta.to_path_buf(),
                    duracion: (medio.duracion * 1000.0).round() / 1000.0,
                }),
                Err(motivo) => log::warn!("the kit's music is not analysed: {motivo}"),
            }
        }
        sonidos
    }

    fn json<T: Serialize>(valor: T) -> Value {
        serde_json::to_value(valor).unwrap_or_default()
    }

    /// One clip's entry, or why there is none; `clave` is its key in the
    /// cache.
    fn uno(
        analista: &mut Analista,
        video: &Video,
        clave: Option<String>,
        trabajo: &Path,
    ) -> Result<Value, String> {
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
            clave_cache: clave,
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
            ..Video::default()
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
                fps: 30.0,
                audio: false,
                ..Video::default()
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
