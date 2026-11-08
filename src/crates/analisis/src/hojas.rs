//! Contact sheets: strips of labelled frames, for the AI that plans the
//! montage to see the material at a glance - the port of the engine's
//! `motor/hojas.py`. Each cell is a clip's label and its three frames from
//! the analysis; each still has a cell of its own. Twelve cells a sheet,
//! four across, into the job's `hojas/hoja_01.jpg`, `hoja_02.jpg`...

use std::io;
use std::path::{Path, PathBuf};

use image::{Rgb, RgbImage};
use serde::Deserialize;

use crate::foto;
use crate::inventario::Inventario;
use crate::lienzo::{self, Letra};

/// A frame's box in a cell: the stills fit in it, centred on black.
const ANCHO: u32 = 120;
const ALTO: u32 = 213;
/// Frames shown per clip.
const POR_CELDA: usize = 3;
/// The strip above the frames, for the label.
const RENGLON: u32 = 20;
const ANCHO_CELDA: u32 = ANCHO * POR_CELDA as u32 + 8;
const ALTO_CELDA: u32 = ALTO + RENGLON;
const CELDAS_POR_HOJA: usize = 12;
const COLUMNAS: usize = 4;
const FONDO: Rgb<u8> = Rgb([40, 40, 40]);
const LETRA: Rgb<u8> = Rgb([255, 255, 0]);
const CUERPO: f32 = 13.0;
const CALIDAD: u8 = 85;

/// One cell: a label and the frames under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Celda {
    /// What it is: the id, the length and how much of it is speech.
    pub etiqueta: String,
    /// The frames, at most three of them shown.
    pub fotogramas: Vec<PathBuf>,
}

/// The cells of the job in `trabajo`: its analysed clips, in the order of
/// `analisis.json`, then the stills of `inventario`.
pub fn celdas(trabajo: &Path, inventario: &Inventario) -> io::Result<Vec<Celda>> {
    let analisis = std::fs::read_to_string(trabajo.join("analisis.json"))?;
    let analisis: Analisis = serde_json::from_str(&analisis).map_err(io::Error::other)?;
    let clips = analisis.clips.into_iter().map(|clip| Celda {
        etiqueta: format!(
            "{} {:.1}s voz {:.0}%",
            clip.id,
            clip.duracion,
            clip.ratio_voz * 100.0
        ),
        fotogramas: clip
            .fotogramas
            .into_iter()
            .map(|f| trabajo.join(f.ruta))
            .collect(),
    });
    let fotos = inventario.imagenes.iter().map(|imagen| Celda {
        etiqueta: format!("{} (foto)", imagen.id),
        fotogramas: vec![imagen.ruta.clone()],
    });
    Ok(clips.chain(fotos).collect())
}

/// Draws `celdas` into `trabajo/hojas` and returns the sheets, in order.
/// A frame that will not open leaves its box black: the sheet is a
/// glance, and the rest of it still helps.
pub fn generar(trabajo: &Path, celdas: &[Celda]) -> io::Result<Vec<PathBuf>> {
    let carpeta = trabajo.join("hojas");
    std::fs::create_dir_all(&carpeta)?;
    let fuente = lienzo::fuente()?;
    let letra = Letra {
        fuente: &fuente,
        cuerpo: CUERPO,
        color: LETRA,
    };
    let mut hojas = Vec::new();
    for (n, grupo) in celdas.chunks(CELDAS_POR_HOJA).enumerate() {
        let ruta = carpeta.join(format!("hoja_{:02}.jpg", n + 1));
        lienzo::guardar_jpeg(&dibujar_hoja(grupo, &letra), &ruta, CALIDAD)?;
        hojas.push(ruta);
    }
    Ok(hojas)
}

/// `analisis.json`, as far as the sheets read it.
#[derive(Deserialize)]
struct Analisis {
    #[serde(default)]
    clips: Vec<ClipAnalizado>,
}

#[derive(Deserialize)]
struct ClipAnalizado {
    id: String,
    #[serde(default)]
    duracion: f64,
    #[serde(default)]
    ratio_voz: f64,
    #[serde(default)]
    fotogramas: Vec<Fotograma>,
}

#[derive(Deserialize)]
struct Fotograma {
    /// Relative to the job folder.
    ruta: PathBuf,
}

/// One sheet of up to [`CELDAS_POR_HOJA`] cells.
fn dibujar_hoja(celdas: &[Celda], letra: &Letra) -> RgbImage {
    let filas = celdas.len().div_ceil(COLUMNAS) as u32;
    let mut hoja = RgbImage::from_pixel(COLUMNAS as u32 * ANCHO_CELDA, filas * ALTO_CELDA, FONDO);
    for (k, celda) in celdas.iter().enumerate() {
        let x0 = (k % COLUMNAS) as u32 * ANCHO_CELDA;
        let y0 = (k / COLUMNAS) as u32 * ALTO_CELDA;
        letra.escribir(&mut hoja, &celda.etiqueta, (x0 + 3) as f32, (y0 + 2) as f32);
        for (j, ruta) in celda.fotogramas.iter().take(POR_CELDA).enumerate() {
            let caja = miniatura(ruta);
            image::imageops::replace(
                &mut hoja,
                &caja,
                i64::from(x0 + j as u32 * ANCHO),
                i64::from(y0 + RENGLON),
            );
        }
    }
    hoja
}

/// A frame fitted in its box - shrunk, never enlarged - and centred on
/// black. Black alone when it will not open.
fn miniatura(ruta: &Path) -> RgbImage {
    match foto::abrir(ruta) {
        Ok(foto) => lienzo::en_caja(&foto, ANCHO, ALTO, false),
        Err(error) => {
            log::warn!("hojas: {} left black: {error}", ruta.display());
            RgbImage::new(ANCHO, ALTO)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventario::Imagen;

    fn carpeta(nombre: &str) -> PathBuf {
        let carpeta =
            std::env::temp_dir().join(format!("analisis-hojas-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    #[test]
    fn a_clip_s_cell_says_its_length_and_its_speech() {
        let trabajo = carpeta("celdas");
        std::fs::write(
            trabajo.join("analisis.json"),
            r#"{"clips": [{"id": "A", "duracion": 12.34, "ratio_voz": 0.456,
                "fotogramas": [{"ruta": "fotogramas/A_0.jpg"}, {"ruta": "fotogramas/A_1.jpg"}]}]}"#,
        )
        .unwrap();
        let inventario = Inventario {
            imagenes: vec![Imagen {
                id: "logo".into(),
                ruta: "M/logo.png".into(),
                ..Imagen::default()
            }],
            ..Inventario::default()
        };
        let celdas = celdas(&trabajo, &inventario).unwrap();
        assert_eq!(celdas[0].etiqueta, "A 12.3s voz 46%");
        assert_eq!(celdas[0].fotogramas[1], trabajo.join("fotogramas/A_1.jpg"));
        assert_eq!(celdas[1].etiqueta, "logo (foto)");
        assert_eq!(celdas[1].fotogramas, [PathBuf::from("M/logo.png")]);
    }

    #[test]
    fn a_still_is_shrunk_to_its_box_and_never_enlarged() {
        let encajar = |ancho, alto| lienzo::encajar(ancho, alto, ANCHO, ALTO, false);
        assert_eq!(encajar(1080, 1920), (120, 213));
        assert_eq!(encajar(1920, 1080), (120, 68));
        assert_eq!(encajar(50, 40), (50, 40));
    }

    #[test]
    fn cells_go_twelve_a_sheet_four_across() {
        let trabajo = carpeta("generar");
        let foto = trabajo.join("rojo.png");
        RgbImage::from_pixel(1080, 1920, Rgb([255, 0, 0]))
            .save(&foto)
            .unwrap();
        let celdas: Vec<Celda> = (0..13)
            .map(|n| Celda {
                etiqueta: format!("clip{n} 3.0s voz 50%"),
                fotogramas: vec![foto.clone(), trabajo.join("falta.jpg")],
            })
            .collect();

        let hojas = generar(&trabajo, &celdas).unwrap();

        assert_eq!(
            hojas,
            [
                trabajo.join("hojas/hoja_01.jpg"),
                trabajo.join("hojas/hoja_02.jpg")
            ]
        );
        let primera = image::open(&hojas[0]).unwrap().to_rgb8();
        assert_eq!(primera.dimensions(), (4 * 368, 3 * 233));
        let segunda = image::open(&hojas[1]).unwrap().to_rgb8();
        assert_eq!(segunda.dimensions(), (4 * 368, 233));
        // The first frame is red, the missing second one black, the empty
        // third slot the sheet's grey.
        let [r, g, _] = primera.get_pixel(60, 120).0;
        assert!(r > 200 && g < 60);
        assert!(primera.get_pixel(180, 120).0.iter().all(|&c| c < 30));
        assert!(
            primera
                .get_pixel(300, 120)
                .0
                .iter()
                .all(|&c| (25..=55).contains(&c))
        );
        // The label is written in yellow.
        let amarillo = (0..120)
            .flat_map(|x| (0..20).map(move |y| (x, y)))
            .any(|(x, y)| {
                let [r, g, b] = primera.get_pixel(x, y).0;
                r > 180 && g > 180 && b < 90
            });
        assert!(amarillo);
    }
}
