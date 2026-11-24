// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The agent's hands: the editor's MCP tools and the agent's own recipes,
//! run on the project on screen from the thread the conversation runs on.
//!
//! The MCP tools ([`super::asistente_catalogo`]) go through [`Vivo`], a
//! backend that carries each request to the window's event loop and waits
//! for the answer there ([`crate::studio::Studio::atender_asistente`]);
//! what does not touch the project (probing a file, the effects
//! catalogue) is answered on this thread. The recipes
//! ([`super::asistente_recetas`]) read and edit through the same window.
//! A frame is gathered on the event loop and painted here, so the window
//! never waits for the compositor.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use analisis::caras::{BuscaCaras, Detector};
use animacion::seguir::Caja;
use asistente::{Ejecutor, Herramienta, Imagen, Salida, Tipo};
use concat_api::{ApiError, Reply, Request};
use concat_export::ExportClip;
use concat_host::preview::{FrameSpec, Monitor};
use concat_mcp::Backend;
use concat_mcp::tools::{Context, Toolbox};
use concat_project::model::Project;
use concat_project::{Command, DocumentSettings};
use serde_json::{Value, json};

use super::asistente_catalogo::{self as catalogo, quita_muchos};
use super::asistente_guion as guion;
use super::asistente_montaje as montaje;
use super::asistente_recetas::{self as recetas, Lienzo};
use crate::en_vivo;
use crate::studio::Studio;

/// The longest side of a frame shown to the AI: enough to read a title
/// and see a face, about a thousand tokens.
const LADO_FOTOGRAMA: u32 = 768;

/// The longest side of a frame looked at for faces: YuNet takes 640.
const LADO_CARAS: u32 = 640;

/// What the window hands over for a frame to be painted off its thread.
pub struct Materiales {
    /// The timeline, flattened.
    pub clips: Arc<Vec<ExportClip>>,
    /// The project's settings.
    pub ajustes: DocumentSettings,
    /// The compositor.
    pub monitor: Monitor,
    /// The frame's time, size and colour.
    pub spec: FrameSpec,
}

/// A backend that asks the window, for the project open in it.
pub struct Vivo {
    /// The project the turn began in; a request for a project since
    /// closed is never answered (`host::on_ui_in_project`).
    epoch: u64,
    /// The turn's gesture: its edits are one undo step.
    gesto: String,
    /// YuNet, loaded the first time a face is looked for.
    detector: Option<Detector>,
}

impl Vivo {
    /// A backend for the open project, its edits under `gesto`.
    pub fn new(gesto: String) -> Self {
        Self {
            epoch: crate::host::project_epoch(),
            gesto,
            detector: None,
        }
    }

    /// Runs `trabajo` on the window's event loop with the studio and the
    /// turn's gesture, and waits for what it gives.
    pub(super) fn en_ventana<T: Send + 'static>(
        &self,
        trabajo: impl FnOnce(&mut Studio, &str) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let gesto = self.gesto.clone();
        en_vivo::en_ventana(Some(self.epoch), move |studio| trabajo(studio, &gesto))
    }
}

impl Backend for Vivo {
    fn call(&mut self, request: Request) -> Result<Value, ApiError> {
        if let Some(local) = aqui(&request) {
            return local;
        }
        let request = en_vivo::preparar(request)?;
        self.en_ventana(move |studio, gesto| studio.atender_asistente(gesto, request))
    }

    fn describe(&self) -> Value {
        json!({ "mode": "window-live" })
    }
}

/// The requests answered on this thread: those that do not touch the
/// project.
fn aqui(request: &Request) -> Option<Result<Value, ApiError>> {
    let reply = match request {
        Request::MediaProbe { path } => concat_host::media::probe(path)
            .map(Reply::Media)
            .map_err(ApiError::failed),
        Request::CatalogueList { kind } => concat_api::catalogue(kind.as_deref()).map(Reply::Packages),
        _ => return None,
    };
    Some(reply.and_then(|reply| {
        serde_json::to_value(reply).map_err(|error| ApiError::failed(error.to_string()))
    }))
}

impl Lienzo for Vivo {
    fn proyecto(&mut self) -> Result<Project, String> {
        self.en_ventana(|studio, _| studio.proyecto_asistente())
            .map_err(|error| error.message)
    }

    fn aplicar(&mut self, comando: Command) -> Result<Option<String>, String> {
        self.en_ventana(move |studio, gesto| studio.editar_asistente(gesto, comando).map(|vista| vista.created_id))
            .map_err(|error| error.message)
    }

    fn fotograma(&mut self, tiempo: f64) -> Result<Imagen, String> {
        let materiales = self
            .en_ventana(move |studio, _| studio.fotograma_asistente(tiempo, LADO_FOTOGRAMA))
            .map_err(|error| error.message)?;
        pintar(materiales, tiempo)
    }

    fn caras(&mut self, tiempo: f64) -> Result<Vec<Caja>, String> {
        let materiales = self
            .en_ventana(move |studio, _| studio.fotograma_asistente(tiempo, LADO_CARAS))
            .map_err(|error| error.message)?;
        let (ancho, alto) = (materiales.spec.width, materiales.spec.height);
        let rgba = materiales.monitor.frame(materiales.clips, &materiales.ajustes, materiales.spec)?;
        let detector = match &mut self.detector {
            Some(detector) => detector,
            hueco => hueco.insert(Detector::new()?),
        };
        let (w, h) = (f64::from(ancho), f64::from(alto));
        Ok(detector
            .caras(&rgba, ancho as usize, alto as usize)?
            .into_iter()
            .map(|cara| Caja {
                x: cara.x / w,
                y: cara.y / h,
                ancho: cara.ancho / w,
                alto: cara.alto / h,
            })
            .collect())
    }
}

/// The frame `materiales` make, as a JPEG.
fn pintar(materiales: Materiales, tiempo: f64) -> Result<Imagen, String> {
    let Materiales {
        clips,
        ajustes,
        monitor,
        spec,
    } = materiales;
    let pixeles = monitor.frame(clips, &ajustes, spec)?;
    let mut rgb = Vec::with_capacity(pixeles.len() / 4 * 3);
    for pixel in pixeles.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode(&rgb, spec.width, spec.height, image::ColorType::Rgb8.into())
        .map_err(|error| format!("el fotograma no se pudo codificar: {error}"))?;
    Ok(Imagen {
        mime: "image/jpeg".into(),
        datos: jpeg,
        nota: format!("t={tiempo:.2} s, {}×{}", spec.width, spec.height),
    })
}

/// The tools, run through [`Vivo`] on the project at `ruta`.
pub struct Herramientas {
    caja: Toolbox,
    vivo: Vivo,
    ruta: String,
    /// Stop, as the person pressed it: a long tool gives up early.
    parada: Arc<AtomicBool>,
}

impl Herramientas {
    /// The tools for the project at `ruta`, its edits under `gesto`; a
    /// tool under way gives up when `parada` is set.
    pub fn new(ruta: String, gesto: String, parada: Arc<AtomicBool>) -> Self {
        Self {
            caja: Toolbox::new(),
            vivo: Vivo::new(gesto),
            ruta,
            parada,
        }
    }

    /// The timeline as `timeline_read` gives it, to go with the person's
    /// words: the AI starts the turn knowing the ids, without asking.
    pub fn linea_de_tiempo(&mut self) -> Option<Value> {
        let inicio = std::time::Instant::now();
        let salida = self.ejecutar("timeline_read", json!({}));
        log::info!("asistente: timeline for the context {} ms", inicio.elapsed().as_millis());
        salida.ok().then_some(salida.valor)
    }

    fn mcp(&mut self, nombre: &str, argumentos: Value) -> Salida {
        let argumentos = con_ruta(argumentos, &self.ruta);
        let contexto = Context {
            backend: &mut self.vivo,
            cancelled: &self.parada,
            progress: None,
        };
        match self.caja.call(nombre, argumentos, contexto) {
            Some(resultado) if resultado.is_error => {
                Salida::mal(Tipo::Error, texto_de(resultado.structured, &resultado.content))
            }
            Some(resultado) => Salida::bien(valor_de(resultado.structured, &resultado.content)),
            None => Salida::mal(Tipo::Error, format!("no hay ninguna herramienta «{nombre}»")),
        }
    }
}

impl Ejecutor for Herramientas {
    fn herramientas(&self) -> Vec<Herramienta> {
        let mut todas = catalogo::herramientas(&self.caja);
        todas.extend(recetas::herramientas());
        todas.extend(guion::herramientas());
        todas.extend(montaje::herramientas());
        todas
    }

    /// The clips a call names exist on the timeline: a wrong id is told to
    /// the AI before anything runs or is put to the person.
    fn comprobar(&mut self, _nombre: &str, argumentos: &Value) -> Result<(), String> {
        let nombrados = ids_de_clips(argumentos);
        if nombrados.is_empty() {
            return Ok(());
        }
        let proyecto = self.vivo.proyecto()?;
        let linea = proyecto.active();
        match nombrados.iter().find(|id| linea.clip(id).is_none()) {
            Some(id) => Err(format!("no hay ningún clip {id} en la línea de tiempo: mira timeline_read")),
            None => Ok(()),
        }
    }

    fn pide_aprobacion(&self, herramienta: &Herramienta, argumentos: &Value) -> bool {
        herramienta.clase.pide_aprobacion() || quita_muchos(&herramienta.nombre, argumentos)
    }

    fn detalle(&self, nombre: &str, argumentos: &Value) -> String {
        guion::detalle(nombre, argumentos)
            .or_else(|| montaje::detalle(nombre, argumentos))
            .unwrap_or_else(|| detalle(nombre, argumentos))
    }

    fn ejecutar(&mut self, nombre: &str, argumentos: Value) -> Salida {
        if recetas::es_receta(nombre) {
            return recetas::ejecutar(&mut self.vivo, nombre, &argumentos);
        }
        if guion::es_del_guion(nombre) {
            return guion::ejecutar(&mut self.vivo, nombre, &argumentos);
        }
        if montaje::es_del_montaje(nombre) {
            return montaje::ejecutar(&mut self.vivo, nombre, &argumentos);
        }
        if catalogo::ofrecida(nombre).is_none() {
            return Salida::mal(Tipo::Error, format!("no hay ninguna herramienta «{nombre}»"));
        }
        self.mcp(nombre, argumentos)
    }
}

/// What a call to approve will do, in the person's words.
fn detalle(nombre: &str, argumentos: &Value) -> String {
    match nombre {
        "clips_remove" => {
            let ids = ids_de_clips(argumentos);
            crate::i18n::tf("asistente.approve.remove", &[&ids.len(), &ids.join(", ")])
        }
        "media_import" => crate::i18n::tf(
            "asistente.approve.import",
            &[&argumentos["file"].as_str().unwrap_or_default()],
        ),
        "project_set_video" => crate::i18n::tf(
            "asistente.approve.video",
            &[&argumentos["video"]["width"], &argumentos["video"]["height"]],
        ),
        otra => format!("{otra} {argumentos}"),
    }
}

/// The clip ids a call names: `clipId`, `clipIds`, or the clips of `moves`.
fn ids_de_clips(argumentos: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    if let Some(id) = argumentos["clipId"].as_str() {
        ids.push(id.to_owned());
    }
    for lista in [&argumentos["clipIds"], &argumentos["moves"]] {
        for elemento in lista.as_array().into_iter().flatten() {
            let id = elemento.as_str().or_else(|| elemento["clipId"].as_str());
            ids.extend(id.map(str::to_owned));
        }
    }
    ids
}

/// `argumentos` naming the open project, whatever path the AI gave.
fn con_ruta(argumentos: Value, ruta: &str) -> Value {
    let mut objeto = match argumentos {
        Value::Object(objeto) => objeto,
        _ => serde_json::Map::new(),
    };
    objeto.insert("path".to_owned(), json!(ruta));
    Value::Object(objeto)
}

/// What a tool gave, as JSON: its structured result, or its text.
fn valor_de(estructurado: Option<Value>, contenido: &[Value]) -> Value {
    estructurado.unwrap_or_else(|| json!(texto_de(None, contenido)))
}

/// Why a tool failed, in words.
fn texto_de(estructurado: Option<Value>, contenido: &[Value]) -> String {
    let texto: Vec<&str> = contenido.iter().filter_map(|c| c["text"].as_str()).collect();
    match (texto.is_empty(), estructurado) {
        (true, Some(valor)) => valor.to_string(),
        _ => texto.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_put_back_in_the_call() {
        let llamada = con_ruta(json!({"time": 2.0, "path": "C:/otro"}), "C:/proyecto");
        assert_eq!(llamada, json!({"time": 2.0, "path": "C:/proyecto"}));
        assert_eq!(con_ruta(Value::Null, "C:/p"), json!({"path": "C:/p"}));
    }

    #[test]
    fn a_result_without_structure_reads_as_its_text() {
        let contenido = [json!({"type": "text", "text": "hecho"})];
        assert_eq!(valor_de(None, &contenido), json!("hecho"));
        assert_eq!(valor_de(Some(json!({"a": 1})), &contenido), json!({"a": 1}));
    }

    #[test]
    fn the_clips_a_call_names_are_found_wherever_it_names_them() {
        assert_eq!(ids_de_clips(&json!({"clipId": "c1"})), ["c1"]);
        assert_eq!(ids_de_clips(&json!({"clipIds": ["c1", "c2"]})), ["c1", "c2"]);
        assert_eq!(
            ids_de_clips(&json!({"moves": [{"clipId": "c3", "start": 1, "trackId": "t1"}]})),
            ["c3"]
        );
        assert!(ids_de_clips(&json!({"time": 1})).is_empty());
    }

    #[test]
    fn the_catalogue_and_a_probe_are_answered_without_the_window() {
        assert!(aqui(&Request::CatalogueList { kind: None }).is_some_and(|r| r.is_ok()));
        assert!(aqui(&Request::Version).is_none());
    }
}
