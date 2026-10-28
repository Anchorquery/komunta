// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Komunta's "Guion de montaje" pane: the plan of a montage project, as a
//! script to edit, and "Aplicar al vídeo".
//!
//! A project made by "Nuevo montaje" carries `montaje.json`, which names
//! the engine's job; the plan is read from there when the project opens.
//! Edits change the plan in memory ([`PlanCrudo`]) and the script is
//! rebuilt from it, so what the pane shows is always what would be laid
//! out. Applying writes the plan back to the job, as a new version, lays it
//! out on a worker, and adds the result to the project as new timelines -
//! the timelines already there, hand edits and all, stay as they are.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use concat_host::projects;
use concat_project::Project;
use montaje::guion::{self, Tipo};
use montaje::{Encargo, Guion, PlanCrudo, Trabajo};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;
use crate::ui::{App, GuionBlockData, GuionChipData, GuionPanel, GuionRowData, GuionWordData};

/// Everything that can happen to the script.
#[derive(Debug)]
pub enum GuionMsg {
    /// The pane is about this many characters wide.
    Resized(i32),
    /// A word of a take cut, or put back: the take, the word.
    WordToggled(i32, i32),
    TakeRemoved(i32),
    /// A take moved up (-1) or down (1).
    TakeMoved(i32, i32),
    /// A caption's words, as typed, by its index in the plan.
    CaptionEdited(i32, String),
    CaptionRemoved(i32),
    ResourceRemoved(i32),
    /// Back to the plan as it was last applied.
    Revert,
    Apply,
    /// The plan laid out on its own, ready to add to the project.
    Prepared(Result<Project, String>),
}

/// The script's state.
pub struct GuionPane {
    /// The project folder, when it is a montage.
    proyecto: Option<PathBuf>,
    encargo: Option<Encargo>,
    /// The job as read; its plan is the edited one.
    trabajo: Option<Trabajo>,
    plan: Option<PlanCrudo>,
    guion: Guion,
    columns: usize,
    dirty: bool,
    busy: bool,
    status: String,
    /// Bumped whenever the takes must be drawn again. A caption being typed
    /// does not bump it: a new model would rebuild the field under the
    /// caret.
    revision: u64,
    published: Cell<u64>,
}

impl Default for GuionPane {
    fn default() -> Self {
        Self {
            proyecto: None,
            encargo: None,
            trabajo: None,
            plan: None,
            guion: Guion::default(),
            columns: 40,
            dirty: false,
            busy: false,
            status: String::new(),
            revision: 1,
            published: Cell::new(0),
        }
    }
}

impl GuionPane {
    /// Reads the plan of the project in `proyecto`, if it is a montage;
    /// otherwise the pane says it is not one.
    pub fn load(&mut self, proyecto: &Path) {
        let columns = self.columns;
        *self = Self {
            columns,
            ..Self::default()
        };
        let Some(encargo) = Encargo::leer(proyecto) else {
            return;
        };
        self.proyecto = Some(proyecto.to_path_buf());
        match Trabajo::leer(&encargo.trabajo)
            .and_then(|trabajo| Ok((PlanCrudo::leer(&encargo.trabajo)?, trabajo)))
        {
            Ok((plan, trabajo)) => {
                self.plan = Some(plan);
                self.trabajo = Some(trabajo);
                self.encargo = Some(encargo);
                self.rebuild();
            }
            Err(error) => {
                log::warn!("guion: {error}");
                self.status = tf("guion.jobMissing", &[&encargo.trabajo.display()]);
                self.encargo = Some(encargo);
            }
        }
    }

    /// Whether the open project is a montage with its plan at hand.
    pub fn linked(&self) -> bool {
        self.plan.is_some()
    }

    /// The script again, from the plan as edited.
    fn rebuild(&mut self) {
        let (Some(plan), Some(trabajo)) = (self.plan.as_ref(), self.trabajo.as_mut()) else {
            return;
        };
        match plan.plan() {
            Ok(nuevo) => {
                trabajo.plan = nuevo;
                self.guion = Guion::de(trabajo);
            }
            Err(error) => self.status = error.to_string(),
        }
        self.revision += 1;
    }

    /// Applies one message.
    pub fn update(&mut self, msg: GuionMsg, studio: &mut Studio) {
        let index = |i: i32| i.max(0) as usize;
        match msg {
            GuionMsg::Resized(columns) => {
                let columns = index(columns).max(16);
                if columns != self.columns {
                    self.columns = columns;
                    self.revision += 1;
                }
                return;
            }
            GuionMsg::Apply => return self.apply(studio),
            GuionMsg::Prepared(result) => return self.prepared(result, studio),
            GuionMsg::Revert => {
                if let Some(proyecto) = self.proyecto.clone() {
                    self.load(&proyecto);
                }
                return;
            }
            _ => {}
        }
        if self.busy {
            return;
        }
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        self.status.clear();
        self.dirty = true;
        match msg {
            GuionMsg::WordToggled(take, word) => plan.alternar_palabra(index(take), index(word)),
            GuionMsg::TakeRemoved(take) => plan.quitar_toma(index(take)),
            GuionMsg::TakeMoved(take, by) => plan.mover_toma(index(take), by),
            GuionMsg::CaptionEdited(k, text) => {
                // The words the field shows are already these; only the
                // plan and the script behind it change, not what is drawn.
                plan.cambiar_subtitulo(index(k), &text);
                for sub in self
                    .guion
                    .bloques
                    .iter_mut()
                    .flat_map(|b| &mut b.subtitulos)
                {
                    if sub.indice == index(k) {
                        sub.texto = text.clone();
                    }
                }
                return;
            }
            GuionMsg::CaptionRemoved(k) => plan.quitar_subtitulo(index(k)),
            GuionMsg::ResourceRemoved(k) => plan.quitar_recurso(index(k)),
            _ => return,
        }
        self.rebuild();
    }

    /// Writes the plan back to the job, then lays it out on a worker.
    fn apply(&mut self, studio: &mut Studio) {
        let (Some(proyecto), Some(encargo), Some(plan)) = (
            self.proyecto.clone(),
            self.encargo.clone(),
            self.plan.as_ref(),
        ) else {
            return;
        };
        if self.busy {
            return;
        }
        if self.dirty {
            if let Err(error) = plan.guardar(&encargo.trabajo, &t("guion.editedNote")) {
                self.status = error.to_string();
                studio.notify(&self.status, true);
                return;
            }
        }
        self.busy = true;
        self.status = t("guion.applying");
        crate::host::spawn_in_project(
            move || {
                encargo
                    .preparar(&proyecto)
                    .map_err(|error| error.to_string())
            },
            |studio, _, _, result| studio.handle(Msg::Guion(GuionMsg::Prepared(result))),
        );
    }

    /// The layout is ready: it joins the project as it stands now, which is
    /// saved and opened again with the new timelines in front.
    fn prepared(&mut self, result: Result<Project, String>, studio: &mut Studio) {
        self.busy = false;
        let nuevo = match result {
            Ok(nuevo) => nuevo,
            Err(error) => {
                self.status = error;
                studio.notify(&t("guion.applyFailed"), true);
                return;
            }
        };
        let (Some(proyecto), Some(mut encargo)) = (self.proyecto.clone(), self.encargo.clone())
        else {
            return;
        };
        studio.flush_commit();
        studio.pause();
        let Some(session) = studio.session.as_mut() else {
            return;
        };
        let (path, documento) = session.prepare_save(None);
        let written = encargo
            .injertar(&documento, nuevo)
            .map_err(|error| error.to_string())
            .and_then(|documento| {
                projects::save_ordered(&path, &documento, projects::ticket())?;
                encargo
                    .guardar(&proyecto)
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = written {
            self.status = error;
            studio.notify(&t("guion.applyFailed"), true);
            return;
        }
        // What was on disk is the project with the new timelines; the
        // session is let go without saving over it, and opened again.
        studio.release_project();
        if let Err(error) = projects::open(&path).and_then(|info| studio.open_project(info)) {
            studio.notify(&error, true);
            return;
        }
        // Opening loaded the script into the studio's pane, which this one
        // is about to be put back over; so this one loads it too.
        self.load(&proyecto);
        self.status = tf("guion.applied", &[&encargo.version]);
        studio.notify(&self.status, false);
    }

    /// Hands the script to the window. The takes only when they changed.
    pub fn publish(&self, app: &App) {
        let panel = app.global::<GuionPanel>();
        panel.set_linked(self.linked());
        panel.set_dirty(self.dirty);
        panel.set_busy(self.busy);
        panel.set_status(self.status.as_str().into());
        if self.published.get() == self.revision {
            return;
        }
        self.published.set(self.revision);
        let version = self.encargo.as_ref().map(|e| e.version).unwrap_or(0);
        panel.set_summary(
            tf(
                "guion.summary",
                &[
                    &self.guion.bloques.len(),
                    &clock(self.guion.duracion()),
                    &version,
                ],
            )
            .into(),
        );
        let total = self.guion.bloques.len();
        let blocks: Vec<GuionBlockData> = self
            .guion
            .bloques
            .iter()
            .enumerate()
            .map(|(n, bloque)| {
                let take = bloque.toma as i32;
                let rows: Vec<GuionRowData> = guion::filas(&bloque.palabras, self.columns)
                    .into_iter()
                    .map(|range| GuionRowData {
                        words: model(
                            bloque.palabras[range]
                                .iter()
                                .map(|p| GuionWordData {
                                    take,
                                    i: p.i as i32,
                                    text: p.txt.as_str().into(),
                                    cut: p.quitada,
                                    filler: p.coletilla,
                                })
                                .collect(),
                        ),
                    })
                    .collect();
                GuionBlockData {
                    take,
                    clip: bloque.clip.as_str().into(),
                    kind: match bloque.tipo {
                        Tipo::Voz => 0,
                        Tipo::SoloAudio => 1,
                        Tipo::Plano => 2,
                    },
                    duration: clock(bloque.duracion).into(),
                    rows: model(rows),
                    captions: model(
                        bloque
                            .subtitulos
                            .iter()
                            .map(|s| GuionChipData {
                                index: s.indice as i32,
                                text: s.texto.as_str().into(),
                                note: s.estilo.as_str().into(),
                            })
                            .collect(),
                    ),
                    resources: model(
                        bloque
                            .recursos
                            .iter()
                            .map(|r| GuionChipData {
                                index: r.indice as i32,
                                text: r.clip.as_str().into(),
                                note: [r.efecto.as_str(), r.garabato.as_str()]
                                    .iter()
                                    .filter(|s| !s.is_empty())
                                    .copied()
                                    .collect::<Vec<_>>()
                                    .join(" · ")
                                    .into(),
                            })
                            .collect(),
                    ),
                    first: n == 0,
                    last: n + 1 == total,
                }
            })
            .collect();
        panel.set_blocks(model(blocks));
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

/// "0:42", "1:05".
fn clock(seconds: f64) -> SharedString {
    let total = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", total / 60, total % 60).into()
}
