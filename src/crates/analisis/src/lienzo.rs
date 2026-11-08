//! Drawing the pictures the AI is shown - the contact sheets, the strips
//! of frames, the contact video's labels: text in the interface's face, a
//! picture fitted in a box, a sheet saved as JPEG.

use std::io;
use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::{DynamicImage, ImageEncoder, Rgb, RgbImage, codecs::jpeg::JpegEncoder};

/// The labels' face: the interface's, bundled with the app. Under the SIL
/// Open Font License; see `crates/concat-text/fonts/LICENSE-HankenGrotesk.txt`.
const FUENTE: &[u8] = include_bytes!("../../concat-text/fonts/HankenGrotesk-Bold.ttf");

/// The labels' face.
pub(crate) fn fuente() -> io::Result<FontRef<'static>> {
    FontRef::try_from_slice(FUENTE).map_err(io::Error::other)
}

/// Text of one size and colour.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Letra<'f> {
    /// The face.
    pub fuente: &'f FontRef<'static>,
    /// Pixels from ascender to descender.
    pub cuerpo: f32,
    /// Its colour.
    pub color: Rgb<u8>,
}

impl Letra<'_> {
    /// How wide `texto` runs, in pixels.
    #[cfg_attr(not(feature = "imagen"), allow(dead_code))]
    pub fn ancho(&self, texto: &str) -> f32 {
        let fuente = self.fuente.as_scaled(PxScale::from(self.cuerpo));
        let mut ancho = 0.0;
        let mut anterior = None;
        for c in texto.chars() {
            let id = fuente.glyph_id(c);
            if let Some(anterior) = anterior {
                ancho += fuente.kern(anterior, id);
            }
            ancho += fuente.h_advance(id);
            anterior = Some(id);
        }
        ancho
    }

    /// How tall a line is, in pixels.
    #[cfg_attr(not(feature = "imagen"), allow(dead_code))]
    pub fn alto(&self) -> f32 {
        self.fuente.as_scaled(PxScale::from(self.cuerpo)).height()
    }

    /// Writes `texto` with its top-left at (`x`, `y`); what runs past the
    /// picture's edge is cut.
    pub fn escribir(&self, lienzo: &mut RgbImage, texto: &str, x: f32, y: f32) {
        let fuente = self.fuente.as_scaled(PxScale::from(self.cuerpo));
        let mut pluma = point(x, y + fuente.ascent());
        let mut anterior = None;
        for c in texto.chars() {
            let id = fuente.glyph_id(c);
            if let Some(anterior) = anterior {
                pluma.x += fuente.kern(anterior, id);
            }
            let glifo = id.with_scale_and_position(fuente.scale(), pluma);
            pluma.x += fuente.h_advance(id);
            anterior = Some(id);
            if let Some(contorno) = fuente.outline_glyph(glifo) {
                let caja = contorno.px_bounds();
                contorno.draw(|gx, gy, cobertura| {
                    let px = caja.min.x as i64 + i64::from(gx);
                    let py = caja.min.y as i64 + i64::from(gy);
                    mezclar(lienzo, px, py, self.color, cobertura);
                });
            }
        }
    }
}

/// `color` over the pixel at (`x`, `y`) by `cobertura`, 0 to 1; nothing
/// off the picture.
fn mezclar(lienzo: &mut RgbImage, x: i64, y: i64, color: Rgb<u8>, cobertura: f32) {
    let dentro = x >= 0 && y >= 0 && x < i64::from(lienzo.width()) && y < i64::from(lienzo.height());
    if !dentro {
        return;
    }
    let pixel = lienzo.get_pixel_mut(x as u32, y as u32);
    for (canal, nuevo) in pixel.0.iter_mut().zip(color.0) {
        let mezcla = f32::from(*canal) * (1.0 - cobertura) + f32::from(nuevo) * cobertura;
        *canal = mezcla.round() as u8;
    }
}

/// A solid rectangle, cut to the picture.
#[cfg_attr(not(feature = "imagen"), allow(dead_code))]
pub(crate) fn rectangulo(lienzo: &mut RgbImage, x: u32, y: u32, ancho: u32, alto: u32, color: Rgb<u8>) {
    let (hasta_x, hasta_y) = ((x + ancho).min(lienzo.width()), (y + alto).min(lienzo.height()));
    for py in y..hasta_y {
        for px in x..hasta_x {
            lienzo.put_pixel(px, py, color);
        }
    }
}

/// The size `ancho` x `alto` takes in a `caja_ancho` x `caja_alto` box,
/// its proportions kept; never larger than it is unless `agrandar`.
pub(crate) fn encajar(ancho: u32, alto: u32, caja_ancho: u32, caja_alto: u32, agrandar: bool) -> (u32, u32) {
    let escala = (f64::from(caja_ancho) / f64::from(ancho.max(1))).min(f64::from(caja_alto) / f64::from(alto.max(1)));
    let escala = if agrandar { escala } else { escala.min(1.0) };
    let lado = |valor: u32| ((f64::from(valor) * escala).round() as u32).max(1);
    (lado(ancho), lado(alto))
}

/// `foto` fitted in a `ancho` x `alto` box and centred on black.
pub(crate) fn en_caja(foto: &DynamicImage, ancho: u32, alto: u32, agrandar: bool) -> RgbImage {
    let mut caja = RgbImage::new(ancho, alto);
    let (w, h) = encajar(foto.width(), foto.height(), ancho, alto, agrandar);
    let reducida = foto
        .resize_exact(w, h, image::imageops::FilterType::Triangle)
        .to_rgb8();
    image::imageops::replace(&mut caja, &reducida, i64::from((ancho - w) / 2), i64::from((alto - h) / 2));
    caja
}

/// A decoded frame's picture, its alpha dropped.
#[cfg(feature = "imagen")]
pub(crate) fn de_fotograma(fotograma: &concat_core::Frame) -> RgbImage {
    let rgb = fotograma.pixels().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    RgbImage::from_raw(fotograma.width(), fotograma.height(), rgb).unwrap_or_default()
}

/// `lienzo` as a frame to encode, opaque.
#[cfg(feature = "imagen")]
pub(crate) fn a_fotograma(lienzo: &RgbImage) -> concat_core::Frame {
    let rgba = lienzo.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    concat_core::Frame::from_rgba(lienzo.width(), lienzo.height(), rgba)
        .unwrap_or_else(|| concat_core::Frame::black(lienzo.width(), lienzo.height()))
}

/// Saves `lienzo` as a JPEG of `calidad` (1-100).
pub(crate) fn guardar_jpeg(lienzo: &RgbImage, ruta: &Path, calidad: u8) -> io::Result<()> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, calidad)
        .write_image(
            lienzo.as_raw(),
            lienzo.width(),
            lienzo.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(io::Error::other)?;
    std::fs::write(ruta, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_shrinks_to_its_box_and_grows_only_when_asked() {
        assert_eq!(encajar(1920, 1080, 120, 213, false), (120, 68));
        assert_eq!(encajar(50, 100, 120, 213, false), (50, 100));
        assert_eq!(encajar(50, 100, 270, 480, true), (240, 480));
    }

    #[test]
    fn text_is_drawn_in_its_colour_and_measured() {
        let fuente = fuente().unwrap();
        let letra = Letra {
            fuente: &fuente,
            cuerpo: 20.0,
            color: Rgb([255, 255, 0]),
        };
        let mut lienzo = RgbImage::new(100, 30);
        letra.escribir(&mut lienzo, "Hola", 2.0, 2.0);
        assert!(lienzo.pixels().any(|p| p.0 == [255, 255, 0]));
        assert!(letra.ancho("Hola") > letra.ancho("H"));
        assert!(letra.ancho("Hola") < 100.0);
    }
}
