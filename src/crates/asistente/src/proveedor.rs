// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The three sides a turn talks to: the AI that answers, whoever runs the
//! tools it asks for, and the person, who sees how it goes, says yes or
//! no to what cannot be undone in place, and may stop it.

use serde_json::Value;

use crate::conversacion::Mensaje;
use crate::herramienta::{Clase, Herramienta, Salida};
use crate::plan::Tarea;

/// What the AI is asked with: how to behave, the conversation so far,
/// the tools it has and which of them it may call now.
pub struct Pedido<'a> {
    /// The standing instruction.
    pub sistema: &'a str,
    /// The conversation, oldest first.
    pub mensajes: &'a [Mensaje],
    /// The tools declared. The list only grows within a conversation, so
    /// the request's prefix stays the one the provider has cached; what
    /// may be called this step is [`Pedido::modo`]'s to say.
    pub herramientas: &'a [Herramienta],
    /// Which tools may be called this step.
    pub modo: Modo,
}

/// Which declared tools the AI may call at a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Modo {
    /// Any, or none and words instead.
    Libre,
    /// Only these - the tools that read - or words.
    SoloLeer(Vec<String>),
    /// None: words only.
    Ninguna,
}

/// How much the AI thinks before it answers: little is seconds, a lot
/// is a better plan for a long task and a longer wait.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Esfuerzo {
    /// The least the model takes: seconds.
    #[default]
    Bajo,
    /// Some thought, for a task of a few steps.
    Medio,
    /// The most, for a long task.
    Alto,
}

impl Esfuerzo {
    /// All of them, least first.
    pub const TODOS: [Esfuerzo; 3] = [Esfuerzo::Bajo, Esfuerzo::Medio, Esfuerzo::Alto];

    /// Its name in settings: "low", "medium", "high".
    pub fn id(self) -> &'static str {
        match self {
            Esfuerzo::Bajo => "low",
            Esfuerzo::Medio => "medium",
            Esfuerzo::Alto => "high",
        }
    }

    /// The one named `id`; low for any other.
    pub fn desde_id(id: &str) -> Self {
        Self::TODOS.into_iter().find(|e| e.id() == id).unwrap_or_default()
    }
}

/// An AI that goes on with a conversation.
pub trait Proveedor: Send {
    /// Its name, for the person: "Gemini (gemini-2.5-flash)".
    fn nombre(&self) -> String;

    /// Its next message: words, calls to tools, or both. A failure is said
    /// in a sentence for the person.
    fn responder(&mut self, pedido: &Pedido<'_>) -> Result<Mensaje, String>;
}

/// Whoever runs the tools.
pub trait Ejecutor {
    /// The tools it offers.
    fn herramientas(&self) -> Vec<Herramienta>;

    /// Checks a call before it is approved or run - the ids it names
    /// exist, the times fall inside the timeline - so the person is never
    /// asked to approve what would fail. Why not, when it would.
    fn comprobar(&mut self, _nombre: &str, _argumentos: &Value) -> Result<(), String> {
        Ok(())
    }

    /// Whether this call waits for the person's yes: what its kind says,
    /// unless its arguments make it more or less than that - removing one
    /// clip is an edit, removing twenty is not.
    fn pide_aprobacion(&self, herramienta: &Herramienta, _argumentos: &Value) -> bool {
        herramienta.clase.pide_aprobacion()
    }

    /// What a call will do, in a sentence for the person who approves it.
    fn detalle(&self, nombre: &str, argumentos: &Value) -> String {
        format!("{nombre} {argumentos}")
    }

    /// Runs `nombre` with `argumentos`.
    fn ejecutar(&mut self, nombre: &str, argumentos: Value) -> Salida;
}

/// Something the person may see while a turn runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Evento {
    /// The AI is being asked.
    Pensando,
    /// A tool is about to run.
    Herramienta(String),
    /// The AI said this, before or between its tools.
    Texto(String),
    /// The AI wrote its plan.
    Plan(Vec<Tarea>),
}

/// A call waiting for the person's yes.
#[derive(Clone, Debug, PartialEq)]
pub struct Aprobacion {
    /// The tool.
    pub herramienta: String,
    /// What kind of effect it has.
    pub clase: Clase,
    /// What it will do, in a sentence.
    pub detalle: String,
}

/// The person, as a turn sees them.
pub trait Persona {
    /// Tells them how the turn goes.
    fn avisar(&mut self, evento: Evento);

    /// Asks them to approve a call, and waits for the answer: yes or no.
    fn aprobar(&mut self, aprobacion: &Aprobacion) -> bool;

    /// Whether they pressed Stop.
    fn parar(&self) -> bool;
}
