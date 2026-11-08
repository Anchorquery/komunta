//! How a montage run reports how it goes, and how it is stopped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One progress report.
#[derive(Clone, Debug, PartialEq)]
pub struct Avance {
    /// "ingesta", "analisis", "descripcion", "plan".
    pub fase: String,
    /// 0..=1 within the phase.
    pub fraccion: f64,
    /// What it is doing, in Spanish.
    pub mensaje: String,
}

/// A run that can be stopped from another thread: whisper, the describer
/// and the planner look at it as they go.
#[derive(Clone, Debug, Default)]
pub struct Parada {
    cancelada: Arc<AtomicBool>,
}

impl Parada {
    /// Asks the run to stop.
    pub fn cancelar(&self) {
        self.cancelada.store(true, Ordering::SeqCst);
    }

    /// Whether [`Parada::cancelar`] was called.
    pub fn cancelada(&self) -> bool {
        self.cancelada.load(Ordering::SeqCst)
    }

    /// The flag [`Parada::cancelar`] raises, for work that takes it as one.
    pub fn bandera(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelada)
    }
}
