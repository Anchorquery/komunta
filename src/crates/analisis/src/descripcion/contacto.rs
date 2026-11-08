//! What Gemini, which watches video, is shown: a "contact video" - the
//! clips and stills one after the other, small, silent, at two frames a
//! second, each with its id written on top - so a single request describes
//! them all. The engine's `video_contacto`, drawn and encoded here rather
//! than by the ffmpeg command line.

use std::path::{Path, PathBuf};

use concat_core::{FrameRate, Rational};
use concat_media::{DecodeOptions, Decoder, EncodeOptions, Encoder, FrameSink, FrameSource};
use image::{Rgb, RgbImage};

use super::respuesta::Tramo;
use super::{Pendiente, Tipo, redondear};
use crate::foto;
use crate::lienzo::{self, Letra};
use crate::mirada::ErrorMirada;

/// The contact video's size: a phone held upright, small.
pub const ANCHO: u32 = 270;
/// See [`ANCHO`].
pub const ALTO: u32 = 480;
/// Its frames a second, which the request tells Gemini.
pub const FPS: u32 = 2;
/// The label's size, before it is shrunk to fit.
const CUERPO: f32 = 22.0;
const AMARILLO: Rgb<u8> = Rgb([255, 255, 0]);
const NEGRO: Rgb<u8> = Rgb([0, 0, 0]);

/// A contact video and where each clip is in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Contacto {
    /// The MP4.
    pub ruta: PathBuf,
    /// Each clip's id and its stretch of the video, in order.
    pub tabla: Vec<(String, Tramo)>,
}

/// Writes the contact video of `lote` to `ruta`: each clip for its
/// [`Pendiente::segundos_en_contacto`], from its start. A clip that runs
/// out early holds its last frame; one that will not decode at all, or a
/// still that will not open, is black under its label.
pub fn video(lote: &[Pendiente], ruta: &Path) -> Result<Contacto, ErrorMirada> {
    if let Some(carpeta) = ruta.parent() {
        std::fs::create_dir_all(carpeta)?;
    }
    let opciones = EncodeOptions {
        preset: "veryfast".into(),
        crf: 32,
        hardware: false,
        ..EncodeOptions::default()
    };
    let mut salida = Encoder::create(ruta, ANCHO, ALTO, FrameRate::new(Rational::new(FPS.into(), 1)), &opciones)?;
    let fuente = lienzo::fuente()?;
    let letra = Letra { fuente: &fuente, cuerpo: CUERPO, color: AMARILLO };
    let mut tabla = Vec::new();
    let mut escritos = 0u64;
    for pendiente in lote {
        let cuantos = cuadros_de(pendiente.segundos_en_contacto());
        let mut cuadros = Cuadros::abrir(pendiente);
        for _ in 0..cuantos {
            let mut cuadro = cuadros.siguiente();
            rotular(&mut cuadro, &letra, &pendiente.id);
            salida.write_frame(&lienzo::a_fotograma(&cuadro))?;
        }
        let desde = segundo(escritos);
        escritos += cuantos;
        tabla.push((pendiente.id.clone(), Tramo { desde, hasta: segundo(escritos) }));
    }
    salida.finish()?;
    Ok(Contacto { ruta: ruta.to_path_buf(), tabla })
}

/// The frames that show `segundos` of a clip: every half second from its
/// start, as ffmpeg's `fps` filter takes them; at least one.
fn cuadros_de(segundos: f64) -> u64 {
    ((segundos * f64::from(FPS)).ceil() as u64).max(1)
}

/// The contact video's second at frame `n`.
fn segundo(n: u64) -> f64 {
    redondear(n as f64 / f64::from(FPS), 2)
}

/// Where a clip's frames come from.
enum Cuadros {
    /// A clip being decoded, and the last frame it gave.
    Clip { lector: Option<Box<Decoder>>, ultimo: RgbImage },
    /// A still, the same frame throughout.
    Foto(RgbImage),
}

impl Cuadros {
    fn abrir(pendiente: &Pendiente) -> Self {
        let negro = RgbImage::new(ANCHO, ALTO);
        match pendiente.tipo {
            Tipo::Foto => Cuadros::Foto(
                foto::abrir(&pendiente.ruta)
                    .map(|foto| lienzo::en_caja(&foto, ANCHO, ALTO, true))
                    .unwrap_or_else(|error| {
                        log::warn!("{}: still left black: {error}", pendiente.id);
                        negro
                    }),
            ),
            Tipo::Clip { .. } => {
                let lector = lector(&pendiente.ruta)
                    .map(Box::new)
                    .inspect_err(|error| log::warn!("{}: clip left black: {error}", pendiente.id))
                    .ok();
                Cuadros::Clip { lector, ultimo: negro }
            }
        }
    }

    fn siguiente(&mut self) -> RgbImage {
        match self {
            Cuadros::Foto(foto) => foto.clone(),
            Cuadros::Clip { lector, ultimo } => {
                if let Some(fotograma) = lector.as_mut().and_then(|l| l.next_frame().ok().flatten()) {
                    *ultimo = lienzo::de_fotograma(&fotograma);
                } else {
                    *lector = None;
                }
                ultimo.clone()
            }
        }
    }
}

/// A reader of the clip at `ruta`, fitted in the contact video's frame on
/// black, at its rate.
fn lector(ruta: &Path) -> Result<Decoder, ErrorMirada> {
    let encaje = format!(
        "scale={ANCHO}:{ALTO}:force_original_aspect_ratio=decrease,\
         pad={ANCHO}:{ALTO}:(ow-iw)/2:(oh-ih)/2"
    );
    let opciones = DecodeOptions::default()
        .prefiltered(encaje)
        .scaled_to(ANCHO, ALTO)
        .at_rate(FrameRate::new(Rational::new(FPS.into(), 1)));
    Ok(Decoder::open(ruta, &opciones)?)
}

/// `id` on top of the frame, centred, yellow on a black box, shrunk until
/// it fits across.
fn rotular(cuadro: &mut RgbImage, letra: &Letra, id: &str) {
    let mut letra = *letra;
    let cabe = (ANCHO - 12) as f32;
    while letra.ancho(id) > cabe && letra.cuerpo > 10.0 {
        letra.cuerpo -= 1.0;
    }
    let (ancho, alto) = (letra.ancho(id).min(cabe), letra.alto());
    let x = (ANCHO as f32 - ancho) / 2.0;
    let y = 6.0;
    lienzo::rectangulo(cuadro, (x - 3.0) as u32, (y - 2.0) as u32, (ancho + 6.0) as u32, (alto + 4.0) as u32, NEGRO);
    letra.escribir(cuadro, id, x, y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_takes_a_frame_every_half_second() {
        assert_eq!(cuadros_de(12.0), 24);
        assert_eq!(cuadros_de(5.3), 11);
        assert_eq!(cuadros_de(0.0), 1);
        assert_eq!(segundo(11), 5.5);
    }

    #[test]
    fn a_long_id_is_shrunk_to_fit_the_frame() {
        let fuente = lienzo::fuente().unwrap();
        let letra = Letra { fuente: &fuente, cuerpo: CUERPO, color: AMARILLO };
        let mut cuadro = RgbImage::new(ANCHO, ALTO);
        rotular(&mut cuadro, &letra, "PORTADARAKO_ARGAZKIA_NAGUSIA_2");
        let amarillo_al_borde = (0..40).any(|y| cuadro.get_pixel(0, y).0 == AMARILLO.0);
        assert!(!amarillo_al_borde);
        assert!(cuadro.pixels().any(|p| p.0 == AMARILLO.0));
    }
}
