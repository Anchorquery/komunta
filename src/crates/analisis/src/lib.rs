//! What a shoot's clips hold, for the montage: who speaks, what is said
//! word by word and in which language, how noisy the sound is, and
//! what the picture shows: how sharp and still, and where the faces are.
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
pub mod caras;
#[cfg(feature = "escucha")]
pub mod clip;
#[cfg(feature = "escucha")]
pub mod escucha;
pub mod idioma;
#[cfg(feature = "imagen")]
pub mod mirada;
pub mod musica;
pub mod palabras;
pub mod recorte;
pub mod ruido;
pub mod tokens;
pub mod trabajo;
pub mod transcripcion;
pub mod visual;
pub mod voz;

pub use idioma::{Idioma, Reparto, Tramo};
pub use musica::Ritmo;
pub use palabras::{Palabra, PalabraOida};
pub use ruido::Ruido;
pub use tokens::TokenOido;
pub use transcripcion::Transcripcion;
pub use visual::{Cara, Encuadre, TramoVisual, Visual};
