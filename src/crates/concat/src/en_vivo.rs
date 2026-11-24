// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Live mode (#337) as the window serves it: a caller on the socket - an
//! agent through `komunta --mcp window`, a script - edits the project on
//! screen. Each request is carried to the event loop and answered there
//! ([`crate::studio::Studio::en_vivo`]), one undo step per call; a frame
//! is gathered there and painted on the caller's thread; a call that
//! cannot be undone in place waits for the person's yes in the agent's
//! pane, whoever the caller is.
//!
//! The in-window agent goes the same way ([`en_ventana`], [`preparar`]),
//! so both are answered by one code path.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use concat_api::{ApiError, ErrorCode, LiveWindow, Picture, Reply, Request};
use concat_project::Command;

use crate::i18n::tf;
use crate::panes::Msg;
use crate::panes::asistente::{AsistenteMsg, Pregunta};
use crate::panes::asistente_catalogo::QUITAR_SIN_PREGUNTAR;
use crate::studio::Studio;

/// How long a request may wait for the window: an event loop busy with a
/// dialog answers late, a project closed meanwhile never does.
pub const PACIENCIA: Duration = Duration::from_secs(60);

/// How long a call waits for the person's yes before it counts as a no.
const ESPERA_PERSONA: Duration = Duration::from_secs(300);

/// The longest side of a live frame when the caller names no size.
const LADO: u32 = 1920;

/// Runs `trabajo` on the window's event loop and waits for what it gives;
/// with `epoch`, only while that project stays open.
pub fn en_ventana<T: Send + 'static>(
    epoch: Option<u64>,
    trabajo: impl FnOnce(&mut Studio) -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    let (envio, respuesta) = mpsc::channel();
    let cuerpo = move |studio: &mut Studio, _: &_, _: &_| {
        let _ = envio.send(trabajo(studio));
    };
    match epoch {
        Some(epoch) => crate::host::on_ui_in_project(epoch, cuerpo),
        None => crate::host::on_ui(cuerpo),
    }
    respuesta.recv_timeout(PACIENCIA).unwrap_or_else(|_| {
        Err(ApiError::new(
            ErrorCode::NotOpen,
            "la ventana no contestó: el proyecto se cerró o está ocupada",
        ))
    })
}

/// `request` made ready for the event loop: a file to import probed here,
/// and turned into the edit that adds it.
pub fn preparar(request: Request) -> Result<Request, ApiError> {
    match request {
        Request::MediaImport { path, file } => {
            let item = concat_host::media::probe(&file).map_err(ApiError::failed)?.to_new_media();
            Ok(Request::EditApply {
                path,
                command: Box::new(Command::AddMedia { item }),
            })
        }
        otra => Ok(otra),
    }
}

/// Asks the person, in the agent's pane, whether `herramienta` may do
/// what `detalle` says; no answer in five minutes is a no.
pub fn preguntar(herramienta: &str, detalle: String) -> bool {
    let (respuesta, espera) = mpsc::channel();
    let pregunta = Pregunta {
        herramienta: herramienta.to_owned(),
        detalle,
        respuesta,
        del_turno: false,
    };
    crate::host::on_ui(move |studio, _, _| studio.handle(Msg::Asistente(AsistenteMsg::Ask(pregunta))));
    espera.recv_timeout(ESPERA_PERSONA).unwrap_or(false)
}

/// The window as live mode reaches it.
#[derive(Default)]
pub struct VentanaViva {
    /// Calls served, for each call's gesture.
    llamadas: AtomicU64,
}

impl LiveWindow for VentanaViva {
    fn call(&self, request: Request) -> Result<Reply, ApiError> {
        if let Request::PreviewFrame {
            time,
            output,
            width,
            height,
            ..
        } = request
        {
            return fotograma(time, output, width.zip(height));
        }
        if let Some((herramienta, detalle)) = a_preguntar(&request)
            && !preguntar(herramienta, detalle)
        {
            return Err(ApiError::new(ErrorCode::Refused, "la persona no lo ha permitido"));
        }
        let request = preparar(request)?;
        let gesto = format!("api:{}", self.llamadas.fetch_add(1, Ordering::Relaxed));
        en_ventana(None, move |studio| {
            let respuesta = studio.en_vivo(&gesto, request);
            if let Some(session) = studio.session.as_mut() {
                session.end_gesture();
            }
            respuesta
        })
    }
}

/// The call to put to the person first, if `request` is one: what it is
/// and what it will do.
fn a_preguntar(request: &Request) -> Option<(&'static str, String)> {
    match request {
        Request::ProjectSetVideo { video, .. } => Some((
            "project_set_video",
            tf("asistente.approve.video", &[&video.width, &video.height]),
        )),
        Request::MediaImport { file, .. } => Some(("media_import", tf("asistente.approve.import", &[file]))),
        Request::EditApply { command, .. } => match command.as_ref() {
            Command::RemoveClips { clip_ids, .. } if clip_ids.len() > QUITAR_SIN_PREGUNTAR => Some((
                "clips_remove",
                tf("asistente.approve.remove", &[&clip_ids.len(), &clip_ids.join(", ")]),
            )),
            _ => None,
        },
        _ => None,
    }
}

/// The timeline's frame at `time`, painted on this thread.
fn fotograma(time: f64, output: Option<String>, tamano: Option<(u32, u32)>) -> Result<Reply, ApiError> {
    if output.is_some() {
        return Err(ApiError::invalid(
            "en vivo el fotograma vuelve en la respuesta: no pases «output»",
        ));
    }
    let lado = tamano.map_or(LADO, |(ancho, alto)| ancho.max(alto));
    let materiales = en_ventana(None, move |studio| studio.fotograma_asistente(time, lado))?;
    let (ancho, alto) = (materiales.spec.width, materiales.spec.height);
    let pixeles = materiales
        .monitor
        .frame(materiales.clips, &materiales.ajustes, materiales.spec)
        .map_err(ApiError::failed)?;
    Ok(Reply::Picture(Picture::from_rgba(ancho, alto, &pixeles)?))
}

#[cfg(test)]
mod tests {
    use concat_project::model::VideoSettings;

    use super::*;

    #[test]
    fn what_cannot_be_undone_in_place_is_put_to_the_person_first() {
        let quitar = |n: usize| Request::EditApply {
            path: "/p".into(),
            command: Box::new(Command::RemoveClips {
                clip_ids: (0..n).map(|i| format!("c{i}")).collect(),
                ripple: false,
            }),
        };
        assert!(a_preguntar(&quitar(2)).is_none());
        assert_eq!(a_preguntar(&quitar(5)).map(|(h, _)| h), Some("clips_remove"));
        let video = Request::ProjectSetVideo {
            path: "/p".into(),
            video: VideoSettings::default(),
        };
        assert_eq!(a_preguntar(&video).map(|(h, _)| h), Some("project_set_video"));
        assert!(a_preguntar(&Request::EditUndo { path: "/p".into() }).is_none());
    }
}
