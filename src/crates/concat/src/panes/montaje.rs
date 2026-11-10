// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Komunta's montage with AI, which any project can have rather than a
//! kind of project: the "Armar montaje" wizard over the open project (from
//! the title bar, File, or the Montaje pane), and its Settings page.
//!
//! The wizard's three steps are the material, what the video is for, and
//! the captions and the script. Running it works on the open project, on a
//! worker ([`crate::panes::montaje_corrida`]): the material is listed, its
//! clips analysed and described, the AI asked for the plan and the
//! `montaje` crate lays it out, and the result joins the project as new
//! timelines ([`crate::panes::guion::graft`]). Its progress comes back as
//! [`MontajeMsg::Progress`] and shows in the Montaje pane.
//!
//! What the montage keeps - jobs, kits, cache, settings - is in the app's
//! data folder ([`datos`]); the Python engine's are brought from its folder
//! the first time.

use std::path::{Path, PathBuf};
use std::sync::Once;

use concat_project::Project;
use montaje::avance::{Avance, Parada};
use montaje::pedido;
use montaje::proveedores::Proveedor;
use montaje::receta::{self, ModoSubtitulos, Receta};
use montaje::{Datos, Encargo, Pedido};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::panes::montaje_ajustes::AjustesIa;
use crate::panes::montaje_analisis;
use crate::panes::montaje_corrida::Corrida;
use crate::platform;
use crate::studio::{Studio, home_folder};
use crate::ui::{App, GuionPanel, MontajeData, MontajeSettingsData, PaneKind};

/// The montage's Settings page.
pub const SETTINGS_PAGE: i32 = 8;

/// Everything that can happen to the montage.
#[derive(Debug)]
pub enum MontajeMsg {
    /// "Armar montaje": the wizard over the open project, filled in from
    /// its last montage when it has one.
    Open,
    /// The Montaje pane's "Rehacer…": the same.
    Redo,
    Close,
    /// A text field, by name, and what it holds now.
    TextEdited(String, String),
    /// A choice, by name, and its index (0 or 1 for a switch).
    Choice(String, i32),
    /// A button, by name.
    Action(String),
    /// A picker answered: which field, and the path.
    Chosen(String, PathBuf),
    /// Montage the open project as the sheet, or New project, has it.
    Run,
    /// The run reported a step.
    Progress(Avance),
    /// The plan is in; it is being laid out.
    Laying,
    /// The run ended: the montage laid out, or why not. An empty error is
    /// a run that was cancelled.
    Done(Result<(Encargo, Project), String>),
    /// The Settings page: a field changed.
    SettingEdited(String, String),
    /// The Settings page: the AI changed.
    SettingChoice(String, i32),
    /// The Settings page: a button.
    SettingAction(String),
    /// A provider's key proved: its models, or why it failed.
    Tested(Proveedor, Result<Vec<String>, String>),
}

/// The montage's state: what is asked, and the run under way.
pub struct MontajePane {
    /// The sheet is up.
    pub open: bool,
    /// The sheet is redoing the project's montage rather than making one.
    redo: bool,
    /// The project the sheet was last filled in for. Opened again over the
    /// same project it keeps what was chosen and typed; a sheet put down by
    /// mistake used to lose the request with it.
    filled_for: Option<PathBuf>,
    step: i32,
    material: String,
    /// What the material folder holds, worked out when it changes rather
    /// than on every publish.
    material_note: String,
    /// "Automático" first, then the recipes it chooses among.
    recipes: Vec<Receta>,
    recipe: usize,
    request: String,
    vertical: bool,
    wide: bool,
    square: bool,
    /// The brand kits; the sheet lists "no brand" before them.
    brands: Vec<String>,
    /// 0 for no brand, otherwise one past the index into `brands`.
    brand: usize,
    language: i32,
    /// 0 as the kind of video has it, then script, voice, key phrases,
    /// none.
    captions: i32,
    script: String,
    busy: bool,
    /// The project folder the run under way is for.
    running_in: Option<PathBuf>,
    phase: i32,
    progress: f32,
    message: String,
    error: String,
    parada: Parada,
    /// Settings' AI half, between publishes.
    ajustes: AjustesIa,
}

impl Default for MontajePane {
    fn default() -> Self {
        Self {
            open: false,
            redo: false,
            filled_for: None,
            step: 0,
            material: String::new(),
            material_note: String::new(),
            recipes: with_automatic(receta::de_serie()),
            recipe: 0,
            request: String::new(),
            vertical: true,
            wide: false,
            square: false,
            brands: Vec::new(),
            brand: 0,
            language: 0,
            captions: 0,
            script: String::new(),
            busy: false,
            running_in: None,
            phase: 0,
            progress: 0.0,
            message: String::new(),
            error: String::new(),
            parada: Parada::default(),
            ajustes: AjustesIa::default(),
        }
    }
}

/// "Automático" before the recipes, which it chooses among.
fn with_automatic(recipes: Vec<Receta>) -> Vec<Receta> {
    std::iter::once(receta::automatica())
        .chain(recipes.into_iter().filter(|r| !r.es_automatica()))
        .collect()
}

/// The montage's folder in the app's data folder, with the Python
/// engine's kits, cache and settings brought over the first time it is
/// asked for.
pub fn datos(studio: &Studio) -> Datos {
    static TRAER: Once = Once::new();
    let datos = Datos::en(&studio.host.dirs.data);
    TRAER.call_once(|| match datos.traer_del_motor(&motor_viejo(studio)) {
        Ok(true) => log::info!(
            "montaje: the Python engine's kits and cache brought to {}",
            datos.carpeta.display()
        ),
        Ok(false) => {}
        Err(error) => log::warn!("montaje: the Python engine's things were not brought over: {error}"),
    });
    datos
}

/// The Python engine's folder, as Settings once had it or where it
/// usually was: where its kits and cache are brought from, and its `.env`
/// still looked in for keys.
fn motor_viejo(studio: &Studio) -> PathBuf {
    studio
        .prefs
        .montaje
        .motor
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| std::env::var("KOMUNTA_MOTOR").ok())
        .map_or_else(
            || PathBuf::from(home_folder("Desktop/apps/automatizacion_videos")),
            PathBuf::from,
        )
}

/// The old engine's `.env`, for the keys not brought into the credential
/// store yet.
pub fn env_viejo(studio: &Studio) -> PathBuf {
    motor_viejo(studio).join(".env")
}

/// The team's own recipes live beside the app's settings.
fn recipes_folder(studio: &Studio) -> PathBuf {
    studio.host.dirs.config.join("recetas")
}

/// The open project's folder.
fn open_project(studio: &Studio) -> Option<PathBuf> {
    studio
        .session
        .as_ref()
        .map(|session| PathBuf::from(session.path()))
}

impl MontajePane {
    /// Applies one message.
    pub fn update(&mut self, msg: MontajeMsg, studio: &mut Studio) {
        match msg {
            MontajeMsg::Open | MontajeMsg::Redo => self.open_sheet(studio),
            MontajeMsg::Close => self.open = false,
            MontajeMsg::TextEdited(field, value) => match field.as_str() {
                "material" => self.set_material(value),
                "request" => self.request = value,
                _ => {}
            },
            MontajeMsg::Choice(field, value) => {
                let index = value.max(0) as usize;
                match field.as_str() {
                    "recipe" => {
                        self.recipe = index.min(self.recipes.len().saturating_sub(1));
                        self.from_recipe();
                    }
                    "brand" => self.brand = index.min(self.brands.len()),
                    "language" => self.language = value.clamp(0, 2),
                    "captions" => self.captions = value.clamp(0, 4),
                    "vertical" => self.vertical = value != 0,
                    "wide" => self.wide = value != 0,
                    "square" => self.square = value != 0,
                    _ => {}
                }
            }
            MontajeMsg::Action(name) => self.action(&name, studio),
            MontajeMsg::Chosen(field, path) => {
                let path = path.to_string_lossy().into_owned();
                match field.as_str() {
                    "material" => self.set_material(path),
                    "script" => self.script = path,
                    _ => {}
                }
            }
            MontajeMsg::Run => self.run(studio),
            MontajeMsg::Progress(avance) => {
                self.phase = match avance.fase.as_str() {
                    "ingesta" => 0,
                    "analisis" if avance.mensaje.contains("mira") => 2,
                    "analisis" => 1,
                    "descripcion" => 2,
                    "plan" => 3,
                    _ => self.phase,
                };
                self.progress = avance.fraccion as f32;
                self.message = avance.mensaje;
            }
            MontajeMsg::Laying => {
                self.phase = 4;
                self.progress = 0.0;
                self.message = t("montaje.layingOut");
            }
            MontajeMsg::Done(result) => self.done(result, studio),
            MontajeMsg::SettingEdited(field, value) => self.ajustes.editado(&field, value),
            MontajeMsg::SettingChoice(field, value) => self.ajustes.elegido(&field, value, studio),
            MontajeMsg::SettingAction(name) => match name.as_str() {
                "recipes" => {
                    let folder = recipes_folder(studio);
                    reveal(studio, &folder);
                }
                "data" => {
                    let folder = datos(studio).carpeta;
                    reveal(studio, &folder);
                }
                _ => self.ajustes.accion(&name, studio),
            },
            MontajeMsg::Tested(proveedor, resultado) => self.ajustes.probado(proveedor, resultado),
        }
    }

    /// The wizard over the open project; for a project montaged before,
    /// as its last montage was asked for.
    fn open_sheet(&mut self, studio: &Studio) {
        let Some(proyecto) = open_project(studio) else {
            return;
        };
        if self.busy {
            return;
        }
        self.open = true;
        self.step = 0;
        self.error.clear();
        self.refresh(studio);
        let last = Encargo::leer(&proyecto);
        self.redo = last.is_some();
        if !self.keeps_edits_for(&proyecto) {
            if let Some(encargo) = last {
                self.fill(&encargo);
            }
            self.filled_for = Some(proyecto);
        }
    }

    /// Whether the sheet already holds what was chosen for `proyecto`, so
    /// opening it again must not put the last montage's choices back over
    /// them.
    fn keeps_edits_for(&self, proyecto: &Path) -> bool {
        self.filled_for.as_deref() == Some(proyecto)
    }

    /// The recipes and brand kits as they are on disk now.
    fn refresh(&mut self, studio: &Studio) {
        let id = self.recipes.get(self.recipe).map(|r| r.id.clone());
        self.recipes = with_automatic(receta::cargar(&recipes_folder(studio)));
        self.recipe = id
            .and_then(|id| self.recipes.iter().position(|r| r.id == id))
            .unwrap_or(0);
        self.brands = datos(studio).marcas();
        self.brand = self.brand.min(self.brands.len());
    }

    /// The sheet as the project's last montage was asked for.
    fn fill(&mut self, encargo: &Encargo) {
        if !encargo.material.as_os_str().is_empty() {
            self.set_material(encargo.material.to_string_lossy().into_owned());
        }
        let pedida = if encargo.pedida.is_empty() {
            &encargo.receta.id
        } else {
            &encargo.pedida
        };
        if let Some(index) = self.recipes.iter().position(|r| &r.id == pedida) {
            self.recipe = index;
        }
        self.request = encargo.peticion.clone();
        if !encargo.formatos.is_empty() {
            let has = |frame: &str| encargo.formatos.iter().any(|f| f == frame);
            self.vertical = has("vertical");
            self.wide = has("16x9");
            self.square = has("1x1");
        }
        self.brand = encargo
            .marca
            .as_ref()
            .and_then(|marca| self.brands.iter().position(|b| b == marca))
            .map_or(0, |i| i + 1);
    }

    fn set_material(&mut self, material: String) {
        self.material_note = material_note(&material);
        self.material = material;
    }

    /// The recipe's own frames, when it is picked; its captions are what
    /// "as the kind has it" means.
    fn from_recipe(&mut self) {
        let Some(recipe) = self.recipes.get(self.recipe) else {
            return;
        };
        self.captions = 0;
        if recipe.es_automatica() {
            return;
        }
        let frames: Vec<&str> = std::iter::once(recipe.formato.as_str())
            .chain(recipe.formatos_extra.iter().map(String::as_str))
            .collect();
        self.vertical = frames.contains(&"vertical");
        self.wide = frames.contains(&"16x9");
        self.square = frames.contains(&"1x1");
    }

    fn action(&mut self, name: &str, studio: &mut Studio) {
        match name {
            "browse-material" => pick(
                "material",
                t("montaje.whereIsMaterial"),
                self.material.clone(),
            ),
            "browse-script" => {
                crate::host::after_dialog(
                    || {
                        platform::pick_files(
                            &t("montaje.chooseScript"),
                            Some(("Guion", &DOCUMENT[..])),
                        )
                        .and_then(|files| files.into_iter().next())
                    },
                    |studio, _, _, file| {
                        studio.handle(Msg::Montaje(MontajeMsg::Chosen("script".into(), file)));
                    },
                );
            }
            "clear-script" => self.script.clear(),
            "settings" => {
                self.open = false;
                studio.handle(Msg::Settings(crate::panes::settings::SettingsMsg::Open));
                studio.handle(Msg::Settings(
                    crate::panes::settings::SettingsMsg::PageChanged(SETTINGS_PAGE),
                ));
            }
            "back" => {
                self.error.clear();
                self.step = (self.step - 1).clamp(0, 2);
            }
            "next" => {
                self.error.clear();
                if self.step == 0 {
                    if let Err(error) = self.check_material() {
                        self.error = error;
                        return;
                    }
                }
                self.step = (self.step + 1).min(2);
            }
            "run" => self.run(studio),
            "cancel" => {
                self.message = t("montaje.cancelling");
                self.parada.cancelar();
            }
            "dismiss" => self.error.clear(),
            _ => {}
        }
    }

    /// Whether the material folder is there and has footage in it.
    fn check_material(&self) -> Result<(), String> {
        let folder = Path::new(self.material.trim());
        if !folder.is_dir() {
            return Err(t("montaje.materialMissing"));
        }
        if contar(folder).0 == 0 {
            return Err(t("montaje.materialNoVideo"));
        }
        Ok(())
    }

    /// Whether a montage could start: the material, and the whisper
    /// models.
    fn ready(&self, studio: &Studio) -> Result<(), String> {
        self.check_material()?;
        self.models_missing(studio).map_or(Ok(()), Err)
    }

    /// What to say when a whisper model the language asked for needs is
    /// not downloaded.
    fn models_missing(&self, studio: &Studio) -> Option<String> {
        let faltan =
            montaje_analisis::faltan(&studio.host.dirs, &studio.prefs.montaje.ia, self.idioma());
        (!faltan.is_empty()).then(|| tf("montaje.modelsMissing", &[&faltan.join(", ")]))
    }

    /// The language asked for: "es", "eu", or None to let whisper tell.
    fn idioma(&self) -> Option<&'static str> {
        match self.language {
            1 => Some("es"),
            2 => Some("eu"),
            _ => None,
        }
    }

    /// The recipe asked for in the frames and captions asked for, the
    /// recipes an automatic one chooses among likewise, and the frames.
    fn asked(&self) -> (Receta, Vec<Receta>, Vec<String>) {
        let mut frames: Vec<String> = [
            (self.vertical, "vertical"),
            (self.wide, "16x9"),
            (self.square, "1x1"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, frame)| frame.to_owned())
        .collect();
        // No frame ticked is the usual one.
        if frames.is_empty() {
            frames.push("vertical".to_owned());
        }
        let modo = match self.captions {
            1 => Some(ModoSubtitulos::Documento),
            2 => Some(ModoSubtitulos::Voz),
            3 => Some(ModoSubtitulos::Claves),
            4 => Some(ModoSubtitulos::Ninguno),
            _ => None,
        };
        let fit = |mut recipe: Receta| {
            recipe.formato = frames[0].clone();
            recipe.formatos_extra = frames[1..].to_vec();
            if let Some(modo) = modo {
                recipe.subtitulos = modo;
            }
            recipe
        };
        let recipe = fit(self
            .recipes
            .get(self.recipe)
            .cloned()
            .unwrap_or_else(receta::automatica));
        let candidates = if recipe.es_automatica() {
            self.recipes
                .iter()
                .filter(|r| !r.es_automatica())
                .cloned()
                .map(fit)
                .collect()
        } else {
            Vec::new()
        };
        (recipe, candidates, frames)
    }

    /// Starts the montage of the open project. The sheet comes down; the
    /// Montaje pane shows how it goes.
    fn run(&mut self, studio: &mut Studio) {
        if self.busy {
            return;
        }
        let Some(proyecto) = open_project(studio) else {
            return;
        };
        self.error.clear();
        if let Err(error) = self.ready(studio) {
            self.error = error;
            return;
        }
        let (recipe, candidatas, frames) = self.asked();
        let name = proyecto
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "montaje".to_owned());
        let pedido = Pedido {
            material: PathBuf::from(self.material.trim()),
            id: pedido::id_trabajo(&name, &pedido::ahora()),
            receta: recipe,
            peticion: self.request.clone(),
            guion: Some(PathBuf::from(self.script.trim())).filter(|p| !p.as_os_str().is_empty()),
            idioma: self.idioma().map(str::to_owned),
            marca: self
                .brand
                .checked_sub(1)
                .and_then(|i| self.brands.get(i))
                .cloned(),
            cerebro: studio.prefs.montaje.ia.cerebro.clone(),
            candidatas,
        };
        log::info!("montaje: {} from {}", pedido.id, pedido.material.display());
        self.open = false;
        self.busy = true;
        self.running_in = Some(proyecto.clone());
        self.phase = 0;
        self.progress = 0.0;
        self.message = t("montaje.starting");
        self.parada = Parada::default();
        if !studio.dock.contains(PaneKind::Guion) {
            studio.dock.replace(PaneKind::Media, PaneKind::Guion);
        }
        let corrida = Corrida {
            datos: datos(studio),
            env_viejo: env_viejo(studio),
            dirs: studio.host.dirs.clone(),
            ia: studio.prefs.montaje.ia.clone(),
            parada: self.parada.clone(),
            // A redo is the next version of the project's montage.
            version: Encargo::leer(&proyecto).map_or(0, |e| e.version),
            proyecto,
            formatos: frames,
        };
        crate::host::spawn(
            move || {
                let avisar = |avance| {
                    crate::host::on_ui(move |studio, _, _| {
                        studio.handle(Msg::Montaje(MontajeMsg::Progress(avance)));
                    });
                };
                let listo = || {
                    crate::host::on_ui(|studio, _, _| studio.handle(Msg::Montaje(MontajeMsg::Laying)));
                };
                corrida.ejecutar(&pedido, avisar, listo)
            },
            |studio, _, _, result| studio.handle(Msg::Montaje(MontajeMsg::Done(result))),
        );
    }

    /// The run ended: the montage joins the project it was made for.
    fn done(&mut self, result: Result<(Encargo, Project), String>, studio: &mut Studio) {
        self.busy = false;
        let running_in = self.running_in.take();
        let (mut encargo, nuevo) = match result {
            Ok(done) => done,
            // Cancelled: nothing to say.
            Err(error) if error.is_empty() => {
                self.message.clear();
                return;
            }
            Err(error) => {
                self.error = error;
                studio.notify(&t("montaje.failed"), true);
                return;
            }
        };
        // The project the montage was for was closed while it ran: it is
        // not put into another one. The job stays in the montage's folder.
        if running_in.is_none() || open_project(studio) != running_in {
            self.error = tf("montaje.projectClosed", &[&encargo.trabajo.display()]);
            studio.notify(&t("montaje.failed"), true);
            return;
        }
        match crate::panes::guion::graft(studio, &mut encargo, nuevo) {
            Ok(()) => {
                self.message.clear();
                studio.notify(&tf("montaje.done", &[&encargo.receta.nombre]), false);
            }
            Err(error) => {
                self.error = error;
                studio.notify(&t("montaje.failed"), true);
            }
        }
    }

    /// The wizard as the window draws it.
    pub fn data(&self, studio: &Studio) -> MontajeData {
        // Down, there is nothing to draw.
        if !self.open {
            return MontajeData::default();
        }
        let models_missing = self.models_missing(studio);
        let mut brands = vec![SharedString::from(t("montaje.noBrand"))];
        brands.extend(self.brands.iter().map(|b| SharedString::from(b.as_str())));
        MontajeData {
            open: self.open,
            redo: self.redo,
            step: self.step,
            material: self.material.as_str().into(),
            material_note: if self.material.trim().is_empty() {
                t("montaje.materialHint")
            } else {
                self.material_note.clone()
            }
            .into(),
            recipe: self.recipe as i32,
            recipes: strings(self.recipes.iter().map(|r| r.nombre.as_str())),
            recipe_note: self
                .recipes
                .get(self.recipe)
                .map(|r| r.descripcion.as_str())
                .unwrap_or("")
                .into(),
            request: self.request.as_str().into(),
            vertical: self.vertical,
            wide: self.wide,
            square: self.square,
            brand: self.brand as i32,
            brands: ModelRc::new(VecModel::from(brands)),
            language: self.language,
            captions: self.captions,
            script: self.script.as_str().into(),
            models_ok: models_missing.is_none(),
            models_note: models_missing.unwrap_or_default().into(),
            busy: self.busy,
            error: self.error.as_str().into(),
        }
    }

    /// The run, as the Montaje pane shows it.
    pub fn publish_run(&self, app: &App) {
        let panel = app.global::<GuionPanel>();
        panel.set_running(self.busy);
        panel.set_phase(self.phase);
        panel.set_progress(self.progress);
        panel.set_message(self.message.as_str().into());
        // A run's error belongs to the pane while the sheet is down.
        panel.set_error(if self.open { "" } else { self.error.as_str() }.into());
    }

    /// The Settings page as the window draws it.
    pub fn settings_data(&self, studio: &Studio) -> MontajeSettingsData {
        if !studio.settings.open || studio.settings.tab != SETTINGS_PAGE {
            return MontajeSettingsData::default();
        }
        let mut data = MontajeSettingsData {
            data_folder: datos(studio).carpeta.to_string_lossy().as_ref().into(),
            recipes: recipes_folder(studio).to_string_lossy().as_ref().into(),
            ..MontajeSettingsData::default()
        };
        self.ajustes.publicar(studio, &mut data);
        data
    }
}

/// Shows `folder` in the system's file browser, made first if need be.
fn reveal(studio: &mut Studio, folder: &Path) {
    let _ = std::fs::create_dir_all(folder);
    if let Err(error) = platform::reveal(&folder.to_string_lossy()) {
        studio.notify(&error, true);
    }
}

/// A folder picker on its own turn, answered as [`MontajeMsg::Chosen`].
fn pick(field: &'static str, title: String, current: String) {
    crate::host::after_dialog(
        move || platform::pick_folder(&title, &current),
        move |studio, _, _, folder| {
            studio.handle(Msg::Montaje(MontajeMsg::Chosen(field.into(), folder)));
        },
    );
}

fn strings<'a>(items: impl Iterator<Item = &'a str>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.map(SharedString::from).collect::<Vec<_>>(),
    ))
}

/// The file kinds the material is read for: videos, photos, audio,
/// documents.
const VIDEO: [&str; 5] = ["mov", "mp4", "m4v", "mkv", "avi"];
const IMAGE: [&str; 5] = ["jpg", "jpeg", "png", "webp", "heic"];
const AUDIO: [&str; 6] = ["mp3", "m4a", "wav", "aac", "ogg", "flac"];
const DOCUMENT: [&str; 3] = ["docx", "txt", "md"];

/// How many videos, photos, audio files and documents a folder holds,
/// subfolders included, as the listing reads it.
fn contar(folder: &Path) -> (usize, usize, usize, usize) {
    let mut cuenta = (0, 0, 0, 0);
    let mut pendientes = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = pendientes.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 {
                    pendientes.push((path, depth + 1));
                }
                continue;
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let ext = ext.as_str();
            if VIDEO.contains(&ext) {
                cuenta.0 += 1;
            } else if IMAGE.contains(&ext) {
                cuenta.1 += 1;
            } else if AUDIO.contains(&ext) {
                cuenta.2 += 1;
            } else if DOCUMENT.contains(&ext) {
                cuenta.3 += 1;
            }
        }
    }
    cuenta
}

/// The line under the material field.
fn material_note(material: &str) -> String {
    let folder = Path::new(material.trim());
    if material.trim().is_empty() {
        return String::new();
    }
    if !folder.is_dir() {
        return t("montaje.materialMissing");
    }
    let (videos, images, audio, docs) = contar(folder);
    tf(
        "montaje.materialCount",
        &[
            &videos.to_string(),
            &images.to_string(),
            &audio.to_string(),
            &docs.to_string(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opened again over the project it was filled in for, the sheet keeps
    /// its edits; over another project it is filled in afresh.
    #[test]
    fn the_sheet_keeps_its_edits_for_the_same_project() {
        let pane = MontajePane {
            filled_for: Some(PathBuf::from("/proyectos/uno")),
            ..MontajePane::default()
        };
        assert!(pane.keeps_edits_for(Path::new("/proyectos/uno")));
        assert!(!pane.keeps_edits_for(Path::new("/proyectos/dos")));
        assert!(!MontajePane::default().keeps_edits_for(Path::new("/proyectos/uno")));
    }
}
