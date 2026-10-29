//! Where the faces are in a picture, with YuNet: OpenCV's small face
//! detector, which finds the faces of a crowd that the engine's old Haar
//! cascade missed (four or five a frame where Haar saw none or one, on the
//! team's own footage).
//!
//! The model takes a 640 × 640 BGR picture and answers, for each cell of
//! three grids (8, 16 and 32 pixels), how likely a face is there and its
//! box. The picture is scaled to fit and padded; the boxes are read back
//! the way OpenCV's `FaceDetectorYN` reads them, checked against it to the
//! thousandth, and overlapping ones are merged.

use crate::visual::Cara;

/// The side of the picture the model takes.
pub const LADO: usize = 640;

/// The grids the model answers on, by cell size in pixels.
pub const PASOS: [usize; 3] = [8, 16, 32];

/// A face this likely or more is kept. OpenCV's own default is 0.9; the
/// team's crowds have real faces down to about 0.6.
pub const UMBRAL: f32 = 0.7;

/// Boxes that overlap more than this are the same face.
const SOLAPE: f32 = 0.3;

/// The model's answer for one grid: per cell, the class and objectness
/// scores and the box (`dx, dy, log w, log h`, in cells).
pub struct Rejilla<'a> {
    /// Cell size in pixels: 8, 16 or 32.
    pub paso: usize,
    /// Per cell, how face-like.
    pub clase: &'a [f32],
    /// Per cell, how likely anything is there.
    pub objeto: &'a [f32],
    /// Per cell, four numbers: the box.
    pub caja: &'a [f32],
}

/// A picture made ready for the model, and how to read its boxes back.
pub struct Entrada {
    /// BGR planes, 640 × 640 each, 0 to 255.
    pub planos: Vec<f32>,
    /// The picture's size, and the scale it was fitted to the square at.
    ancho: usize,
    alto: usize,
    escala: f64,
}

impl Entrada {
    /// Fits an RGBA picture into the model's square: scaled down (or up)
    /// to fit, top-left, the rest black.
    pub fn nueva(rgba: &[u8], ancho: usize, alto: usize) -> Self {
        let escala = LADO as f64 / ancho.max(alto).max(1) as f64;
        let (dentro_ancho, dentro_alto) = (
            ((ancho as f64 * escala).round() as usize).min(LADO),
            ((alto as f64 * escala).round() as usize).min(LADO),
        );
        let mut planos = vec![0.0; 3 * LADO * LADO];
        for y in 0..dentro_alto {
            let origen_y = ((y as f64 + 0.5) / escala).floor() as usize;
            for x in 0..dentro_ancho {
                let origen_x = ((x as f64 + 0.5) / escala).floor() as usize;
                let pixel = (origen_y.min(alto - 1) * ancho + origen_x.min(ancho - 1)) * 4;
                let [r, g, b] = [rgba[pixel], rgba[pixel + 1], rgba[pixel + 2]];
                // Blue, green, red: the order OpenCV hands the model.
                for (plano, valor) in [b, g, r].into_iter().enumerate() {
                    planos[plano * LADO * LADO + y * LADO + x] = f32::from(valor);
                }
            }
        }
        Self {
            planos,
            ancho,
            alto,
            escala,
        }
    }

    /// The faces in the model's answer, most likely first, as fractions of
    /// the picture.
    pub fn caras(&self, rejillas: &[Rejilla]) -> Vec<Cara> {
        let mut candidatas: Vec<(Caja, f32)> = rejillas.iter().flat_map(candidatas).collect();
        candidatas.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut elegidas: Vec<Caja> = Vec::new();
        for (caja, _) in candidatas {
            if elegidas.iter().all(|otra| caja.solape(otra) < SOLAPE) {
                elegidas.push(caja);
            }
        }
        elegidas.iter().map(|caja| self.relativa(caja)).collect()
    }

    fn relativa(&self, caja: &Caja) -> Cara {
        let fraccion = |valor: f32, de: usize| {
            let valor = f64::from(valor) / self.escala / de as f64;
            (valor * 1000.0).round() / 1000.0
        };
        Cara {
            x: fraccion(caja.x, self.ancho),
            y: fraccion(caja.y, self.alto),
            ancho: fraccion(caja.ancho, self.ancho),
            alto: fraccion(caja.alto, self.alto),
        }
    }
}

/// A box in the model's square, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Caja {
    x: f32,
    y: f32,
    ancho: f32,
    alto: f32,
}

impl Caja {
    fn solape(&self, otra: &Caja) -> f32 {
        let ancho = (self.x + self.ancho).min(otra.x + otra.ancho) - self.x.max(otra.x);
        let alto = (self.y + self.alto).min(otra.y + otra.alto) - self.y.max(otra.y);
        let comun = ancho.max(0.0) * alto.max(0.0);
        comun / (self.ancho * self.alto + otra.ancho * otra.alto - comun)
    }
}

/// One grid's boxes likely enough to be faces, with their score.
fn candidatas<'a>(rejilla: &'a Rejilla) -> impl Iterator<Item = (Caja, f32)> + 'a {
    let columnas = LADO / rejilla.paso;
    let paso = rejilla.paso as f32;
    (0..rejilla.clase.len()).filter_map(move |celda| {
        let probable = rejilla.clase[celda].clamp(0.0, 1.0) * rejilla.objeto[celda].clamp(0.0, 1.0);
        let puntuacion = probable.sqrt();
        if puntuacion < UMBRAL {
            return None;
        }
        let [dx, dy, ancho, alto] = rejilla.caja[celda * 4..celda * 4 + 4] else {
            return None;
        };
        let (fila, columna) = ((celda / columnas) as f32, (celda % columnas) as f32);
        let (ancho, alto) = (ancho.exp() * paso, alto.exp() * paso);
        let (cx, cy) = ((columna + dx) * paso, (fila + dy) * paso);
        Some((
            Caja {
                x: cx - ancho / 2.0,
                y: cy - alto / 2.0,
                ancho,
                alto,
            },
            puntuacion,
        ))
    })
}

/// Something that finds faces in a picture: YuNet's `Detector` in the app, a
/// stand-in in tests.
pub trait BuscaCaras {
    /// The faces in an RGBA picture, most likely first.
    fn caras(&mut self, rgba: &[u8], ancho: usize, alto: usize) -> Result<Vec<Cara>, String>;
}

#[cfg(feature = "vision")]
pub use detector::Detector;

#[cfg(feature = "vision")]
mod detector {
    use concat_vision::runtime::{Input, Model};

    use super::{BuscaCaras, Entrada, LADO, PASOS, Rejilla};
    use crate::visual::Cara;

    /// The model, compiled in: 232 KB, MIT, from OpenCV's model zoo
    /// (`face_detection_yunet_2023mar.onnx`).
    const YUNET: &[u8] = include_bytes!("../models/face_detection_yunet_2023mar.onnx");

    /// YuNet, loaded.
    pub struct Detector {
        modelo: Model,
    }

    impl Detector {
        /// Loads the compiled-in model.
        pub fn new() -> Result<Self, String> {
            Ok(Self {
                modelo: Model::from_bytes(YUNET)?,
            })
        }
    }

    impl BuscaCaras for Detector {
        fn caras(&mut self, rgba: &[u8], ancho: usize, alto: usize) -> Result<Vec<Cara>, String> {
            let entrada = Entrada::nueva(rgba, ancho, alto);
            let nombres: Vec<String> = PASOS
                .iter()
                .flat_map(|paso| ["cls", "obj", "bbox"].map(|que| format!("{que}_{paso}")))
                .collect();
            let pedidas: Vec<&str> = nombres.iter().map(String::as_str).collect();
            let salidas = self.modelo.run(
                vec![Input {
                    name: "input",
                    dims: vec![1, 3, LADO, LADO],
                    data: entrada.planos.clone().into(),
                }],
                &pedidas,
            )?;
            let rejillas: Vec<Rejilla> = PASOS
                .iter()
                .zip(salidas.chunks(3))
                .map(|(paso, tres)| Rejilla {
                    paso: *paso,
                    clase: &tres[0].data,
                    objeto: &tres[1].data,
                    caja: &tres[2].data,
                })
                .collect();
            Ok(entrada.caras(&rejillas))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One 32-pixel grid with a single likely cell.
    fn una_cara(celda: usize, caja: [f32; 4], clase: f32) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let celdas = (LADO / 32).pow(2);
        let mut clases = vec![0.0; celdas];
        let objetos = vec![1.0; celdas];
        let mut cajas = vec![0.0; celdas * 4];
        clases[celda] = clase;
        cajas[celda * 4..celda * 4 + 4].copy_from_slice(&caja);
        (clases, objetos, cajas)
    }

    #[test]
    fn a_landscape_picture_fills_the_width_and_is_padded_below() {
        let rgba = [10, 20, 30, 255].repeat(1280 * 720);
        let entrada = Entrada::nueva(&rgba, 1280, 720);
        let plano = LADO * LADO;
        assert_eq!(entrada.planos[0], 30.0, "blue first");
        assert_eq!(entrada.planos[2 * plano], 10.0, "red last");
        assert_eq!(
            entrada.planos[359 * LADO + 639],
            30.0,
            "the last row of the picture"
        );
        assert_eq!(entrada.planos[360 * LADO], 0.0, "padding below it");
    }

    #[test]
    fn a_box_is_read_back_as_opencv_reads_it() {
        let rgba = vec![0; 1280 * 720 * 4];
        let entrada = Entrada::nueva(&rgba, 1280, 720);
        // Cell (row 2, column 3) of the 32-pixel grid, a box 64 × 32
        // centred half a cell right of it.
        let (clases, objetos, cajas) = una_cara(2 * 20 + 3, [0.5, 0.0, 2f32.ln(), 0.0], 0.81);
        let rejilla = Rejilla {
            paso: 32,
            clase: &clases,
            objeto: &objetos,
            caja: &cajas,
        };
        let caras = entrada.caras(&[rejilla]);
        // Centre (3.5 × 32, 2 × 32) = (112, 64) and 64 × 32 in the square:
        // corner (80, 48). The picture is twice the square's scale.
        assert_eq!(
            caras,
            [Cara {
                x: 0.125,
                y: 0.133,
                ancho: 0.1,
                alto: 0.089
            }]
        );
    }

    #[test]
    fn unlikely_boxes_are_dropped() {
        let entrada = Entrada::nueva(&[0; 4 * 4 * 4], 4, 4);
        let (clases, objetos, cajas) = una_cara(5, [0.0; 4], 0.4);
        let rejilla = Rejilla {
            paso: 32,
            clase: &clases,
            objeto: &objetos,
            caja: &cajas,
        };
        assert!(
            entrada.caras(&[rejilla]).is_empty(),
            "sqrt(0.4) is under the threshold"
        );
    }

    #[test]
    fn overlapping_boxes_are_one_face() {
        let a = Caja {
            x: 0.0,
            y: 0.0,
            ancho: 10.0,
            alto: 10.0,
        };
        let b = Caja {
            x: 1.0,
            y: 0.0,
            ancho: 10.0,
            alto: 10.0,
        };
        let c = Caja {
            x: 20.0,
            y: 0.0,
            ancho: 10.0,
            alto: 10.0,
        };
        assert!(a.solape(&b) > SOLAPE);
        assert_eq!(a.solape(&c), 0.0);
    }
}
