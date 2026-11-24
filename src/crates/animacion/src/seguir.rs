// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! A title that follows a face: the face is looked for every fifth of a
//! second of the stretch, and the title is keyed to stand just above it,
//! or just below, as it moves. Where no face is seen the title holds where
//! it was; the path is smoothed so a detector's jitter does not shake it.

use concat_project::Command;
use concat_project::model::{Clip, KeyEase, KeyProperty};

use crate::llaves;
use crate::tiempo::{en_clip, tramo};

/// A face's box in the frame, `0..=1` from the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caja {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub ancho: f64,
    /// Height.
    pub alto: f64,
}

impl Caja {
    fn centro_x(&self) -> f64 {
        self.x + self.ancho / 2.0
    }

    fn area(&self) -> f64 {
        self.ancho * self.alto
    }
}

/// Which side of the face the title stands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lado {
    /// Over the head.
    Encima,
    /// Under the chin.
    Debajo,
}

impl Lado {
    /// The side named `nombre`: `encima` or `debajo`.
    pub fn por_nombre(nombre: &str) -> Option<Self> {
        match nombre {
            "encima" => Some(Lado::Encima),
            "debajo" => Some(Lado::Debajo),
            _ => None,
        }
    }
}

/// The face seen at timeline second `t`, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Muestra {
    /// Timeline second.
    pub t: f64,
    /// The face, the largest one when there are several.
    pub cara: Option<Caja>,
}

/// How often the face is looked for.
const PASO: f64 = 0.2;
/// Most looks in one stretch: a long one is looked at less often.
const MUESTRAS: usize = 40;
/// Air between the face and the title, as a fraction of frame height.
const AIRE: f64 = 0.06;
/// How close to the frame's edges the title's middle may go.
const BORDE: f64 = 0.08;

/// The seconds of `desde..=hasta` to look for the face at.
pub fn instantes(desde: f64, hasta: f64) -> Vec<f64> {
    let largo = (hasta - desde).max(0.0);
    let pasos = ((largo / PASO).ceil() as usize).clamp(1, MUESTRAS - 1);
    (0..=pasos).map(|i| desde + largo * i as f64 / pasos as f64).collect()
}

/// The largest of `caras`, the one a title follows.
pub fn mayor(caras: &[Caja]) -> Option<Caja> {
    caras.iter().copied().max_by(|a, b| a.area().total_cmp(&b.area()))
}

/// The commands that key `titulo`'s offsets to follow the faces of
/// `muestras` from `desde` to `hasta`, or why not. Offset keys the title
/// already has in the stretch are replaced.
pub fn claves(
    titulo: &Clip,
    desde: f64,
    hasta: f64,
    muestras: &[Muestra],
    lado: Lado,
) -> Result<Vec<Command>, String> {
    let (desde, hasta) = tramo(titulo, desde, hasta)?;
    let puntos = suavizar(&rellenar(muestras)?);
    let propiedades = [KeyProperty::OffsetX, KeyProperty::OffsetY];
    let mut comandos = llaves::quitar(titulo, desde, hasta, &propiedades);
    for (t, caja) in puntos.iter().filter(|(t, _)| (desde..=hasta).contains(t)) {
        let (x, y) = sitio(caja, lado);
        for (propiedad, valor) in [(KeyProperty::OffsetX, x - 0.5), (KeyProperty::OffsetY, y - 0.5)] {
            comandos.push(Command::SetClipKey {
                clip_id: titulo.id.clone(),
                property: propiedad,
                at: en_clip(titulo, *t),
                value: valor,
                ease: KeyEase::LINEAR,
            });
        }
    }
    Ok(comandos)
}

/// Every look with a face: one where none was seen holds the last face
/// seen, and the first ones take the first face seen.
fn rellenar(muestras: &[Muestra]) -> Result<Vec<(f64, Caja)>, String> {
    let primera = muestras
        .iter()
        .find_map(|m| m.cara)
        .ok_or("no se ve ninguna cara en ese tramo: mira el fotograma o pon el título a mano")?;
    let mut ultima = primera;
    Ok(muestras
        .iter()
        .map(|m| {
            ultima = m.cara.unwrap_or(ultima);
            (m.t, ultima)
        })
        .collect())
}

/// Each box averaged with its neighbours: the detector's jitter out.
fn suavizar(puntos: &[(f64, Caja)]) -> Vec<(f64, Caja)> {
    (0..puntos.len())
        .map(|i| {
            let vecinos = &puntos[i.saturating_sub(1)..(i + 2).min(puntos.len())];
            let n = vecinos.len() as f64;
            let media = |f: fn(&Caja) -> f64| vecinos.iter().map(|(_, c)| f(c)).sum::<f64>() / n;
            let caja = Caja {
                x: media(|c| c.x),
                y: media(|c| c.y),
                ancho: media(|c| c.ancho),
                alto: media(|c| c.alto),
            };
            (puntos[i].0, caja)
        })
        .collect()
}

/// Where the title's middle goes for a face at `caja`.
fn sitio(caja: &Caja, lado: Lado) -> (f64, f64) {
    let y = match lado {
        Lado::Encima => caja.y - AIRE,
        Lado::Debajo => caja.y + caja.alto + AIRE,
    };
    let dentro = BORDE..=1.0 - BORDE;
    (caja.centro_x().clamp(*dentro.start(), *dentro.end()), y.clamp(*dentro.start(), *dentro.end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiempo::prueba::clip;

    fn cara(x: f64) -> Option<Caja> {
        Some(Caja {
            x,
            y: 0.3,
            ancho: 0.2,
            alto: 0.25,
        })
    }

    fn llaves_de(comandos: &[Command], propiedad: KeyProperty) -> Vec<f64> {
        comandos
            .iter()
            .filter_map(|c| match c {
                Command::SetClipKey { property, value, .. } if *property == propiedad => Some(*value),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_stretch_is_looked_at_every_fifth_of_a_second_and_a_long_one_less() {
        assert_eq!(instantes(1.0, 2.0).len(), 6);
        assert_eq!(instantes(0.0, 60.0).len(), MUESTRAS);
        assert_eq!(instantes(3.0, 3.0), [3.0, 3.0]);
    }

    #[test]
    fn the_title_stands_over_the_face_and_moves_with_it() {
        let titulo = clip(0.0, 4.0);
        let muestras = [
            Muestra { t: 0.0, cara: cara(0.2) },
            Muestra { t: 2.0, cara: cara(0.2) },
            Muestra { t: 4.0, cara: cara(0.2) },
        ];
        let comandos = claves(&titulo, 0.0, 4.0, &muestras, Lado::Encima).unwrap();
        let xs = llaves_de(&comandos, KeyProperty::OffsetX);
        let ys = llaves_de(&comandos, KeyProperty::OffsetY);
        assert_eq!(xs.len(), 3);
        assert!(xs.iter().all(|x| (x - (0.3 - 0.5)).abs() < 1e-9), "{xs:?}");
        assert!(ys.iter().all(|y| (y - (0.3 - AIRE - 0.5)).abs() < 1e-9), "{ys:?}");
    }

    #[test]
    fn a_look_without_a_face_holds_and_jitter_is_smoothed() {
        let titulo = clip(0.0, 3.0);
        let muestras = [
            Muestra { t: 0.0, cara: None },
            Muestra { t: 1.0, cara: cara(0.4) },
            Muestra { t: 2.0, cara: None },
            Muestra { t: 3.0, cara: cara(0.4) },
        ];
        let comandos = claves(&titulo, 0.0, 3.0, &muestras, Lado::Debajo).unwrap();
        let xs = llaves_de(&comandos, KeyProperty::OffsetX);
        assert_eq!(xs.len(), 4);
        assert!(xs.iter().all(|x| (x - 0.0).abs() < 1e-9), "{xs:?}");
    }

    #[test]
    fn no_face_at_all_is_said_and_the_title_kept_in_the_frame() {
        let titulo = clip(0.0, 1.0);
        let nada = [Muestra { t: 0.0, cara: None }];
        assert!(claves(&titulo, 0.0, 1.0, &nada, Lado::Encima).unwrap_err().contains("ninguna cara"));
        let arriba = Caja { x: 0.0, y: 0.0, ancho: 0.1, alto: 0.1 };
        assert_eq!(sitio(&arriba, Lado::Encima), (BORDE, BORDE));
    }

    #[test]
    fn the_largest_face_is_followed() {
        let chica = Caja { x: 0.0, y: 0.0, ancho: 0.1, alto: 0.1 };
        let grande = Caja { x: 0.5, y: 0.5, ancho: 0.3, alto: 0.3 };
        assert_eq!(mayor(&[chica, grande]), Some(grande));
        assert_eq!(mayor(&[]), None);
    }
}
