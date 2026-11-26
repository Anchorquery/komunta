// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Animation recipes: what a video editor does by hand with keys, said once
//! as the commands that do it. Each recipe is a pure function of the clip
//! it acts on - no window, no project open - so the agent's tools, the
//! montage and the window share one way of zooming on a phrase or bringing
//! a title in, and each is tested alone.
//!
//! - [`zoom`]: a push in on a stretch of a clip, eased or cut, towards a
//!   point of the frame.
//! - [`titulo`]: how a title comes in and goes: fades, rises, grows.
//! - [`seguir`]: a title that follows a face.
//! - [`copia`]: a copy of a clip, as Duplicate and Paste make one.
//! - [`movimiento`]: free keys an AI writes, the named moves (a pop, a
//!   shake, a pulse, a slide, a spin, a drift) and a photo's parallax.

pub mod copia;
pub mod llaves;
pub mod movimiento;
pub mod seguir;
pub mod tiempo;
pub mod titulo;
pub mod zoom;
