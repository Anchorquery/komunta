// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The person, as the agent's turn sees them from its own thread: what the
//! turn says goes to the pane as messages, a call that needs a yes waits
//! here for the person's answer, and Stop is a flag the turn reads between
//! calls.
//!
//! The two ends are made together ([`par`]): [`EnVentana`] goes with the
//! turn, [`Mando`] stays with the pane, which stops the turn through it.
//! Each question carries the channel its answer comes back on
//! ([`Pregunta::respuesta`]), as a caller on the socket's does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use asistente::{Aprobacion, Evento, Persona};

use super::asistente::{AsistenteMsg, Pregunta, a_la_ventana};

/// How often a turn waiting for a yes looks whether Stop was pressed.
const VISTAZO: Duration = Duration::from_millis(200);

/// The turn's end: it tells the window, and waits for it.
pub struct EnVentana {
    epoch: u64,
    parada: Arc<AtomicBool>,
}

/// The pane's end: Stop.
pub struct Mando {
    parada: Arc<AtomicBool>,
}

/// Both ends, for a turn begun with the project of `epoch` open.
pub fn par(epoch: u64) -> (EnVentana, Mando) {
    let parada = Arc::new(AtomicBool::new(false));
    let turno = EnVentana {
        epoch,
        parada: Arc::clone(&parada),
    };
    (turno, Mando { parada })
}

impl EnVentana {
    /// The flag a long tool reads to give up early.
    pub fn parada(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.parada)
    }
}

impl Persona for EnVentana {
    fn avisar(&mut self, evento: Evento) {
        a_la_ventana(self.epoch, AsistenteMsg::Event(evento));
    }

    /// Puts the call to the person and waits for the answer; Stop, or a
    /// window gone, is a no.
    fn aprobar(&mut self, aprobacion: &Aprobacion) -> bool {
        let (respuesta, espera) = mpsc::channel();
        let pregunta = Pregunta {
            herramienta: aprobacion.herramienta.clone(),
            detalle: aprobacion.detalle.clone(),
            respuesta,
            del_turno: true,
        };
        a_la_ventana(self.epoch, AsistenteMsg::Ask(pregunta));
        esperar(&espera, &self.parada)
    }

    fn parar(&self) -> bool {
        self.parada.load(Ordering::Relaxed)
    }
}

/// The answer on `espera`, or a no once `parada` is set or nobody is left
/// to answer.
fn esperar(espera: &mpsc::Receiver<bool>, parada: &AtomicBool) -> bool {
    loop {
        match espera.recv_timeout(VISTAZO) {
            Ok(si) => return si,
            Err(RecvTimeoutError::Timeout) if !parada.load(Ordering::Relaxed) => {}
            Err(_) => return false,
        }
    }
}

impl Mando {
    /// Stops the turn after the call under way; a question of its waiting
    /// counts as a no.
    pub fn parar(&self) {
        self.parada.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_reaches_the_turn_and_stop_is_a_no() {
        let (turno, mando) = par(0);
        let (respuesta, espera) = mpsc::channel();
        respuesta.send(true).unwrap();
        assert!(esperar(&espera, &turno.parada));
        assert!(!turno.parar());
        mando.parar();
        assert!(turno.parar());
        assert!(!esperar(&espera, &turno.parada), "stopped: a no");
        drop(respuesta);
        assert!(!esperar(&espera, &AtomicBool::new(false)), "nobody to answer: a no");
    }
}
