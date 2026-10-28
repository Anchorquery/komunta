//! Komunta's montage: the job folder the engine leaves (the inventory, the
//! analysis and the AI's plan) turned into an editable project.
//!
//! [`linea`] works out the timeline in seconds, as the Python engine's
//! `construir_linea` does; [`proyecto`] lays it out as clips on lanes, one
//! timeline per frame. A [`receta`] says what kind of video it is and what
//! happens to captions; a [`kit`] brings the client's look. The result is
//! an ordinary project folder the editor opens, where every cut, caption
//! and piece of B-roll can still be changed.

pub mod entrada;
pub mod garabatos;
pub mod kit;
pub mod linea;
pub mod proyecto;
pub mod receta;
pub mod sondeo;

use std::path::{Path, PathBuf};

use concat_project::{DocumentSettings, Editor, Project, to_document};

pub use entrada::Trabajo;
pub use kit::Kit;
pub use linea::{Linea, construir_linea};
pub use proyecto::{Formato, Opciones, construir_proyecto};
pub use receta::Receta;

/// What can go wrong reading a job or writing its project.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A file could not be read.
    #[error("no se pudo leer {0}: {1}")]
    Leer(PathBuf, std::io::Error),
    /// A file is not the JSON the engine writes.
    #[error("{0} no es válido: {1}")]
    Json(PathBuf, serde_json::Error),
    /// A brand kit's `kit.yaml` cannot be read.
    #[error("el kit de marca {0} no es válido: {1}")]
    Kit(PathBuf, String),
    /// The plan names a clip the inventory or the analysis does not have.
    #[error("el plan usa «{0}», que no está en el inventario o el análisis")]
    Falta(String),
    /// The plan names a take that does not exist.
    #[error("el plan usa la toma {0}, que no existe")]
    Toma(usize),
    /// The plan names a caption that does not exist.
    #[error("un recurso usa el subtítulo {0}, que no existe")]
    Subtitulo(usize),
    /// The plan names a word past the end of a transcription.
    #[error("el plan usa la palabra {1} de «{0}», que no existe")]
    Palabra(String, usize),
    /// A piece of B-roll is anchored to nothing.
    #[error("el recurso «{0}» no dice dónde va")]
    Anclaje(String),
    /// A hand-drawn mark could not be drawn.
    #[error("no se pudo dibujar el garabato «{0}»")]
    Garabato(String),
    /// The project came out in a shape the editor refuses.
    #[error("el proyecto generado no es válido para el editor")]
    Proyecto,
    /// The project could not be written.
    #[error("no se pudo escribir {0}: {1}")]
    Escribir(PathBuf, std::io::Error),
}

/// The project for a job folder under a recipe, checked by loading it the
/// way the editor does. Captions are cut to fit the main frame's width.
pub fn montar(
    carpeta: &Path,
    nombre: &str,
    receta: &Receta,
    opciones: &Opciones,
) -> Result<(Project, DocumentSettings), Error> {
    let mut trabajo = Trabajo::leer(carpeta)?;
    let principal = opciones.formatos.first();
    let horizontal = principal.is_some_and(|f| f.ancho >= f.alto);
    receta::aplicar(&mut trabajo, receta, if horizontal { 42 } else { 32 });
    let linea = construir_linea(&trabajo)?;
    let project = construir_proyecto(&trabajo, &linea, opciones)?;
    let video = &project.active().video;
    let settings = DocumentSettings {
        name: nombre.to_owned(),
        width: video.width,
        height: video.height,
        rate_num: video.rate_num,
        rate_den: video.rate_den,
    };
    let document = to_document(&settings, &project);
    let editor = Editor::from_document(&document).ok_or(Error::Proyecto)?;
    Ok((editor.project().clone(), settings))
}

/// Writes a project folder (`concat.json` inside `destino`) the editor
/// opens.
pub fn escribir(
    destino: &Path,
    settings: &DocumentSettings,
    project: &Project,
) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(destino).map_err(|error| Error::Escribir(destino.into(), error))?;
    let ruta = destino.join("concat.json");
    let document = to_document(settings, project);
    let texto = serde_json::to_string_pretty(&document).expect("a document always serialises");
    std::fs::write(&ruta, texto).map_err(|error| Error::Escribir(ruta.clone(), error))?;
    Ok(ruta)
}
