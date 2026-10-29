//! What a shoot's clips hold, for the montage: who speaks, what is said
//! word by word and in which language, and how noisy the sound is.
//!
//! A port of the Python engine's `motor/analisis.py`, module by module.
//! What it writes is the engine's `analisis.json`, field for field, so the
//! montage (`crates/montaje`) and the engine's AI step read it unchanged
//! while the rest of the engine is still being ported.
//!
//! The pieces that decide - which words to keep, which language won, how
//! loud the room is - are plain functions over plain data, tested here
//! without a model. The pieces that listen (whisper, the voice detector,
//! the decoder) hand them that data.

#[cfg(feature = "escucha")]
pub mod audio;
#[cfg(feature = "escucha")]
pub mod clip;
#[cfg(feature = "escucha")]
pub mod escucha;
pub mod idioma;
pub mod palabras;
pub mod recorte;
pub mod ruido;
pub mod tokens;
pub mod transcripcion;
pub mod voz;

pub use idioma::{Idioma, Segunda, Tramo};
pub use palabras::{Palabra, PalabraOida};
pub use ruido::Ruido;
pub use tokens::TokenOido;
pub use transcripcion::Transcripcion;
