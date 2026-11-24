// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! A conversation as the assistant keeps it: who said what, and which
//! tools were called with what and gave what, in no provider's format.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::herramienta::Imagen;
use crate::plan::Tarea;

/// Who a message is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rol {
    /// The person, or the results of the tools run for the AI.
    Usuario,
    /// The AI.
    Modelo,
}

/// One piece of a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Parte {
    /// Words.
    Texto(String),
    /// The AI asks for a tool: its call's id (empty when the provider
    /// gives none), the tool's name and its arguments.
    Llamada {
        /// The provider's id for the call, echoed in its result.
        id: String,
        /// The tool.
        nombre: String,
        /// Its arguments, as JSON.
        argumentos: Value,
    },
    /// What a tool gave, for the call with the same id.
    Resultado {
        /// The call this answers.
        id: String,
        /// The tool.
        nombre: String,
        /// What it gave, as JSON.
        valor: Value,
        /// It failed; `valor` says why.
        error: bool,
        /// A picture the tool shows the AI: a frame. Sent inside the
        /// result to the models that take it there, beside it to the rest.
        imagen: Option<Imagen>,
    },
}

/// One message.
#[derive(Clone, Debug, PartialEq)]
pub struct Mensaje {
    /// Who it is from.
    pub rol: Rol,
    /// What it holds, in order.
    pub partes: Vec<Parte>,
    /// The provider's own form of a message from the AI, when it must be
    /// sent back exactly as it came - Gemini signs its reasoning, and a
    /// call sent back without the signature is refused. Only the provider
    /// that made it reads it.
    pub crudo: Option<Value>,
}

impl Mensaje {
    /// The person's words.
    pub fn del_usuario(texto: impl Into<String>) -> Self {
        Self {
            rol: Rol::Usuario,
            partes: vec![Parte::Texto(texto.into())],
            crudo: None,
        }
    }

    /// The words in it, joined.
    pub fn texto(&self) -> String {
        self.partes
            .iter()
            .filter_map(|parte| match parte {
                Parte::Texto(texto) => Some(texto.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The tools it asks for.
    pub fn llamadas(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.partes.iter().filter_map(|parte| match parte {
            Parte::Llamada {
                id,
                nombre,
                argumentos,
            } => Some((id.as_str(), nombre.as_str(), argumentos)),
            _ => None,
        })
    }
}

/// The whole conversation, oldest first, and what the agent keeps
/// across its turns: its plan and the families of tools it has asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Conversacion {
    /// Its messages.
    pub mensajes: Vec<Mensaje>,
    /// The agent's plan, as it last wrote it.
    pub plan: Vec<Tarea>,
    /// The families of tools offered besides the core.
    pub familias: BTreeSet<String>,
}

impl Conversacion {
    /// Back to nothing said.
    pub fn olvidar(&mut self) {
        *self = Self::default();
    }

    /// The AI's last words: those of its last message.
    pub fn respuesta(&self) -> String {
        self.mensajes
            .iter()
            .rev()
            .find(|mensaje| mensaje.rol == Rol::Modelo)
            .map(Mensaje::texto)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_message_reads_its_words_and_its_calls_apart() {
        let mensaje = Mensaje {
            rol: Rol::Modelo,
            partes: vec![
                Parte::Texto("Corto ".into()),
                Parte::Llamada {
                    id: "1".into(),
                    nombre: "clips_split".into(),
                    argumentos: json!({"time": 2.0}),
                },
                Parte::Texto("el clip.".into()),
            ],
            crudo: None,
        };
        assert_eq!(mensaje.texto(), "Corto el clip.");
        let llamadas: Vec<_> = mensaje.llamadas().collect();
        assert_eq!(llamadas, [("1", "clips_split", &json!({"time": 2.0}))]);
    }
}
