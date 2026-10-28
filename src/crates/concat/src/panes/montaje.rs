// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Komunta's "Nuevo montaje" sheet, and the Settings page that says where
//! the montage engine is.
//!
//! Three steps - the material, what the video is for, the captions and
//! the script - then the engine runs on a worker: the Python engine reads,
//! transcribes and asks the AI for the plan ([`montaje::motor`]), and the
//! `montaje` crate lays the plan out as a project, which opens. Its
//! progress comes back as [`MontajeMsg::Progress`], one line at a time.

use std::path::{Path, PathBuf};

use concat_host::projects;
use montaje::motor::{self, Avance, FalloMotor, Parada};
use montaje::receta::{self, ModoSubtitulos, Receta};
use montaje::{Encargo, Motor, Pedido};
use slint::{ModelRc, SharedString, VecModel};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::platform;
use crate::studio::{Studio, home_folder};
use crate::ui::{MontajeData, MontajeSettingsData};

/// The AIs the engine can plan with, as its `--ia` names them; the first,
/// empty, is whichever the engine's own settings choose.
pub const CEREBROS: [&str; 8] = [
    "",
    "claude_code",
    "anthropic",
    "google",
    "openai",
    "codex",
    "gemini_cli",
    "manual",
];

/// The Settings page the engine's folder is set on.
pub const SETTINGS_PAGE: i32 = 8;

/// Everything that can happen to the sheet and its Settings page.
#[derive(Debug)]
pub enum MontajeMsg {
    Open,
    Close,
    /// A text field, by name, and what it holds now.
    TextEdited(String, String),
    /// A choice, by name, and its index (0 or 1 for a switch).
    Choice(String, i32),
    /// A button, by name.
    Action(String),
    /// A picker answered: which field, and the path.
    Chosen(String, PathBuf),
    /// The engine reported a step.
    Progress(Avance),
    /// The plan is in; the project is being laid out.
    Laying,
    /// The run ended: the project folder, or why not. An empty error is a
    /// run that was cancelled.
    Done(Result<PathBuf, String>),
    /// The Settings page: a field changed.
    SettingEdited(String, String),
    /// The Settings page: the AI changed.
    SettingChoice(String, i32),
    /// The Settings page: a button.
    SettingAction(String),
}

/// The sheet's state.
pub struct MontajePane {
    pub open: bool,
    step: i32,
    name: String,
    material: String,
    location: String,
    recipes: Vec<Receta>,
    recipe: usize,
    request: String,
    vertical: bool,
    wide: bool,
    square: bool,
    /// The engine's kits; the sheet lists "no brand" before them.
    brands: Vec<String>,
    /// 0 for no brand, otherwise one past the index into `brands`.
    brand: usize,
    language: i32,
    captions: i32,
    script: String,
    busy: bool,
    phase: i32,
    progress: f32,
    message: String,
    error: String,
    parada: Parada,
}

impl Default for MontajePane {
    fn default() -> Self {
        Self {
            open: false,
            step: 0,
            name: String::new(),
            material: String::new(),
            location: String::new(),
            recipes: receta::de_serie(),
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
            phase: 0,
            progress: 0.0,
            message: String::new(),
            error: String::new(),
            parada: Parada::default(),
        }
    }
}

/// The engine as Settings has it, or where it usually is.
pub fn motor(studio: &Studio) -> Motor {
    let prefs = &studio.prefs.montaje;
    let carpeta = prefs
        .motor
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| std::env::var("KOMUNTA_MOTOR").ok())
        .unwrap_or_else(|| home_folder("Desktop/apps/automatizacion_videos"));
    Motor {
        carpeta: PathBuf::from(carpeta),
        python: prefs.python.clone().unwrap_or_default(),
    }
}

/// The team's own recipes live beside the app's settings.
fn recipes_folder(studio: &Studio) -> PathBuf {
    studio.host.dirs.config.join("recetas")
}

impl MontajePane {
    /// Applies one message.
    pub fn update(&mut self, msg: MontajeMsg, studio: &mut Studio) {
        match msg {
            MontajeMsg::Open => {
                self.open = true;
                if !self.busy {
                    self.step = 0;
                    self.error.clear();
                }
                self.recipes = receta::cargar(&recipes_folder(studio));
                self.recipe = self.recipe.min(self.recipes.len().saturating_sub(1));
                self.brands = motor(studio).marcas();
                self.brand = self.brand.min(self.brands.len());
                if self.location.is_empty() {
                    self.location = studio.start.location.clone();
                }
            }
            // A run keeps going with the sheet down; it opens the project
            // when it is done.
            MontajeMsg::Close => self.open = false,
            MontajeMsg::TextEdited(field, value) => match field.as_str() {
                "name" => self.name = value,
                "material" => self.material = value,
                "location" => self.location = value,
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
                    "captions" => self.captions = value.clamp(0, 3),
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
                    "material" => {
                        if self.name.trim().is_empty() {
                            self.name = Path::new(&path)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                        }
                        self.material = path;
                    }
                    "location" => self.location = path,
                    "script" => self.script = path,
                    "engine" => {
                        studio.prefs.montaje.motor = Some(path);
                        studio.prefs.save(&studio.host.dirs);
                        self.brands = motor(studio).marcas();
                    }
                    _ => {}
                }
            }
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
            MontajeMsg::Done(result) => {
                self.busy = false;
                match result {
                    Ok(folder) => {
                        self.open = false;
                        self.step = 0;
                        self.name.clear();
                        self.request.clear();
                        self.script.clear();
                        let path = folder.to_string_lossy().into_owned();
                        if let Err(error) =
                            projects::open(&path).and_then(|info| studio.open_project(info))
                        {
                            studio.notify(&error, true);
                        }
                    }
                    // Cancelled: back to the last step, as it was.
                    Err(error) if error.is_empty() => self.step = 2,
                    Err(error) => {
                        self.error = error;
                        // Said on the toast too, for a sheet put away
                        // while it ran.
                        if !self.open {
                            studio.notify(&t("montaje.failed"), true);
                        }
                    }
                }
            }
            MontajeMsg::SettingEdited(field, value) => {
                let value = Some(value).filter(|v| !v.trim().is_empty());
                match field.as_str() {
                    "engine" => studio.prefs.montaje.motor = value,
                    "python" => studio.prefs.montaje.python = value,
                    _ => return,
                }
                studio.prefs.save(&studio.host.dirs);
            }
            MontajeMsg::SettingChoice(field, value) => {
                if field == "brain" {
                    let cerebro = CEREBROS.get(value.max(0) as usize).copied().unwrap_or("");
                    studio.prefs.montaje.cerebro =
                        Some(cerebro.to_owned()).filter(|c| !c.is_empty());
                    studio.prefs.save(&studio.host.dirs);
                }
            }
            MontajeMsg::SettingAction(name) => {
                if name == "browse-engine" {
                    let current = motor(studio).carpeta.to_string_lossy().into_owned();
                    pick("engine", t("montaje.whereIsEngine"), current);
                } else if name == "recipes" {
                    let folder = recipes_folder(studio);
                    let _ = std::fs::create_dir_all(&folder);
                    if let Err(error) = platform::reveal(&folder.to_string_lossy()) {
                        studio.notify(&error, true);
                    }
                }
            }
        }
    }

    /// The recipe's own frames and captions, when it is picked.
    fn from_recipe(&mut self) {
        let Some(recipe) = self.recipes.get(self.recipe) else {
            return;
        };
        let frames: Vec<&str> = std::iter::once(recipe.formato.as_str())
            .chain(recipe.formatos_extra.iter().map(String::as_str))
            .collect();
        self.vertical = frames.contains(&"vertical");
        self.wide = frames.contains(&"16x9");
        self.square = frames.contains(&"1x1");
        self.captions = match recipe.subtitulos {
            ModoSubtitulos::Documento => 0,
            ModoSubtitulos::Voz => 1,
            ModoSubtitulos::Claves => 2,
            ModoSubtitulos::Ninguno => 3,
        };
    }

    fn action(&mut self, name: &str, studio: &mut Studio) {
        match name {
            "browse-material" => pick(
                "material",
                t("montaje.whereIsMaterial"),
                self.material.clone(),
            ),
            "browse-location" => pick(
                "location",
                t("start.whereShouldProjectFolder"),
                self.location.clone(),
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
            "create" => self.create(studio),
            "cancel" => {
                self.message = t("montaje.cancelling");
                self.parada.cancelar();
            }
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

    fn create(&mut self, studio: &mut Studio) {
        self.error.clear();
        if let Err(error) = self.check_material() {
            self.error = error;
            return;
        }
        let engine = motor(studio);
        if !engine.valido() {
            self.error = t("montaje.engineMissing");
            return;
        }
        let name = self.name.trim().to_owned();
        if name.is_empty() {
            self.step = 0;
            self.error = t("montaje.nameMissing");
            return;
        }
        let folder = Path::new(self.location.trim()).join(projects::folder_name(&name));
        if folder.exists() {
            self.error = tf("montaje.folderExists", &[&folder.to_string_lossy()]);
            return;
        }
        let Some(mut recipe) = self.recipes.get(self.recipe).cloned() else {
            return;
        };
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
        let Some((main, extra)) = frames.split_first() else {
            return;
        };
        recipe.formato = main.clone();
        recipe.formatos_extra = extra.to_vec();
        recipe.subtitulos = match self.captions {
            1 => ModoSubtitulos::Voz,
            2 => ModoSubtitulos::Claves,
            3 => ModoSubtitulos::Ninguno,
            _ => ModoSubtitulos::Documento,
        };
        let pedido = Pedido {
            material: PathBuf::from(self.material.trim()),
            id: motor::id_trabajo(&name, &motor::ahora()),
            receta: recipe.clone(),
            peticion: self.request.clone(),
            guion: Some(PathBuf::from(self.script.trim())).filter(|p| !p.as_os_str().is_empty()),
            idioma: match self.language {
                1 => Some("es".to_owned()),
                2 => Some("eu".to_owned()),
                _ => None,
            },
            marca: self
                .brand
                .checked_sub(1)
                .and_then(|i| self.brands.get(i))
                .cloned(),
            cerebro: studio.prefs.montaje.cerebro.clone(),
        };
        log::info!("montaje: {} from {}", pedido.id, pedido.material.display());
        self.busy = true;
        self.step = 3;
        self.phase = 0;
        self.progress = 0.0;
        self.message = t("montaje.starting");
        self.parada = Parada::default();
        let parada = self.parada.clone();
        crate::host::spawn(
            move || -> Result<PathBuf, String> {
                let trabajo = motor::analizar(&engine, &pedido, &parada, |avance| {
                    crate::host::on_ui(move |studio, _, _| {
                        studio.handle(Msg::Montaje(MontajeMsg::Progress(avance)));
                    });
                })
                .map_err(|error| match error {
                    FalloMotor::Cancelado => String::new(),
                    error => error.to_string(),
                })?;
                crate::host::on_ui(|studio, _, _| studio.handle(Msg::Montaje(MontajeMsg::Laying)));
                let mut encargo = Encargo {
                    trabajo,
                    motor: engine.carpeta.clone(),
                    receta: recipe,
                    marca: pedido.marca.clone(),
                    formatos: frames,
                    version: 0,
                };
                encargo
                    .crear(&folder, &name)
                    .map_err(|error| error.to_string())?;
                Ok(folder)
            },
            |studio, _, _, result| studio.handle(Msg::Montaje(MontajeMsg::Done(result))),
        );
    }

    /// The sheet as the window draws it.
    pub fn data(&self, studio: &Studio) -> MontajeData {
        // Closed, there is nothing to draw, and the folder is not walked
        // on every publish.
        if !self.open {
            return MontajeData::default();
        }
        let engine = motor(studio);
        let engine_ok = engine.valido();
        let brain = studio
            .prefs
            .montaje
            .cerebro
            .clone()
            .unwrap_or_else(|| t("montaje.brainEngine"));
        let mut brands = vec![SharedString::from(t("montaje.noBrand"))];
        brands.extend(self.brands.iter().map(|b| SharedString::from(b.as_str())));
        MontajeData {
            open: self.open,
            step: self.step,
            name: self.name.as_str().into(),
            material: self.material.as_str().into(),
            material_note: material_note(&self.material).into(),
            location: self.location.as_str().into(),
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
            engine_ok,
            engine_note: if engine_ok {
                tf("montaje.engineAi", &[&brain])
            } else {
                tf(
                    "montaje.engineNotFound",
                    &[&engine.carpeta.to_string_lossy()],
                )
            }
            .into(),
            busy: self.busy,
            phase: self.phase,
            progress: self.progress,
            message: self.message.as_str().into(),
            error: self.error.as_str().into(),
        }
    }

    /// The Settings page as the window draws it.
    pub fn settings_data(&self, studio: &Studio) -> MontajeSettingsData {
        if !studio.settings.open || studio.settings.tab != SETTINGS_PAGE {
            return MontajeSettingsData::default();
        }
        let engine = motor(studio);
        let prefs = &studio.prefs.montaje;
        let brain = prefs.cerebro.as_deref().unwrap_or("");
        MontajeSettingsData {
            engine: engine.carpeta.to_string_lossy().as_ref().into(),
            engine_ok: engine.valido(),
            python: prefs.python.clone().unwrap_or_default().into(),
            python_note: tf(
                "montaje.pythonUsed",
                &[&engine.interprete().to_string_lossy()],
            )
            .into(),
            brain: CEREBROS.iter().position(|c| *c == brain).unwrap_or(0) as i32,
            brains: strings(CEREBROS.iter().map(|c| if c.is_empty() { "—" } else { *c })),
            recipes: recipes_folder(studio).to_string_lossy().as_ref().into(),
        }
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

/// The engine's own file kinds: videos, photos, audio, documents.
const VIDEO: [&str; 5] = ["mov", "mp4", "m4v", "mkv", "avi"];
const IMAGE: [&str; 5] = ["jpg", "jpeg", "png", "webp", "heic"];
const AUDIO: [&str; 6] = ["mp3", "m4a", "wav", "aac", "ogg", "flac"];
const DOCUMENT: [&str; 3] = ["docx", "txt", "md"];

/// How many videos, photos, audio files and documents a folder holds,
/// subfolders included, as the engine reads it.
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
        return t("montaje.materialHint");
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
