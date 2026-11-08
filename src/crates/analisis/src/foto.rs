//! A still opened the way it is seen: decoded whole, so a half-copied file
//! fails here and not later, and turned by its EXIF orientation - phone
//! photos are stored lying down. The engine's `medios.abrir_imagen`.

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, metadata::Orientation};

/// The still at `ruta`, upright, or why it would not open.
pub fn abrir(ruta: &Path) -> Result<DynamicImage, String> {
    let texto = |error: image::ImageError| error.to_string();
    let lector = ImageReader::open(ruta)
        .map_err(|error| error.to_string())?
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut decodificador = lector.into_decoder().map_err(texto)?;
    let orientacion = decodificador
        .orientation()
        .unwrap_or(Orientation::NoTransforms);
    let mut imagen = DynamicImage::from_decoder(decodificador).map_err(texto)?;
    imagen.apply_orientation(orientacion);
    Ok(imagen)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io;

    use super::*;

    /// A JPEG of `ancho` x `alto` whose EXIF says to turn it `orientacion`
    /// (the EXIF number).
    pub(crate) fn jpeg_con_exif(ruta: &Path, ancho: u32, alto: u32, orientacion: u16) {
        let mut jpeg = Vec::new();
        image::RgbImage::new(ancho, alto)
            .write_to(&mut io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        // "Exif", a little-endian TIFF header and one IFD with Orientation.
        let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        exif.extend_from_slice(&orientacion.to_le_bytes());
        exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let largo = (exif.len() + 2) as u16;
        let mut con_exif = jpeg[..2].to_vec();
        con_exif.extend_from_slice(&[0xFF, 0xE1]);
        con_exif.extend_from_slice(&largo.to_be_bytes());
        con_exif.extend_from_slice(&exif);
        con_exif.extend_from_slice(&jpeg[2..]);
        std::fs::write(ruta, con_exif).unwrap();
    }

    fn carpeta(nombre: &str) -> std::path::PathBuf {
        let carpeta =
            std::env::temp_dir().join(format!("analisis-foto-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    #[test]
    fn a_photo_stored_lying_down_opens_standing() {
        let carpeta = carpeta("exif");
        for (orientacion, medida) in [(1, (8, 4)), (3, (8, 4)), (6, (4, 8)), (8, (4, 8))] {
            let ruta = carpeta.join(format!("foto{orientacion}.jpg"));
            jpeg_con_exif(&ruta, 8, 4, orientacion);
            let foto = abrir(&ruta).unwrap();
            assert_eq!((foto.width(), foto.height()), medida, "EXIF {orientacion}");
        }
    }

    #[test]
    fn a_half_copied_photo_will_not_open() {
        let ruta = carpeta("media").join("foto.png");
        image::RgbImage::new(64, 64).save(&ruta).unwrap();
        let bytes = std::fs::read(&ruta).unwrap();
        std::fs::write(&ruta, &bytes[..bytes.len() / 2]).unwrap();
        assert!(abrir(&ruta).is_err());
    }
}
