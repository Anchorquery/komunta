//! A montage project's link to its job: `montaje.json` in the project
//! folder says which job, recipe, kit and frames made it, so the script
//! pane can find the plan again and "Aplicar al vídeo" can rebuild it.
//!
//! Applying never touches what is already on the timelines: the plan
//! comes in as new timelines ("Montaje 2 · Vertical"...) beside the old
//! ones, so the team's hand edits are still there to compare or copy from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use concat_project::{DocumentSettings, Project, from_document, to_document};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kit::Kit;
use crate::motor::Motor;
use crate::proyecto::{Formato, Opciones};
use crate::receta::Receta;
use crate::{Error, sondeo};

/// The file in the project folder.
pub const ARCHIVO: &str = "montaje.json";

/// What made a montage project.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Encargo {
    /// The engine's job folder.
    pub trabajo: PathBuf,
    /// The engine's folder, for its brand kits.
    pub motor: PathBuf,
    /// The recipe the job was made with.
    pub receta: Receta,
    /// Brand kit id; None is the neutral look.
    pub marca: Option<String>,
    /// Frames, the main one first: "vertical", "16x9", "1x1".
    pub formatos: Vec<String>,
    /// How many times the plan has been laid out; the next timelines carry
    /// the number after it.
    pub version: u32,
    /// The material folder the job was read from, to ask again with.
    pub material: PathBuf,
    /// The recipe asked for, by id: "automatica" when the AI chose
    /// `receta`.
    pub pedida: String,
    /// "¿Qué quieres?", as it was asked.
    pub peticion: String,
}

impl Encargo {
    /// The link of the project in `proyecto`, if it is a montage.
    pub fn leer(proyecto: &Path) -> Option<Encargo> {
        let texto = std::fs::read_to_string(proyecto.join(ARCHIVO)).ok()?;
        serde_json::from_str(&texto).ok()
    }

    /// Writes the link into the project folder.
    pub fn guardar(&self, proyecto: &Path) -> Result<(), Error> {
        let ruta = proyecto.join(ARCHIVO);
        let texto = serde_json::to_string_pretty(self).expect("always serialises");
        std::fs::write(&ruta, texto).map_err(|e| Error::Escribir(ruta, e))
    }

    /// The layout options for this job, the project's folder holding the
    /// drawn marks.
    pub fn opciones(&self, proyecto: &Path) -> Result<Opciones, Error> {
        let motor = Motor {
            carpeta: self.motor.clone(),
            python: String::new(),
        };
        let marcas = motor.carpeta.join("marcas");
        let kit = match &self.marca {
            Some(id) => Kit::cargar(&marcas, id, destello(&motor.carpeta))?,
            None => Kit::neutro(&marcas.join("_comun").join("fuentes")),
        };
        let mut formatos: Vec<Formato> = self
            .formatos
            .iter()
            .filter_map(|f| Formato::de(f))
            .collect();
        if formatos.is_empty() {
            formatos.push(Formato::de("vertical").expect("vertical is a frame"));
        }
        Ok(Opciones {
            formatos,
            fps: 30,
            kit,
            carpeta: proyecto.to_path_buf(),
            sondear: sondeo::ffprobe,
        })
    }

    /// Lays the job out as a new project in `proyecto` and links it.
    pub fn crear(&mut self, proyecto: &Path, nombre: &str) -> Result<PathBuf, Error> {
        let opciones = self.opciones(proyecto)?;
        let (project, settings) = crate::montar(&self.trabajo, nombre, &self.receta, &opciones)?;
        let ruta = crate::escribir(proyecto, &settings, &project)?;
        self.version = 1;
        self.guardar(proyecto)?;
        Ok(ruta)
    }

    /// Lays the job's plan out again as new timelines of the project in
    /// `proyecto`, after the ones it has, and makes the first of them the
    /// open one. Works on the project's saved file.
    pub fn aplicar(&mut self, proyecto: &Path) -> Result<(), Error> {
        let ruta = proyecto.join("concat.json");
        let texto = std::fs::read_to_string(&ruta).map_err(|e| Error::Leer(ruta.clone(), e))?;
        let documento: Value = serde_json::from_str(&texto).map_err(|e| Error::Json(ruta, e))?;
        let nuevo = self.aplicar_documento(&documento, proyecto)?;
        let ruta = proyecto.join("concat.json");
        let texto = serde_json::to_string_pretty(&nuevo).expect("a document always serialises");
        std::fs::write(&ruta, texto).map_err(|e| Error::Escribir(ruta, e))?;
        self.guardar(proyecto)
    }

    /// [`Encargo::aplicar`] on a project document in memory - the editor's
    /// own, unsaved edits and all - returning the document to save. The
    /// version count goes up; saving the link is the caller's.
    pub fn aplicar_documento(
        &mut self,
        documento: &Value,
        proyecto: &Path,
    ) -> Result<Value, Error> {
        let nuevo = self.preparar(proyecto)?;
        self.injertar(documento, nuevo)
    }

    /// The slow half of [`Encargo::aplicar_documento`]: the plan laid out
    /// on its own, every file probed. For a worker.
    pub fn preparar(&self, proyecto: &Path) -> Result<Project, Error> {
        let opciones = self.opciones(proyecto)?;
        let (nuevo, _) = crate::montar(&self.trabajo, "Montaje", &self.receta, &opciones)?;
        Ok(nuevo)
    }

    /// The quick half: `nuevo`'s timelines added to the document, so it can
    /// be done on the document as it is at that moment.
    pub fn injertar(&mut self, documento: &Value, nuevo: Project) -> Result<Value, Error> {
        let mut destino = from_document(documento).ok_or(Error::Proyecto)?;
        let nombre = documento["name"].as_str().unwrap_or("Montaje").to_owned();
        // A project's first montage takes the place of the empty timeline
        // it was made with.
        if self.version == 0 && !nuevo.timelines.is_empty() {
            destino.timelines.retain(|tl| !tl.clips.is_empty());
        }
        self.version += 1;
        anadir(&mut destino, nuevo, self.version);
        let video = &destino.active().video;
        let settings = DocumentSettings {
            name: nombre,
            width: video.width,
            height: video.height,
            rate_num: video.rate_num,
            rate_den: video.rate_den,
        };
        Ok(to_document(&settings, &destino))
    }
}

/// The engine's flare strength from its `config.yaml` (`video.destello`),
/// or its default.
fn destello(motor: &Path) -> f64 {
    let texto = std::fs::read_to_string(motor.join("config.yaml")).unwrap_or_default();
    let valor: serde_yaml::Value = serde_yaml::from_str(&texto).unwrap_or_default();
    valor["video"]["destello"].as_f64().unwrap_or(0.45)
}

/// Adds `nuevo`'s timelines to `destino`, numbered `version`, with every
/// id made fresh past the ones `destino` uses and media matched by file,
/// so a clip already in the bin is not listed twice. The first of them
/// becomes the open timeline.
pub fn anadir(destino: &mut Project, nuevo: Project, version: u32) {
    let mut siguiente = mayor_id(destino) + 1;
    let mut fresco = |prefijo: &str| {
        let id = format!("{prefijo}{siguiente}");
        siguiente += 1;
        id
    };
    // The new project's media ids, to the ids they have in `destino`.
    let mut medios: HashMap<String, String> = HashMap::new();
    for mut medio in nuevo.media {
        let viejo = medio.id.clone();
        let id = match destino.media.iter().find(|m| m.path == medio.path) {
            Some(existente) => existente.id.clone(),
            None => {
                medio.id = fresco("m");
                let id = medio.id.clone();
                destino.media.push(medio);
                id
            }
        };
        medios.insert(viejo, id);
    }
    for fuente in nuevo.fonts {
        if !destino
            .fonts
            .iter()
            .any(|f| f.family == fuente.family && f.path == fuente.path)
        {
            destino.fonts.push(fuente);
        }
    }
    let mut primera = None;
    for linea in nuevo.timelines {
        let mut linea = (*linea).clone();
        linea.id = fresco("tl");
        linea.name = linea
            .name
            .replacen("Montaje", &format!("Montaje {version}"), 1);
        let mut pistas: HashMap<String, String> = HashMap::new();
        for pista in &mut linea.tracks {
            let id = fresco("t");
            pistas.insert(std::mem::replace(&mut pista.id, id.clone()), id);
        }
        for clip in &mut linea.clips {
            let clip = std::sync::Arc::make_mut(clip);
            clip.id = fresco("c");
            if let Some(id) = pistas.get(&clip.track_id) {
                clip.track_id = id.clone();
            }
            if let Some(id) = medios.get(&clip.media_id) {
                clip.media_id = id.clone();
            }
        }
        primera.get_or_insert_with(|| linea.id.clone());
        destino.timelines.push(std::sync::Arc::new(linea));
    }
    if let Some(id) = primera {
        destino.active_timeline_id = id;
    }
}

/// The highest number behind any id in the project: they share one
/// counter, whatever their letters.
fn mayor_id(project: &Project) -> u64 {
    let numero = |id: &str| -> u64 {
        id.trim_start_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .unwrap_or(0)
    };
    let mut mayor = 0;
    for m in &project.media {
        mayor = mayor.max(numero(&m.id));
    }
    for tl in &project.timelines {
        mayor = mayor.max(numero(&tl.id));
        for t in &tl.tracks {
            mayor = mayor.max(numero(&t.id));
        }
        for c in &tl.clips {
            mayor = mayor.max(numero(&c.id));
        }
    }
    mayor
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::*;
    use crate::entrada::Trabajo;

    fn proyecto() -> Project {
        let trabajo = Trabajo {
            plan: serde_json::from_value(json!({
                "tomas": [{"clip": "a", "desde": 0, "hasta": 1}],
                "subtitulos": [{"texto": "hola mundo", "toma": 0, "desde": 0, "hasta": 1}]
            }))
            .unwrap(),
            analisis: serde_json::from_value(json!({"clips": [{
                "id": "a", "duracion": 3.0,
                "transcripcion": {"palabras": [
                    {"i": 0, "ini": 0.5, "fin": 0.9, "txt": "hola"},
                    {"i": 1, "ini": 1.0, "fin": 1.4, "txt": "mundo"}
                ]}
            }]}))
            .unwrap(),
            inventario: serde_json::from_value(json!({"videos": [{
                "id": "a", "ruta": "C:/material/a.mp4", "duracion": 3.0,
                "ancho": 1080, "alto": 1920, "fps": 30.0, "audio": true
            }]}))
            .unwrap(),
        };
        let linea = crate::construir_linea(&trabajo).unwrap();
        let opciones = Opciones {
            formatos: vec![
                Formato::de("vertical").unwrap(),
                Formato::de("1x1").unwrap(),
            ],
            fps: 30,
            kit: Kit::neutro(Path::new("no-hay-fuentes")),
            carpeta: std::env::temp_dir(),
            sondear: |_| None,
        };
        crate::construir_proyecto(&trabajo, &linea, &opciones).unwrap()
    }

    #[test]
    fn a_new_layout_comes_in_beside_the_old_one() {
        let mut destino = proyecto();
        let antes = destino.timelines.len();
        anadir(&mut destino, proyecto(), 2);
        assert_eq!(destino.timelines.len(), antes * 2);
        // The same file is one item in the bin.
        assert_eq!(destino.media.len(), 1);
        let nombres: Vec<&str> = destino.timelines.iter().map(|t| t.name.as_str()).collect();
        assert!(nombres.contains(&"Montaje 2 · Vertical"), "{nombres:?}");
        let activa = destino.active();
        assert_eq!(activa.name, "Montaje 2 · Vertical");
        // Every id once, and every clip on a lane and a file that exist.
        let mut ids = HashSet::new();
        for tl in &destino.timelines {
            assert!(ids.insert(tl.id.clone()));
            for t in &tl.tracks {
                assert!(ids.insert(t.id.clone()));
            }
            for c in &tl.clips {
                assert!(ids.insert(c.id.clone()), "{}", c.id);
                assert!(tl.tracks.iter().any(|t| t.id == c.track_id));
                assert!(c.media_id.is_empty() || destino.media.iter().any(|m| m.id == c.media_id));
            }
        }
    }

    #[test]
    fn a_first_montage_takes_the_place_of_the_empty_timeline() {
        let mut destino = proyecto();
        let mut vacia = (*destino.timelines[0]).clone();
        vacia.id = "tl900".to_owned();
        vacia.name = "Timeline 1".to_owned();
        vacia.clips.clear();
        destino.timelines.push(std::sync::Arc::new(vacia));
        let video = &destino.active().video;
        let ajustes = DocumentSettings {
            name: "Fiesta".to_owned(),
            width: video.width,
            height: video.height,
            rate_num: video.rate_num,
            rate_den: video.rate_den,
        };
        let documento = to_document(&ajustes, &destino);
        let mut encargo = Encargo::default();
        let primero = encargo.injertar(&documento, proyecto()).unwrap();
        assert_eq!(encargo.version, 1);
        let primero = from_document(&primero).unwrap();
        assert!(primero.timelines.iter().all(|tl| !tl.clips.is_empty()));
        assert!(!primero.timelines.iter().any(|tl| tl.name == "Timeline 1"));
        // A second one keeps what is there, empty or not.
        let segundo = encargo.injertar(&documento, proyecto()).unwrap();
        let segundo = from_document(&segundo).unwrap();
        assert!(segundo.timelines.iter().any(|tl| tl.name == "Timeline 1"));
    }
}
