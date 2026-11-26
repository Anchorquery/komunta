// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Live mode (jub0t/Concat#337): with the window's switch on, a request
//! that names the project open on screen is run there - on the window's
//! session, with the same command, the edit seen at once - instead of
//! being refused because the window holds the folder. Reads come from the
//! window's state too. Off, nothing here runs and the window's project
//! stays the person's alone.
//!
//! The API does not know the window: an embedder hands it a [`LiveWindow`]
//! and switches it on and off ([`LiveSwitch`]); the API asks which project
//! the window has and hands it the requests that are its.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::message::{ApiError, Reply, Request};

/// The window, as live mode reaches it.
pub trait LiveWindow: Send + Sync {
    /// Runs `request` on the window's session and gives its reply. The
    /// window runs it on its own thread and decides, for an edit that
    /// cannot be undone in place, whether to ask the person first.
    fn call(&self, request: Request) -> Result<Reply, ApiError>;
}

/// Whether live mode is on: the window's setting, shared with the API.
/// Cheap to clone; every clone is the same switch.
#[derive(Clone, Default)]
pub struct LiveSwitch(Arc<AtomicBool>);

impl LiveSwitch {
    /// Turns live mode on or off.
    pub fn set(&self, on: bool) {
        self.0.store(on, Ordering::Relaxed);
    }

    /// Whether it is on.
    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// The project folder a request acts on, for the methods live mode hands
/// to the window: reading it, editing it, importing into it, looking at
/// its frames and running the window's own tools on it. Saving, closing and exporting the window's project stay
/// the person's.
pub fn live_path(request: &Request) -> Option<&str> {
    match request {
        Request::ProjectOpen { path }
        | Request::ProjectGet { path }
        | Request::ProjectDocument { path }
        | Request::ProjectSetVideo { path, .. }
        | Request::EditApply { path, .. }
        | Request::EditUndo { path }
        | Request::EditRedo { path }
        | Request::MediaImport { path, .. }
        | Request::PreviewFrame { path, .. }
        | Request::WindowTool { path, .. } => Some(path),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_reads_go_live_and_saving_closing_and_exporting_do_not() {
        let path = "/p".to_owned();
        assert_eq!(live_path(&Request::EditUndo { path: path.clone() }), Some("/p"));
        assert_eq!(live_path(&Request::ProjectGet { path: path.clone() }), Some("/p"));
        assert_eq!(
            live_path(&Request::ProjectSave {
                path: path.clone(),
                name: None
            }),
            None
        );
        assert_eq!(live_path(&Request::ProjectClose { path, save: true }), None);
        assert_eq!(live_path(&Request::Version), None);
    }

    #[test]
    fn the_switch_is_shared_by_its_clones() {
        let switch = LiveSwitch::default();
        let other = switch.clone();
        assert!(!other.is_on());
        switch.set(true);
        assert!(other.is_on());
    }
}
