// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! An agent of someone else's - Gemini CLI, Claude Code, Codex, any that
//! speaks the Agent Client Protocol - in the pane instead of Komunta's
//! own. It runs as its own process with its own AI and its own account;
//! it reaches the project through the MCP servers it is handed (Komunta's
//! `--mcp window`, in live mode), and the person sees its words, its
//! tools, its plan and its questions as they see Komunta's.
//!
//! A [`Sesion`] keeps the agent's process and its ACP session for the
//! whole conversation, on a thread of its own ([`conexion`]); a turn
//! ([`Sesion::turno`]) hands it the person's words and plays back what
//! comes the way [`crate::turno`] would: [`Evento`]s, approvals, Stop.

mod ajustes;
mod conexion;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;

use crate::{Conversacion, Evento, Fin, Mensaje, Parte, Persona, Rol};

pub use ajustes::{Ajuste, Faceta, Valor};
pub use conexion::Suceso;

use conexion::Orden;

/// How often a turn looks whether Stop was pressed.
const VISTAZO: Duration = Duration::from_millis(200);

/// How long an agent may take to start: `npx` fetches it the first time.
const ARRANQUE: Duration = Duration::from_secs(180);

/// How to start an agent: the program and what it is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agente {
    /// The program, by name or path.
    pub orden: String,
    /// Its arguments.
    pub argumentos: Vec<String>,
    /// What its environment is given besides this program's: a key.
    pub entorno: Vec<(String, String)>,
    /// The ACP sign-in it is asked for before the session, when it offers
    /// it: the one that takes the key in `entorno`, rather than whatever
    /// the agent was last signed in with.
    pub acceso: Option<String>,
}

/// An MCP server the agent is to start for itself, over stdio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Servidor {
    /// The name the agent knows its tools by.
    pub nombre: String,
    /// The program.
    pub orden: PathBuf,
    /// Its arguments.
    pub argumentos: Vec<String>,
}

/// An agent running, with its session open in `carpeta`.
#[derive(Debug)]
pub struct Sesion {
    pedidos: UnboundedSender<Orden>,
    cancelar: UnboundedSender<()>,
    sucesos: Receiver<Suceso>,
    /// What the person can choose for the session, as the agent last
    /// said - or as the person last chose, until it answers.
    ajustes: Vec<Ajuste>,
}

impl Sesion {
    /// Starts `agente` and opens a session in `carpeta` with `servidores`,
    /// or says why it could not.
    pub fn abrir(agente: &Agente, carpeta: PathBuf, servidores: Vec<Servidor>) -> Result<Self, String> {
        let (pedidos, pedidos_rx) = futures::channel::mpsc::unbounded();
        let (cancelar, cancelar_rx) = futures::channel::mpsc::unbounded();
        let (sucesos_tx, sucesos) = mpsc::channel();
        let (listo_tx, listo) = mpsc::channel();
        let arranque = conexion::Arranque {
            agente: agente.clone(),
            carpeta,
            servidores,
            pedidos: pedidos_rx,
            cancelar: cancelar_rx,
            sucesos: sucesos_tx,
            listo: listo_tx,
        };
        std::thread::Builder::new()
            .name("agente-externo".into())
            .spawn(move || conexion::servir(arranque))
            .map_err(|error| error.to_string())?;
        match listo.recv_timeout(ARRANQUE) {
            Ok(Ok(ajustes)) => Ok(Self {
                pedidos,
                cancelar,
                sucesos,
                ajustes,
            }),
            Ok(Err(motivo)) => Err(motivo),
            Err(_) => Err(format!("«{}» no arrancó", agente.orden)),
        }
    }

    /// What the person can choose for the session: its model, how much it
    /// thinks, its mode - whichever of them the agent offers.
    pub fn ajustes(&mut self) -> &[Ajuste] {
        // Between turns only the answers to a choice come.
        while let Ok(suceso) = self.sucesos.try_recv() {
            if let Suceso::Ajustes(ajustes) = suceso {
                self.ajustes = ajustes;
            }
        }
        &self.ajustes
    }

    /// Sets the choice `id` to `valor`, as the person picked it; the agent
    /// takes it in before the next turn.
    pub fn ajustar(&mut self, id: &str, valor: &str) {
        if let Some(ajuste) = self.ajustes.iter_mut().find(|ajuste| ajuste.id == id) {
            ajuste.actual = valor.to_owned();
        }
        let _ = self.pedidos.unbounded_send(Orden::Ajustar {
            id: id.to_owned(),
            valor: valor.to_owned(),
        });
    }

    /// One turn: `texto` to the agent, what it does played to `persona`,
    /// its last words kept in `conversacion`.
    pub fn turno(
        &mut self,
        persona: &mut dyn Persona,
        conversacion: &mut Conversacion,
        texto: &str,
    ) -> Result<Fin, String> {
        conversacion.mensajes.push(Mensaje::del_usuario(texto));
        self.pedidos
            .unbounded_send(Orden::Turno(texto.to_owned()))
            .map_err(|_| "el agente se cerró".to_owned())?;
        let mut turno = Turno::default();
        loop {
            match self.sucesos.recv_timeout(VISTAZO) {
                Ok(Suceso::Fin(fin)) => return Ok(turno.cerrar(fin?, conversacion)),
                Ok(Suceso::Ajustes(ajustes)) => self.ajustes = ajustes,
                Ok(suceso) => turno.recibir(suceso, persona),
                Err(RecvTimeoutError::Timeout) => {
                    if persona.parar() && !turno.cancelado {
                        turno.cancelado = true;
                        let _ = self.cancelar.unbounded_send(());
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Err("el agente se cerró".to_owned()),
            }
        }
    }
}

/// A turn as it goes: the words not yet said, whether Stop was sent.
#[derive(Default)]
struct Turno {
    palabras: String,
    plan: Option<Vec<crate::Tarea>>,
    cancelado: bool,
}

impl Turno {
    fn recibir(&mut self, suceso: Suceso, persona: &mut dyn Persona) {
        match suceso {
            Suceso::Trozo(trozo) => self.palabras.push_str(&trozo),
            Suceso::Pensando => persona.avisar(Evento::Pensando),
            Suceso::Herramienta(nombre) => {
                self.decir(persona);
                persona.avisar(Evento::Herramienta(nombre));
            }
            Suceso::Plan(tareas) => {
                self.plan = Some(tareas.clone());
                persona.avisar(Evento::Plan(tareas));
            }
            Suceso::Permiso(aprobacion, respuesta) => {
                self.decir(persona);
                // Stop pressed: the agent was told, and a question still
                // put is a no.
                let si = !self.cancelado && persona.aprobar(&aprobacion);
                let _ = respuesta.send(si);
            }
            Suceso::Fin(_) | Suceso::Ajustes(_) => unreachable!("handled by the session"),
        }
    }

    /// Words between tools go to the person as they come.
    fn decir(&mut self, persona: &mut dyn Persona) {
        let dicho = std::mem::take(&mut self.palabras);
        if !dicho.trim().is_empty() {
            persona.avisar(Evento::Texto(dicho));
        }
    }

    /// The last words are the answer; the plan stays with the conversation.
    fn cerrar(self, fin: Fin, conversacion: &mut Conversacion) -> Fin {
        if let Some(plan) = self.plan {
            conversacion.plan = plan;
        }
        conversacion.mensajes.push(Mensaje {
            rol: Rol::Modelo,
            partes: vec![Parte::Texto(self.palabras)],
            crudo: None,
        });
        fin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Aprobacion, Clase, Estado, Tarea};

    #[derive(Default)]
    struct Mira {
        eventos: Vec<Evento>,
        preguntas: usize,
    }

    impl Persona for Mira {
        fn avisar(&mut self, evento: Evento) {
            self.eventos.push(evento);
        }
        fn aprobar(&mut self, _: &Aprobacion) -> bool {
            self.preguntas += 1;
            true
        }
        fn parar(&self) -> bool {
            false
        }
    }

    #[test]
    fn words_before_a_tool_are_said_and_the_last_are_the_answer() {
        let mut mira = Mira::default();
        let mut turno = Turno::default();
        for suceso in [
            Suceso::Trozo("Miro ".into()),
            Suceso::Trozo("el proyecto.".into()),
            Suceso::Herramienta("project_get".into()),
            Suceso::Plan(vec![Tarea {
                texto: "cortar".into(),
                estado: Estado::EnCurso,
            }]),
            Suceso::Trozo("Hecho.".into()),
        ] {
            turno.recibir(suceso, &mut mira);
        }
        let mut conversacion = Conversacion::default();
        assert_eq!(turno.cerrar(Fin::Respuesta, &mut conversacion), Fin::Respuesta);
        assert_eq!(mira.eventos[0], Evento::Texto("Miro el proyecto.".into()));
        assert_eq!(mira.eventos[1], Evento::Herramienta("project_get".into()));
        assert_eq!(conversacion.respuesta(), "Hecho.");
        assert_eq!(conversacion.plan.len(), 1);
    }

    #[test]
    fn after_stop_a_question_is_a_no_without_asking() {
        let mut mira = Mira::default();
        let mut turno = Turno {
            cancelado: true,
            ..Turno::default()
        };
        let (respuesta, espera) = futures::channel::oneshot::channel();
        let aprobacion = Aprobacion {
            herramienta: "edit_apply".into(),
            clase: Clase::Edicion,
            detalle: String::new(),
        };
        turno.recibir(Suceso::Permiso(aprobacion, respuesta), &mut mira);
        assert_eq!(futures::executor::block_on(espera), Ok(false));
        assert_eq!(mira.preguntas, 0);
    }
}
