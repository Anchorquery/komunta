// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Komunta's agent, the part that knows nothing of windows.
//!
//! A conversation ([`Conversacion`]) is kept here in a shape no provider
//! owns. A turn ([`bucle::turno`]) hands it to a [`Proveedor`] with the
//! tools an [`Ejecutor`] offers, runs the tools the provider asks for and
//! hands back what they gave, until the provider answers in words. On the
//! way the harness keeps the agent's plan ([`plan`]), offers its tools by
//! family ([`catalogo`]), gets every step ready ([`paso`]), guards against
//! loops and runaway turns ([`guardia`]) and asks the [`Persona`] before
//! anything that cannot be undone in place. The window supplies the
//! executor - the editor's own tools, run on the project on screen - the
//! person, and a provider made from the AI settings: [`gemini`] for
//! Gemini, `generico` (feature `genai`) for every other AI. Or the pane
//! talks to someone else's agent over ACP instead: `externo` (feature
//! `acp`). The design is `docs/agente-komunta.md`.

pub mod bucle;
pub mod catalogo;
pub mod conversacion;
#[cfg(feature = "acp")]
pub mod externo;
pub mod gemini;
#[cfg(feature = "genai")]
pub mod generico;
pub mod guardia;
pub mod herramienta;
pub mod paso;
pub mod plan;
pub mod proveedor;
pub mod traza;

pub use bucle::{Fin, turno};
pub use conversacion::{Conversacion, Mensaje, Parte, Rol};
pub use gemini::Gemini;
#[cfg(feature = "genai")]
pub use generico::Generico;
pub use guardia::Limites;
pub use herramienta::{Clase, Herramienta, Imagen, NUCLEO, Salida, Tipo};
pub use plan::{Estado, Tarea};
pub use proveedor::{Aprobacion, Ejecutor, Esfuerzo, Evento, Modo, Pedido, Persona, Proveedor};
