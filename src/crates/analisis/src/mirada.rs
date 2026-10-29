//! A clip as the analysis sees it: sampled a couple of times a second for
//! its look, and a few frames kept - written as small JPEGs for the AI that
//! describes the clips - with the faces found in each.

use std::path::Path;

use concat_core::{Frame, FrameRate, Rational};
use concat_media::{DecodeOptions, Decoder, FrameSource, MediaInfo};
use serde::{Deserialize, Serialize};

use crate::caras::{BuscaCaras, LADO};
use crate::visual::{self, ALTO, ANCHO, Cara, Encuadre, Gris, TramoVisual, Visual};

/// The width a kept frame is written at: enough for the AI to see what is
/// in it, small enough to send many.
const ANCHO_FOTOGRAMA: u32 = 360;

/// JPEG quality, as ffmpeg's `-q:v` (2 best, 31 worst).
const CALIDAD_JPEG: u8 = 4;

/// Why a clip could not be looked at.
#[derive(Debug, thiserror::Error)]
pub enum ErrorMirada {
    /// The clip would not decode.
    #[error("{0}")]
    Medio(#[from] concat_media::Error),
    /// A frame could not be written.
    #[error("{0}")]
    Disco(#[from] std::io::Error),
    /// The face detector failed.
    #[error("caras: {0}")]
    Caras(String),
    /// There is no picture to look at.
    #[error("el clip no tiene imagen")]
    SinImagen,
}

/// A kept frame, as `analisis.json` lists it under `fotogramas`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fotograma {
    /// Seconds into the clip.
    pub t: f64,
    /// The JPEG, relative to the job's folder.
    pub ruta: String,
    /// The faces in it, most likely first.
    pub caras: Vec<Cara>,
}

/// What a clip's picture holds, under the names `analisis.json` gives it:
/// the picture half of the clip's entry, beside [`crate::clip::Escucha`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mirada {
    /// Its look over time.
    pub visual: Visual,
    /// The stretch to show when it is used as footage.
    pub mejor_tramo: TramoVisual,
    /// The frames kept, with their faces.
    pub fotogramas: Vec<Fotograma>,
    /// Somebody is close to the camera in some frame.
    pub cara_cerca: bool,
    /// Where to centre a reframe; None with no faces.
    pub encuadre: Option<Encuadre>,
}

/// Looks at the clip at `ruta`, `duracion` seconds long, keeping `cuantos`
/// frames as `<trabajo>/fotogramas/<id>_<n>.jpg`. With no detector - ONNX
/// Runtime cannot run here - the frames are kept without faces.
pub fn mirar(
    detector: Option<&mut dyn BuscaCaras>,
    ruta: &Path,
    id: &str,
    duracion: f64,
    cuantos: usize,
    trabajo: &Path,
) -> Result<Mirada, ErrorMirada> {
    let visual = visual(ruta)?;
    let mejor_tramo = visual.mejor_tramo(duracion, visual::LARGO_TRAMO);
    let fotogramas = fotogramas(ruta, id, duracion, cuantos, trabajo, detector)?;
    let cara_cerca = fotogramas.iter().flat_map(|f| &f.caras).any(Cara::cerca);
    let encuadre = visual::encuadre(fotogramas.iter().map(|f| f.caras.as_slice()));
    Ok(Mirada {
        visual,
        mejor_tramo,
        fotogramas,
        cara_cerca,
        encuadre,
    })
}

/// The clip's look over time: [`visual::FPS_MUESTREO`] samples a second,
/// cropped to 9:16 and shrunk to [`ANCHO`] × [`ALTO`], in grey.
pub fn visual(ruta: &Path) -> Result<Visual, ErrorMirada> {
    let opciones = DecodeOptions::default()
        .prefiltered(format!(
            "scale={ANCHO}:{ALTO}:force_original_aspect_ratio=increase,crop={ANCHO}:{ALTO}"
        ))
        .scaled_to(ANCHO, ALTO)
        .at_rate(FrameRate::new(Rational::new(
            visual::FPS_MUESTREO as i64,
            1,
        )));
    let mut lector = Decoder::open(ruta, &opciones)?;
    let mut grises = Vec::new();
    while let Some(fotograma) = lector.next_frame()? {
        grises.push(gris(&fotograma));
    }
    let muestras = grises.iter().map(|pixeles| Gris {
        ancho: ANCHO as usize,
        alto: ALTO as usize,
        pixeles,
    });
    Ok(Visual::medir(muestras, visual::FPS_MUESTREO))
}

/// `cuantos` frames spread evenly through the `duracion`-second clip
/// (never its first or last instant), each written to `<trabajo>/fotogramas/<id>_<n>.jpg`, with
/// the faces in it when there is a detector.
pub fn fotogramas(
    ruta: &Path,
    id: &str,
    duracion: f64,
    cuantos: usize,
    trabajo: &Path,
    mut detector: Option<&mut dyn BuscaCaras>,
) -> Result<Vec<Fotograma>, ErrorMirada> {
    let info = concat_media::probe(ruta)?;
    let carpeta = trabajo.join("fotogramas");
    std::fs::create_dir_all(&carpeta)?;
    (0..cuantos)
        .map(|n| {
            let t = duracion * (n + 1) as f64 / (cuantos + 1) as f64;
            let nombre = format!("{id}_{n}.jpg");
            let caras = match detector.as_deref_mut() {
                Some(detector) => caras_en(detector, ruta, t, &info)?,
                None => Vec::new(),
            };
            let (ancho, alto) = a_lo_ancho(&info, ANCHO_FOTOGRAMA)?;
            let pequeno = uno(ruta, t, ancho, alto)?;
            std::fs::write(
                carpeta.join(&nombre),
                concat_media::jpeg(&pequeno, CALIDAD_JPEG)?,
            )?;
            Ok(Fotograma {
                t: (t * 100.0).round() / 100.0,
                ruta: format!("fotogramas/{nombre}"),
                caras,
            })
        })
        .collect()
}

/// The faces in the frame at `t` seconds, looked for at the size the
/// detector sees.
fn caras_en(
    detector: &mut dyn BuscaCaras,
    ruta: &Path,
    t: f64,
    info: &MediaInfo,
) -> Result<Vec<Cara>, ErrorMirada> {
    let (ancho, alto) = encajar(info, LADO as u32)?;
    let fotograma = uno(ruta, t, ancho, alto)?;
    detector
        .caras(fotograma.pixels(), ancho as usize, alto as usize)
        .map_err(ErrorMirada::Caras)
}

/// The frame at `t` seconds, at `ancho` × `alto`.
fn uno(ruta: &Path, t: f64, ancho: u32, alto: u32) -> Result<Frame, ErrorMirada> {
    let desde = Rational::new((t * 1000.0).round() as i64, 1000);
    let opciones = DecodeOptions::default()
        .starting_at(desde)
        .limited_to(1)
        .scaled_to(ancho, alto);
    Decoder::open(ruta, &opciones)?
        .next_frame()?
        .ok_or(ErrorMirada::SinImagen)
}

/// The picture's size with its longer side at `lado`, even.
fn encajar(info: &MediaInfo, lado: u32) -> Result<(u32, u32), ErrorMirada> {
    let video = info.video.as_ref().ok_or(ErrorMirada::SinImagen)?;
    let escala = f64::from(lado) / f64::from(video.width.max(video.height).max(1));
    Ok((
        par(f64::from(video.width) * escala),
        par(f64::from(video.height) * escala),
    ))
}

/// The picture's size at `ancho` wide, its height even.
fn a_lo_ancho(info: &MediaInfo, ancho: u32) -> Result<(u32, u32), ErrorMirada> {
    let video = info.video.as_ref().ok_or(ErrorMirada::SinImagen)?;
    let alto = f64::from(video.height) * f64::from(ancho) / f64::from(video.width.max(1));
    Ok((ancho, par(alto)))
}

fn par(valor: f64) -> u32 {
    ((valor / 2.0).round() as u32 * 2).max(2)
}

/// An RGBA frame's luma, as BT.601 weighs it.
fn gris(fotograma: &Frame) -> Vec<u8> {
    fotograma
        .pixels()
        .chunks_exact(4)
        .map(|p| {
            let luma = 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]);
            luma.round() as u8
        })
        .collect()
}
