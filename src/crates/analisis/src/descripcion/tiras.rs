//! What Claude, which sees pictures and not video, is shown: sheets of
//! four rows, each a clip's id and length over five frames spread through
//! it, the second of each written under it; a still takes a row on its
//! own. The engine's `hojas_fotogramas`.

use std::io;
use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage};

use super::{Pendiente, Tipo, numero, redondear};
use crate::lienzo::{self, Letra};
use crate::mirada::{self, ErrorMirada};
use crate::foto;

/// Frames a clip's strip shows.
pub const FOTOGRAMAS_TIRA: usize = 5;
/// Rows a sheet holds.
pub const FILAS_HOJA: usize = 4;
const ANCHO: u32 = 150;
const ALTO: u32 = 266;
/// The strip above a row's frames, for its label.
const RENGLON: u32 = 22;
const ALTO_FILA: u32 = ALTO + RENGLON;
const FONDO: Rgb<u8> = Rgb([30, 30, 30]);
const AMARILLO: Rgb<u8> = Rgb([255, 255, 0]);
const BLANCO: Rgb<u8> = Rgb([255, 255, 255]);
const NEGRO: Rgb<u8> = Rgb([0, 0, 0]);
const CALIDAD: u8 = 82;

/// The sheets for `pendientes`, in order, written into `carpeta` as
/// `hoja_01.jpg`, `hoja_02.jpg`... A frame that will not decode is left
/// black: the rest of the row still says what the clip is.
pub fn hojas(pendientes: &[Pendiente], carpeta: &Path) -> io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(carpeta)?;
    let fuente = lienzo::fuente()?;
    let rotulo = Letra { fuente: &fuente, cuerpo: 15.0, color: AMARILLO };
    let segundo = Letra { fuente: &fuente, cuerpo: 12.0, color: BLANCO };
    let mut hojas = Vec::new();
    for (n, grupo) in pendientes.chunks(FILAS_HOJA).enumerate() {
        let mut hoja = RgbImage::from_pixel(
            ANCHO * FOTOGRAMAS_TIRA as u32,
            ALTO_FILA * grupo.len() as u32,
            FONDO,
        );
        for (fila, pendiente) in grupo.iter().enumerate() {
            let y0 = fila as u32 * ALTO_FILA;
            rotulo.escribir(&mut hoja, &etiqueta(pendiente), 4.0, (y0 + 3) as f32);
            for (k, (t, cuadro)) in cuadros(pendiente).into_iter().enumerate() {
                pegar(&mut hoja, &cuadro, k as u32 * ANCHO, y0 + RENGLON);
                if let Some(t) = t {
                    marcar_segundo(&mut hoja, &segundo, t, k as u32 * ANCHO, y0 + RENGLON + ALTO);
                }
            }
        }
        let ruta = carpeta.join(format!("hoja_{:02}.jpg", n + 1));
        lienzo::guardar_jpeg(&hoja, &ruta, CALIDAD)?;
        hojas.push(ruta);
    }
    Ok(hojas)
}

/// A row's label: the id, and `(foto)` or the clip's length.
fn etiqueta(pendiente: &Pendiente) -> String {
    match pendiente.tipo {
        Tipo::Foto => format!("{}  (foto)", pendiente.id),
        Tipo::Clip { duracion, .. } => {
            format!("{}  ({} s)", pendiente.id, numero(redondear(duracion, 1)))
        }
    }
}

/// What a row shows, each at the second it was taken: a still once, a
/// clip's frames at the middle of each fifth of it.
fn cuadros(pendiente: &Pendiente) -> Vec<(Option<f64>, RgbImage)> {
    match pendiente.tipo {
        Tipo::Foto => {
            let cuadro = foto::abrir(&pendiente.ruta)
                .map(|foto| lienzo::en_caja(&foto, ANCHO, ALTO, false))
                .unwrap_or_else(|error| {
                    log::warn!("{}: still left black: {error}", pendiente.id);
                    RgbImage::new(ANCHO, ALTO)
                });
            vec![(None, cuadro)]
        }
        Tipo::Clip { duracion, .. } => {
            let momentos: Vec<f64> = (0..FOTOGRAMAS_TIRA)
                .map(|k| duracion * (k as f64 + 0.5) / FOTOGRAMAS_TIRA as f64)
                .collect();
            let mut fotogramas = fotogramas(&pendiente.ruta, &momentos).unwrap_or_else(|error| {
                log::warn!("{}: frames left black: {error}", pendiente.id);
                Vec::new()
            });
            fotogramas.resize_with(FOTOGRAMAS_TIRA, || RgbImage::new(ANCHO, ALTO));
            momentos.into_iter().map(Some).zip(fotogramas).collect()
        }
    }
}

/// The clip at `ruta`'s frames at `momentos`, fitted in a row's box.
fn fotogramas(ruta: &Path, momentos: &[f64]) -> Result<Vec<RgbImage>, ErrorMirada> {
    let info = concat_media::probe(ruta)?;
    let video = info.video.as_ref().ok_or(ErrorMirada::SinImagen)?;
    let (ancho, alto) = lienzo::encajar(video.width, video.height, ANCHO, ALTO, false);
    let (ancho, alto) = (mirada::par(f64::from(ancho)), mirada::par(f64::from(alto)));
    let mut lector = mirada::lector(ruta, ancho, alto)?;
    momentos
        .iter()
        .map(|&t| {
            let cuadro = lienzo::de_fotograma(&mirada::uno(&mut lector, t)?);
            let mut caja = RgbImage::new(ANCHO, ALTO);
            pegar(&mut caja, &cuadro, 0, 0);
            Ok(caja)
        })
        .collect()
}

/// `cuadro` centred in the `ANCHO` x `ALTO` box at (`x`, `y`).
fn pegar(hoja: &mut RgbImage, cuadro: &RgbImage, x: u32, y: u32) {
    let dx = ANCHO.saturating_sub(cuadro.width()) / 2;
    let dy = ALTO.saturating_sub(cuadro.height()) / 2;
    image::imageops::replace(hoja, cuadro, i64::from(x + dx), i64::from(y + dy));
}

/// The second `t`, in white on black, at the bottom-left of the box whose
/// bottom-left corner is (`x`, `abajo`).
fn marcar_segundo(hoja: &mut RgbImage, letra: &Letra, t: f64, x: u32, abajo: u32) {
    lienzo::rectangulo(hoja, x, abajo - 18, 53, 19, NEGRO);
    letra.escribir(hoja, &format!("{t:.1} s"), (x + 4) as f32, (abajo - 17) as f32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_is_labelled_with_its_id_and_length() {
        let clip = Pendiente {
            id: "IMG_1".into(),
            ruta: PathBuf::new(),
            tipo: Tipo::Clip { duracion: 12.34, con_voz: true },
        };
        assert_eq!(etiqueta(&clip), "IMG_1  (12.3 s)");
        let foto = Pendiente { tipo: Tipo::Foto, ..clip };
        assert_eq!(etiqueta(&foto), "IMG_1  (foto)");
    }

    #[test]
    fn four_rows_a_sheet_and_what_will_not_open_stays_black() {
        let carpeta = std::env::temp_dir().join(format!("analisis-tiras-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        let foto = carpeta.join("rojo.png");
        RgbImage::from_pixel(400, 300, Rgb([255, 0, 0])).save(&foto).unwrap();
        let mut pendientes: Vec<Pendiente> = (0..4)
            .map(|n| Pendiente {
                id: format!("F{n}"),
                ruta: foto.clone(),
                tipo: Tipo::Foto,
            })
            .collect();
        pendientes.push(Pendiente {
            id: "roto".into(),
            ruta: carpeta.join("no_existe.mov"),
            tipo: Tipo::Clip { duracion: 5.0, con_voz: false },
        });

        let hojas = hojas(&pendientes, &carpeta.join("tiras")).unwrap();

        assert_eq!(hojas.len(), 2);
        let primera = image::open(&hojas[0]).unwrap();
        assert_eq!((primera.width(), primera.height()), (750, 4 * ALTO_FILA));
        let segunda = image::open(&hojas[1]).unwrap();
        assert_eq!(segunda.height(), ALTO_FILA);
    }
}
