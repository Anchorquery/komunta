//! What a clip looks like over time - how sharp, how still, how bright,
//! sampled a couple of times a second - and the stretch of it worth
//! showing when it is used as footage rather than as a take.

use serde::{Deserialize, Serialize};

/// Samples per second the clip is looked at.
pub const FPS_MUESTREO: f64 = 2.0;

/// The width a sample is looked at; the height keeps 9:16, cropped.
pub const ANCHO: u32 = 96;
/// See [`ANCHO`].
pub const ALTO: u32 = ANCHO * 16 / 9;

/// A footage stretch lasts this long, in seconds.
pub const LARGO_TRAMO: f64 = 2.0;

/// Below this mean grey a sample is too dark to show.
const OSCURO: f64 = 30.0;
/// Above it, blown out.
const QUEMADO: f64 = 235.0;

/// One sample: its grey pixels, row by row, [`ANCHO`] × [`ALTO`] or any
/// other size.
#[derive(Clone, Copy, Debug)]
pub struct Gris<'a> {
    /// Width in pixels.
    pub ancho: usize,
    /// Height in pixels.
    pub alto: usize,
    /// Grey, row by row.
    pub pixeles: &'a [u8],
}

impl Gris<'_> {
    fn en(&self, x: usize, y: usize) -> f64 {
        f64::from(self.pixeles[y * self.ancho + x])
    }

    /// How sharp: the variance of the Laplacian over the inner pixels, as
    /// OpenCV's `Laplacian(..., ksize=1).var()` measures it.
    pub fn nitidez(&self) -> f64 {
        if self.ancho < 3 || self.alto < 3 {
            return 0.0;
        }
        let valores: Vec<f64> = (1..self.alto - 1)
            .flat_map(|y| (1..self.ancho - 1).map(move |x| (x, y)))
            .map(|(x, y)| {
                self.en(x - 1, y) + self.en(x + 1, y) + self.en(x, y - 1) + self.en(x, y + 1)
                    - 4.0 * self.en(x, y)
            })
            .collect();
        varianza(&valores)
    }

    /// The mean grey, 0 to 255.
    pub fn brillo(&self) -> f64 {
        media(self.pixeles.iter().map(|p| f64::from(*p)))
    }

    /// How much moved since `antes`: the mean difference, pixel by pixel.
    pub fn movimiento(&self, antes: &Gris) -> f64 {
        media(
            self.pixeles
                .iter()
                .zip(antes.pixeles)
                .map(|(a, b)| f64::from(a.abs_diff(*b))),
        )
    }
}

/// A clip's look over time, as `analisis.json` keeps it under `visual`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Visual {
    /// Samples a second.
    pub fps_muestreo: f64,
    /// Each sample's sharpness ([`Gris::nitidez`]).
    pub nitidez: Vec<f64>,
    /// How much moved since the sample before; 0 for the first.
    pub movimiento: Vec<f64>,
    /// Each sample's mean grey.
    pub brillo: Vec<f64>,
}

impl Visual {
    /// The look of a clip from its samples, in order, taken at `fps`.
    pub fn medir<'a>(muestras: impl IntoIterator<Item = Gris<'a>>, fps: f64) -> Self {
        let mut visual = Self {
            fps_muestreo: fps,
            ..Self::default()
        };
        let mut anterior: Option<Gris> = None;
        for muestra in muestras {
            visual.nitidez.push(redondeo(muestra.nitidez(), 1));
            visual.brillo.push(redondeo(muestra.brillo(), 1));
            let movido = anterior.map_or(0.0, |antes| muestra.movimiento(&antes));
            visual.movimiento.push(redondeo(movido, 2));
            anterior = Some(muestra);
        }
        visual
    }

    /// The `largo`-second stretch of a `duracion`-second clip that is
    /// sharpest and stillest, kept clear of the first and last samples -
    /// the shake of pressing record - and of dark or blown-out samples.
    pub fn mejor_tramo(&self, duracion: f64, largo: f64) -> TramoVisual {
        let paso = 1.0 / self.fps_muestreo;
        let muestras = self.nitidez.len();
        let ancho = ((largo / paso).round() as usize).max(1);
        // One sample off each end, when the clip has room for it.
        let margen = usize::from(muestras >= ancho + 2);
        if muestras <= ancho {
            return TramoVisual {
                desde: 0.0,
                dur: redondeo(largo.min(duracion), 2),
            };
        }
        let puntos = self.puntos();
        // Fewest unusable samples first, then the best on average; the
        // earliest among equals.
        let valor = |i: usize| {
            let tramo = &puntos[i..i + ancho];
            let inutiles = tramo.iter().filter(|p| p.is_none()).count();
            (inutiles, media(tramo.iter().flatten().copied()))
        };
        let inicio = (margen..=muestras - ancho - margen)
            .max_by(|&a, &b| {
                let ((inutiles_a, media_a), (inutiles_b, media_b)) = (valor(a), valor(b));
                inutiles_b
                    .cmp(&inutiles_a)
                    .then(media_a.total_cmp(&media_b))
                    .then(b.cmp(&a))
            })
            .unwrap_or(0);
        let desde = inicio as f64 * paso;
        TramoVisual {
            desde: redondeo(desde, 2),
            dur: redondeo(largo.min(duracion - desde), 2),
        }
    }

    /// Each sample's worth: sharpness up, movement down, both scaled to
    /// the clip's own range; None for a dark or blown-out sample.
    fn puntos(&self) -> Vec<Option<f64>> {
        let nitidez = escalar(&self.nitidez);
        let movimiento = escalar(&self.movimiento);
        nitidez
            .iter()
            .zip(&movimiento)
            .zip(&self.brillo)
            .map(|((n, m), b)| (OSCURO..=QUEMADO).contains(b).then_some(n - m))
            .collect()
    }
}

/// Where a clip's footage stretch starts and how long it is, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TramoVisual {
    /// Seconds into the clip.
    pub desde: f64,
    /// Seconds long.
    pub dur: f64,
}

/// A face in a picture, as fractions of its width and height.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 4]", into = "[f64; 4]")]
pub struct Cara {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub ancho: f64,
    /// Height.
    pub alto: f64,
}

impl From<[f64; 4]> for Cara {
    fn from([x, y, ancho, alto]: [f64; 4]) -> Self {
        Self { x, y, ancho, alto }
    }
}

impl From<Cara> for [f64; 4] {
    fn from(cara: Cara) -> Self {
        [cara.x, cara.y, cara.ancho, cara.alto]
    }
}

impl Cara {
    fn area(&self) -> f64 {
        self.ancho * self.alto
    }

    fn centro(&self) -> (f64, f64) {
        (self.x + self.ancho / 2.0, self.y + self.alto / 2.0)
    }

    /// Close enough to the camera to be a take's subject.
    pub fn cerca(&self) -> bool {
        self.ancho >= 0.15
    }
}

/// Where to centre a reframe so heads are not cut, as `analisis.json`
/// keeps it under `encuadre`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Encuadre {
    /// Across, 0 left to 1 right.
    pub cx: f64,
    /// Down, 0 top to 1 bottom.
    pub cy: f64,
}

/// The median centre of the largest face in each frame that has one.
pub fn encuadre<'a>(caras_por_fotograma: impl IntoIterator<Item = &'a [Cara]>) -> Option<Encuadre> {
    let centros: Vec<(f64, f64)> = caras_por_fotograma
        .into_iter()
        .filter_map(|caras| caras.iter().max_by(|a, b| a.area().total_cmp(&b.area())))
        .map(Cara::centro)
        .collect();
    if centros.is_empty() {
        return None;
    }
    let cx = mediana(centros.iter().map(|c| c.0).collect());
    let cy = mediana(centros.iter().map(|c| c.1).collect());
    Some(Encuadre {
        cx: redondeo(cx, 3),
        cy: redondeo(cy, 3),
    })
}

/// `valores` scaled to 0..1 over their own range; all equal is all 0.
fn escalar(valores: &[f64]) -> Vec<f64> {
    let menor = valores.iter().copied().fold(f64::INFINITY, f64::min);
    let mayor = valores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let rango = if mayor > menor { mayor - menor } else { 1.0 };
    valores.iter().map(|v| (v - menor) / rango).collect()
}

fn media(valores: impl Iterator<Item = f64>) -> f64 {
    let (suma, cuantos) = valores.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if cuantos == 0 {
        0.0
    } else {
        suma / cuantos as f64
    }
}

fn varianza(valores: &[f64]) -> f64 {
    let m = media(valores.iter().copied());
    media(valores.iter().map(|v| (v - m).powi(2)))
}

fn mediana(mut valores: Vec<f64>) -> f64 {
    valores.sort_by(f64::total_cmp);
    let mitad = valores.len() / 2;
    if valores.len().is_multiple_of(2) {
        (valores[mitad - 1] + valores[mitad]) / 2.0
    } else {
        valores[mitad]
    }
}

fn redondeo(valor: f64, decimales: i32) -> f64 {
    let escala = 10f64.powi(decimales);
    (valor * escala).round() / escala
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plano(valor: u8) -> Vec<u8> {
        vec![valor; 16]
    }

    fn gris(pixeles: &[u8]) -> Gris<'_> {
        Gris {
            ancho: 4,
            alto: 4,
            pixeles,
        }
    }

    #[test]
    fn a_flat_picture_is_not_sharp_and_an_edge_is() {
        assert_eq!(gris(&plano(128)).nitidez(), 0.0);
        let borde: Vec<u8> = (0..16).map(|i| if i % 4 < 2 { 0 } else { 255 }).collect();
        assert!(gris(&borde).nitidez() > 1000.0);
    }

    #[test]
    fn movement_is_the_mean_difference() {
        let (a, b) = (plano(10), plano(30));
        assert_eq!(gris(&b).movimiento(&gris(&a)), 20.0);
        let visual = Visual::medir([gris(&a), gris(&b), gris(&b)], 2.0);
        assert_eq!(visual.movimiento, [0.0, 20.0, 0.0]);
        assert_eq!(visual.brillo, [10.0, 30.0, 30.0]);
    }

    fn clip(nitidez: &[f64], movimiento: &[f64]) -> Visual {
        Visual {
            fps_muestreo: 2.0,
            nitidez: nitidez.to_vec(),
            movimiento: movimiento.to_vec(),
            brillo: vec![120.0; nitidez.len()],
        }
    }

    #[test]
    fn the_best_stretch_is_sharp_and_still_and_clear_of_the_ends() {
        let visual = clip(
            &[90.0, 10.0, 10.0, 80.0, 80.0, 80.0, 80.0, 10.0, 90.0],
            &[0.0, 9.0, 9.0, 1.0, 1.0, 1.0, 1.0, 9.0, 0.0],
        );
        assert_eq!(
            visual.mejor_tramo(4.5, 2.0),
            TramoVisual {
                desde: 1.5,
                dur: 2.0
            }
        );
    }

    #[test]
    fn a_short_clip_is_shown_whole() {
        let visual = clip(&[10.0, 20.0], &[0.0, 1.0]);
        assert_eq!(
            visual.mejor_tramo(1.2, 2.0),
            TramoVisual {
                desde: 0.0,
                dur: 1.2
            }
        );
    }

    #[test]
    fn dark_samples_are_never_the_best() {
        let mut visual = clip(&[50.0; 10], &[1.0; 10]);
        visual.nitidez[2] = 99.0;
        visual.nitidez[3] = 99.0;
        visual.nitidez[4] = 99.0;
        visual.nitidez[5] = 99.0;
        visual.brillo[3] = 5.0;
        let tramo = visual.mejor_tramo(5.0, 2.0);
        assert!(tramo.desde >= 2.0, "{tramo:?} starts in the dark");
    }

    #[test]
    fn the_frame_follows_the_largest_face() {
        let lejos = Cara {
            x: 0.0,
            y: 0.0,
            ancho: 0.1,
            alto: 0.1,
        };
        let cerca = Cara {
            x: 0.4,
            y: 0.2,
            ancho: 0.2,
            alto: 0.2,
        };
        let fotogramas: [&[Cara]; 3] = [&[lejos, cerca], &[], &[cerca]];
        assert_eq!(encuadre(fotogramas), Some(Encuadre { cx: 0.5, cy: 0.3 }));
        assert_eq!(encuadre([&[][..]]), None);
        assert!(cerca.cerca() && !lejos.cerca());
    }

    #[test]
    fn faces_are_kept_as_the_engine_keeps_them() {
        let cara = Cara {
            x: 0.1,
            y: 0.2,
            ancho: 0.3,
            alto: 0.4,
        };
        assert_eq!(serde_json::to_string(&cara).unwrap(), "[0.1,0.2,0.3,0.4]");
        assert_eq!(
            serde_json::from_str::<Cara>("[0.1,0.2,0.3,0.4]").unwrap(),
            cara
        );
    }
}
