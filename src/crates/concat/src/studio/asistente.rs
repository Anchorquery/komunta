// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Requests answered from the project on screen: live mode (#337), for
//! the in-window agent and for any agent on the window's socket.
//!
//! Both ask in the editor's own API vocabulary ([`Request`]), so a tool
//! means here what it means over MCP; only the project is a different
//! one: the window's, with its undo history, its autosave and its
//! monitor. Every edit is made under the caller's gesture: the in-window
//! agent's turn, or one API call from outside - one Ctrl+Z each.

use concat_api::{ApiError, ErrorCode, Reply, Request};
use concat_host::preview::FrameSpec;
use concat_host::session::EditorView;
use concat_project::Command;
use concat_project::model::Project;
use serde_json::Value;

use super::{Studio, command_name};
use crate::panes::asistente_vivo::Materiales;
use crate::panes::guion::GuionPane;
use crate::panes::montaje::MontajePane;

impl Studio {
    /// Answers one of the agent's requests from the project on screen, as
    /// the API would answer it; edits are moves of `gesto`.
    pub(crate) fn atender_asistente(
        &mut self,
        gesto: &str,
        request: Request,
    ) -> Result<Value, ApiError> {
        let respuesta = self.en_vivo(gesto, request)?;
        serde_json::to_value(respuesta)
            .map_err(|error| ApiError::failed(format!("la respuesta no se pudo leer: {error}")))
    }

    /// One request on the project on screen, its edits moves of `gesto`.
    /// A file to import comes probed already ([`crate::en_vivo::preparar`]):
    /// the probe is not the event loop's to wait for.
    pub(crate) fn en_vivo(&mut self, gesto: &str, request: Request) -> Result<Reply, ApiError> {
        let vista = match request {
            Request::ProjectOpen { .. } | Request::ProjectGet { .. } => self.vista_asistente()?,
            Request::ProjectDocument { .. } => {
                let session = self.session.as_ref().ok_or_else(sin_proyecto)?;
                return Ok(Reply::Document(session.document()));
            }
            Request::ProjectSetVideo { video, .. } => {
                let timeline_id = self.project().active_timeline_id.clone();
                self.editar_asistente(gesto, Command::SetTimelineVideo { timeline_id, video })?
            }
            Request::EditApply { command, .. } => self.editar_asistente(gesto, *command)?,
            Request::EditUndo { .. } => {
                self.undo();
                self.vista_asistente()?
            }
            Request::EditRedo { .. } => {
                self.redo();
                self.vista_asistente()?
            }
            otra => {
                return Err(ApiError::new(
                    ErrorCode::Refused,
                    format!("esto no se hace en vivo sobre el proyecto de la ventana: {otra:?}"),
                ));
            }
        };
        Ok(Reply::View(Box::new(vista)))
    }

    /// The project on screen, for a recipe to read.
    pub(crate) fn proyecto_asistente(&self) -> Result<Project, ApiError> {
        Ok(self.vista_asistente()?.project)
    }

    fn vista_asistente(&self) -> Result<EditorView, ApiError> {
        self.session
            .as_ref()
            .map(|session| session.view())
            .ok_or_else(sin_proyecto)
    }

    /// [`Studio::apply_within`]'s bookkeeping, but a refusal goes back to
    /// the agent to read and correct rather than to a toast.
    pub(crate) fn editar_asistente(
        &mut self,
        gesto: &str,
        command: concat_project::Command,
    ) -> Result<EditorView, ApiError> {
        log::info!("edit: {} (asistente)", command_name(&command));
        self.flush_commit();
        self.echo = None;
        self.last_commit = None;
        let session = self.session.as_mut().ok_or_else(sin_proyecto)?;
        let vista = session
            .apply_within(Some(gesto), command)
            .map_err(|motivo| ApiError::new(ErrorCode::Refused, motivo))?;
        self.after_change();
        Ok(vista)
    }

    /// Runs `trabajo` on the script pane the way a message to it runs:
    /// the pane taken out, so it may ask the studio for more.
    pub(crate) fn guion_asistente<T>(
        &mut self,
        trabajo: impl FnOnce(&mut GuionPane, &mut Studio) -> T,
    ) -> T {
        let mut pane = std::mem::take(&mut self.guion);
        let resultado = trabajo(&mut pane, self);
        self.guion = pane;
        resultado
    }

    /// Runs `trabajo` on the Montaje pane the way a message to it runs.
    pub(crate) fn montaje_asistente<T>(
        &mut self,
        trabajo: impl FnOnce(&mut MontajePane, &mut Studio) -> T,
    ) -> T {
        let mut pane = std::mem::take(&mut self.montaje);
        let resultado = trabajo(&mut pane, self);
        self.montaje = pane;
        resultado
    }

    /// What it takes to paint the timeline's frame at `tiempo`, its
    /// longest side `lado` at most - the painting is the agent thread's,
    /// so the window does not wait for the compositor. The timeline's,
    /// whatever the monitor shows: a library file open there is put aside.
    pub(crate) fn fotograma_asistente(&mut self, tiempo: f64, lado: u32) -> Result<Materiales, ApiError> {
        let duracion = self.project().active().duration();
        if !(0.0..=duracion.max(0.0)).contains(&tiempo) {
            return Err(ApiError::invalid(format!(
                "el segundo {tiempo} está fuera de la línea de tiempo (0–{duracion:.2} s)"
            )));
        }
        let fuente = self.source.take();
        let preparado = self.preview_clips();
        self.source = fuente;
        let (clips, ajustes) = preparado.ok_or_else(sin_proyecto)?;
        let (ancho, alto) = self.output_size();
        let escala = (f64::from(lado) / f64::from(ancho.max(alto))).min(1.0);
        let medida = |n: u32| (((f64::from(n) * escala) / 2.0).round() as u32 * 2).max(2);
        Ok(Materiales {
            clips,
            ajustes,
            monitor: self.host.monitor.clone(),
            spec: FrameSpec {
                time: tiempo,
                width: medida(ancho),
                height: medida(alto),
                moving: false,
                proxy: false,
                color_space: self.project().active().video.color_space,
            },
        })
    }
}

fn sin_proyecto() -> ApiError {
    ApiError::new(ErrorCode::NotOpen, "no hay ningún proyecto abierto en la ventana")
}
