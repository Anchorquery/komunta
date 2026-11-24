// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The guards of a turn: an AI that repeats the same call, or keeps
//! failing with the same tool, is warned and then has that tool stopped -
//! refused when called for the rest of the turn, which goes on without it -
//! and a turn has a budget of steps and time it closes within.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Calls with the same arguments before the AI is warned, and before the
/// tool is withdrawn.
const REPETIDAS: (usize, usize) = (3, 5);
/// Failures in a row of one tool before the AI is warned, and before the
/// tool is withdrawn.
const FALLOS: (usize, usize) = (2, 3);

/// What the guards remember of a turn.
pub struct Guardia {
    firmas: HashMap<String, usize>,
    fallos: HashMap<String, usize>,
    /// The tools stopped for the rest of the turn.
    pub bloqueadas: BTreeSet<String>,
    limites: Limites,
    inicio: Instant,
}

/// How far a turn may go.
#[derive(Clone, Copy, Debug)]
pub struct Limites {
    /// Times the AI is asked.
    pub pasos: usize,
    /// Time from the person's words.
    pub tiempo: Duration,
}

impl Default for Limites {
    fn default() -> Self {
        Self {
            pasos: 30,
            tiempo: Duration::from_secs(300),
        }
    }
}

/// What a step may use, as the budget left has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Margen {
    /// Every tool.
    Pleno,
    /// Only tools that read: close what is open, check it.
    SoloLectura,
    /// No tools: say what was done.
    Cerrar,
}

impl Guardia {
    /// The guards of a turn starting now.
    pub fn new(limites: Limites) -> Self {
        Self {
            firmas: HashMap::new(),
            fallos: HashMap::new(),
            bloqueadas: BTreeSet::new(),
            limites,
            inicio: Instant::now(),
        }
    }

    /// What step `paso` (from 0) may use.
    pub fn margen(&self, paso: usize) -> Margen {
        let quedan = self.limites.pasos.saturating_sub(paso);
        let tiempo = self.inicio.elapsed().as_secs_f64() / self.limites.tiempo.as_secs_f64();
        if quedan <= 1 || tiempo >= 0.85 {
            Margen::Cerrar
        } else if quedan <= 3 || tiempo >= 0.7 {
            Margen::SoloLectura
        } else {
            Margen::Pleno
        }
    }

    /// Whether the turn has used every step.
    pub fn agotado(&self, paso: usize) -> bool {
        paso >= self.limites.pasos
    }

    /// Records a call of `nombre` with `argumentos` that failed or not; a
    /// warning for the AI when it is going round in circles. A tool past
    /// the second threshold is stopped.
    pub fn registrar(&mut self, nombre: &str, argumentos: &Value, fallo: bool) -> Option<String> {
        let firma = format!("{nombre}{argumentos}");
        let repetidas = *self.firmas.entry(firma).and_modify(|n| *n += 1).or_insert(1);
        let fallos = self.fallos.entry(nombre.to_owned()).or_insert(0);
        *fallos = if fallo { *fallos + 1 } else { 0 };
        let fallos = *fallos;
        if repetidas >= REPETIDAS.1 || fallos >= FALLOS.1 {
            self.bloqueadas.insert(nombre.to_owned());
            return Some(format!(
                "{nombre} queda bloqueada en este turno: se repetía o fallaba sin avanzar. Sigue de otra forma o \
                 explica a la persona qué falta."
            ));
        }
        if repetidas >= REPETIDAS.0 {
            return Some(format!(
                "Has llamado {nombre} {repetidas} veces con los mismos argumentos. Cambia de enfoque."
            ));
        }
        if fallos >= FALLOS.0 {
            return Some(format!("{nombre} ha fallado {fallos} veces seguidas. Lee el error y corrige los argumentos."));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_same_call_again_and_again_is_warned_then_withdrawn() {
        let mut guardia = Guardia::new(Limites::default());
        let args = json!({"time": 1});
        assert_eq!(guardia.registrar("clips_split", &args, false), None);
        assert_eq!(guardia.registrar("clips_split", &args, false), None);
        assert!(guardia.registrar("clips_split", &args, false).unwrap().contains("3 veces"));
        guardia.registrar("clips_split", &args, false);
        assert!(guardia.registrar("clips_split", &args, false).unwrap().contains("bloqueada"));
        assert!(guardia.bloqueadas.contains("clips_split"));
    }

    #[test]
    fn failures_in_a_row_count_and_a_success_clears_them() {
        let mut guardia = Guardia::new(Limites::default());
        assert_eq!(guardia.registrar("zoom_en", &json!(1), true), None);
        assert!(guardia.registrar("zoom_en", &json!(2), true).unwrap().contains("2 veces seguidas"));
        assert_eq!(guardia.registrar("zoom_en", &json!(3), false), None);
        assert_eq!(guardia.registrar("zoom_en", &json!(4), true), None);
        assert!(!guardia.bloqueadas.contains("zoom_en"));
    }

    #[test]
    fn the_last_steps_read_then_close() {
        let guardia = Guardia::new(Limites {
            pasos: 10,
            tiempo: Duration::from_secs(300),
        });
        assert_eq!(guardia.margen(0), Margen::Pleno);
        assert_eq!(guardia.margen(7), Margen::SoloLectura);
        assert_eq!(guardia.margen(9), Margen::Cerrar);
        assert!(guardia.agotado(10));
    }
}
