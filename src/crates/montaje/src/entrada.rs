//! What the montage engine leaves in a job folder: the inventory of the
//! files, the analysis of each one and the AI's plan. Only the fields the
//! timeline needs are read; everything else in those files is ignored.

use std::path::Path;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::Error;

/// `ia/plan.json`: which takes, in what order, and what goes over them.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Plan {
    /// The recipe it was made with; for an automatic request, the one the
    /// AI chose.
    pub plantilla: String,
    /// The voice line, in order.
    pub tomas: Vec<Toma>,
    /// On-screen captions.
    pub subtitulos: Vec<Subtitulo>,
    /// B-roll laid over the voice line.
    pub recursos: Vec<Recurso>,
    /// Background music, when the plan has one.
    pub musica: Option<Musica>,
}

/// A take: either words `desde..=hasta` of a transcribed clip, or a shot of
/// `dur` seconds from `desde_seg` (a clip without speech, or a still).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Toma {
    /// Inventory id of the video or image.
    pub clip: String,
    /// First word kept.
    pub desde: Option<usize>,
    /// Last word kept.
    pub hasta: Option<usize>,
    /// Inclusive word ranges cut out of the take.
    pub quitar: Vec<[usize; 2]>,
    /// Shot start in the source, in seconds.
    pub desde_seg: Option<f64>,
    /// Shot length in seconds.
    pub dur: Option<f64>,
    /// Whether the shot's cut may snap to the music's beat.
    pub al_ritmo: Option<bool>,
    /// What a shot sounds like: "silencio" or "ambiente".
    pub audio: Option<String>,
    /// The take is only heard; its picture is covered.
    pub solo_audio: bool,
    /// Look over the picture: "bn" is black and white.
    pub efecto: Option<String>,
}

impl Toma {
    /// A take without words, cut by seconds.
    pub fn es_plano(&self) -> bool {
        self.desde_seg.is_some() && self.desde.is_none()
    }
}

/// A caption over words of a take, or over a span of a shot.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Subtitulo {
    /// The words shown.
    pub texto: String,
    /// The take it belongs to.
    pub toma: usize,
    /// First word it covers.
    pub desde: Option<usize>,
    /// Last word it covers.
    pub hasta: Option<usize>,
    /// Start inside a shot, in seconds.
    pub seg: Option<f64>,
    /// Length inside a shot, in seconds.
    pub dur: Option<f64>,
    /// "normal", "enfasis" or "clave".
    pub estilo: Option<String>,
    /// "auto", "arriba", "centro" or "abajo".
    pub posicion: Option<String>,
    /// Consecutive captions with the same group pile up on screen until
    /// the last one ends (lists).
    pub apilar: Option<serde_json::Value>,
    /// "arriba" when each new line of a pile goes above the last.
    pub apilar_hacia: Option<String>,
}

/// A piece of B-roll, anchored to a caption, to a word of a take or to a
/// second of a take.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Recurso {
    /// Inventory id of the video or image.
    pub clip: String,
    /// Caption it covers.
    pub subtitulo: Option<usize>,
    /// Take it is anchored to.
    pub toma: Option<usize>,
    /// Word of that take it starts on.
    pub palabra: Option<usize>,
    /// Second of that take it starts on.
    pub seg: Option<f64>,
    /// Length in seconds.
    pub dur: Option<f64>,
    /// Where in the source it starts.
    pub desde_seg: Option<f64>,
    /// Look over the picture: "bn" is black and white.
    pub efecto: Option<String>,
    /// Hand-drawn mark over it ("circulo", "corazon", "subrayado").
    pub garabato: Option<String>,
    /// Where the mark goes, as fractions of the frame.
    pub garabato_pos: Option<[f64; 2]>,
}

/// Background music chosen by the plan.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Musica {
    /// Audio id from the analysis (or "musica_kit").
    pub clip: Option<String>,
    /// Gain under the voice.
    pub volumen: Option<f64>,
    /// Where in the track it starts.
    pub desde_seg: Option<f64>,
}

/// `analisis.json`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Analisis {
    /// One per video.
    pub clips: Vec<ClipAnalisis>,
    /// One per still.
    pub imagenes: Vec<ImagenAnalisis>,
    /// One per audio file.
    pub audios: Vec<AudioAnalisis>,
}

/// What the analysis learned about a video.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ClipAnalisis {
    /// Inventory id.
    pub id: String,
    /// Seconds.
    pub duracion: f64,
    /// Speech, word by word, when there is any.
    pub transcripcion: Option<Transcripcion>,
    /// Centre of the main face, as fractions of the picture.
    pub encuadre: Option<Encuadre>,
}

/// A clip's speech.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Transcripcion {
    /// Every word, in order; `i` is its index.
    pub palabras: Vec<Palabra>,
}

/// One transcribed word.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Palabra {
    /// Index in the clip.
    pub i: usize,
    /// Start in the source, in seconds.
    pub ini: f64,
    /// End in the source, in seconds.
    pub fin: f64,
    /// The word as heard.
    pub txt: String,
    /// A filler ("eh", "pues") the analysis spotted.
    pub coletilla: bool,
}

/// Face centre, fractions of the picture.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct Encuadre {
    /// Across.
    pub cx: f64,
    /// Down.
    pub cy: f64,
}

/// What the analysis learned about a still.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImagenAnalisis {
    /// Inventory id.
    pub id: String,
    /// Faces as `[x, y, w, h]` fractions of the picture.
    pub caras: Vec<[f64; 4]>,
}

/// What the analysis learned about an audio file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AudioAnalisis {
    /// Inventory id.
    pub id: String,
    /// Path, for audio that is not in the inventory (the kit's music).
    pub ruta: Option<String>,
    /// Seconds.
    pub duracion: Option<f64>,
    /// Beat times in seconds.
    pub pulsos: Vec<f64>,
}

/// `inventario.json`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Inventario {
    /// Every usable video.
    pub videos: Vec<VideoInventario>,
    /// Every usable still.
    pub imagenes: Vec<ImagenInventario>,
    /// Every usable audio file.
    pub audios: Vec<AudioInventario>,
}

/// A video as probed (width and height already turned for rotation).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct VideoInventario {
    /// Id the plan uses.
    pub id: String,
    /// Absolute path.
    pub ruta: String,
    /// Seconds.
    pub duracion: f64,
    /// Displayed width.
    pub ancho: u32,
    /// Displayed height.
    pub alto: u32,
    /// Frames per second.
    pub fps: f64,
    /// Whether it carries sound.
    pub audio: Option<bool>,
}

/// A still.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImagenInventario {
    /// Id the plan uses.
    pub id: String,
    /// Absolute path.
    pub ruta: String,
    /// Width with EXIF orientation applied.
    pub ancho: u32,
    /// Height with EXIF orientation applied.
    pub alto: u32,
}

/// An audio file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AudioInventario {
    /// Id the plan uses.
    pub id: String,
    /// Absolute path.
    pub ruta: String,
    /// Seconds.
    pub duracion: Option<f64>,
}

/// The three files of a job folder.
#[derive(Clone, Debug, Default)]
pub struct Trabajo {
    /// `ia/plan.json`.
    pub plan: Plan,
    /// `analisis.json`.
    pub analisis: Analisis,
    /// `inventario.json`.
    pub inventario: Inventario,
}

impl Trabajo {
    /// Reads a job folder as the engine leaves it.
    pub fn leer(carpeta: &Path) -> Result<Trabajo, Error> {
        Ok(Trabajo {
            plan: leer_json(&carpeta.join("ia").join("plan.json"))?,
            analisis: leer_json(&carpeta.join("analisis.json"))?,
            inventario: leer_json(&carpeta.join("inventario.json"))?,
        })
    }
}

fn leer_json<T: DeserializeOwned>(ruta: &Path) -> Result<T, Error> {
    let texto = std::fs::read_to_string(ruta).map_err(|error| Error::Leer(ruta.into(), error))?;
    serde_json::from_str(&texto).map_err(|error| Error::Json(ruta.into(), error))
}
