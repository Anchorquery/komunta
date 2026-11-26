// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The picture models as Settings lists them: each a thing the user can
//! have on disk or not - cutting out a person, cutting out an object, the
//! selection brush, the enhancer - made of one model file or, for the
//! brush, two. They are fetched on first use anyway; this is what lets
//! them be seen, fetched ahead of a trip without network, and taken off
//! the disk again.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use concat_vision::{ModelId, models};

use crate::cutout::{Progress, fetch};

/// One entry of the list.
#[derive(Clone, Copy, Debug)]
pub struct Pack {
    /// Its id in the Settings list: `vision:` and a word.
    pub id: &'static str,
    /// The model files it is made of.
    pub parts: &'static [ModelId],
}

/// Every picture model, in the order the list shows them.
pub const PACKS: [Pack; 4] = [
    Pack {
        id: "vision:person",
        parts: &[ModelId::Person],
    },
    Pack {
        id: "vision:object",
        parts: &[ModelId::Object],
    },
    Pack {
        id: "vision:brush",
        parts: &[ModelId::BrushEncoder, ModelId::BrushDecoder],
    },
    Pack {
        id: "vision:enhance",
        parts: &[ModelId::Enhance],
    },
];

impl Pack {
    /// The entry with `id`.
    pub fn by_id(id: &str) -> Option<Pack> {
        PACKS.iter().copied().find(|pack| pack.id == id)
    }

    /// Bytes on disk once fetched.
    pub fn bytes(self) -> u64 {
        self.parts.iter().map(|part| part.spec().bytes).sum()
    }

    /// The licences it comes under, one each, joined.
    pub fn licence(self) -> String {
        let mut licences: Vec<&str> = self.parts.iter().map(|part| part.spec().licence).collect();
        licences.dedup();
        licences.join(", ")
    }

    /// Whether every file of it is on disk under `data`.
    pub fn installed(self, data: &Path) -> bool {
        self.parts.iter().all(|part| models::installed(data, *part))
    }

    /// Fetches what of it is missing, checked against the table's digests;
    /// `progress` hears bytes so far of the whole, and `cancel` stops it,
    /// keeping the partial file for the next attempt.
    pub fn download(self, data: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let total = self.bytes();
        let mut done = 0;
        for part in self.parts {
            let bytes = part.spec().bytes;
            if !models::installed(data, *part) {
                let file = models::model_file(data, *part);
                fetch(*part, &file, cancel, &mut |report| {
                    if let Progress::Fetching { received, .. } = report {
                        progress(done + received, total);
                    }
                })?;
            }
            done += bytes;
            progress(done, total);
        }
        Ok(())
    }

    /// Takes its files off the disk, partial downloads too. A model already
    /// loaded keeps working until the app closes; the next use after that
    /// fetches it again.
    pub fn remove(self, data: &Path) -> Result<(), String> {
        for part in self.parts {
            let file = models::model_file(data, *part);
            for path in [file.clone(), file.with_extension("part")] {
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(format!("could not delete {}: {error}", path.display())),
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_downloadable_picture_model_is_in_one_entry() {
        let mut listed: Vec<ModelId> = PACKS.iter().flat_map(|pack| pack.parts.iter().copied()).collect();
        assert_eq!(listed.len(), models::MODELS.len());
        for spec in models::MODELS {
            assert!(listed.contains(&spec.id), "{:?} is not listed", spec.id);
        }
        listed.dedup();
        assert_eq!(listed.len(), models::MODELS.len());
        assert!(Pack::by_id("vision:brush").unwrap().bytes() > 30_000_000);
        assert!(Pack::by_id("vision:nada").is_none());
    }

    #[test]
    fn a_removed_pack_is_not_installed_and_removing_nothing_is_fine() {
        let data = std::env::temp_dir().join(format!("komunta-vision-{}", std::process::id()));
        let pack = Pack::by_id("vision:enhance").unwrap();
        let file = models::model_file(&data, ModelId::Enhance);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"onnx").unwrap();
        assert!(pack.installed(&data));
        pack.remove(&data).unwrap();
        assert!(!pack.installed(&data));
        pack.remove(&data).unwrap();
        let _ = std::fs::remove_dir_all(&data);
    }
}
