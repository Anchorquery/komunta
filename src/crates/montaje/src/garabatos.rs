//! Hand-drawn marks (circle, heart, underline): the engine's ASS drawings
//! rasterised once to a transparent PNG. The project shows the PNG and
//! reveals it left to right with an animated mask, as the engine's
//! `\clip` sweep did.

use std::path::{Path, PathBuf};

use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::Error;

/// The engine's strokes, in units around the centre; the end is left a
/// little open on purpose.
pub fn trazo(forma: &str) -> Option<&'static str> {
    Some(match forma {
        "circulo" => {
            "m 10 -205 b 125 -200 210 -105 200 5 b 190 120 100 205 -10 198 \
             b -125 190 -205 100 -198 -10 b -190 -120 -95 -200 30 -190"
        }
        "corazon" => {
            "m 0 -60 b 40 -185 225 -140 160 25 b 120 125 30 160 0 225 \
             b -30 160 -125 120 -165 20 b -225 -145 -35 -180 6 -48"
        }
        "subrayado" => "m -230 0 b -120 -18 60 12 150 -6 b 190 -12 215 -4 235 4",
        _ => return None,
    })
}

/// A drawn mark on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Dibujo {
    /// The PNG.
    pub ruta: PathBuf,
    /// Its size in pixels.
    pub ancho: u32,
    /// Its size in pixels.
    pub alto: u32,
}

/// Draws mark `forma` at `escala` pixels per unit with a `grosor`-pixel
/// white line into `carpeta`, reusing the file when it is already there.
pub fn dibujar(
    forma: &str,
    escala: f32,
    grosor: f32,
    carpeta: &Path,
) -> Result<Option<Dibujo>, Error> {
    let Some(trazo) = trazo(forma) else {
        return Ok(None);
    };
    let puntos = numeros(trazo);
    let (min_x, max_x) = extremos(puntos.iter().map(|(_, x, _)| *x));
    let (min_y, max_y) = extremos(puntos.iter().map(|(_, _, y)| *y));
    // Centred on the drawing's own centre, as the mark is placed by it.
    let medio = grosor;
    let semi_w = (max_x.abs().max(min_x.abs())) * escala + medio;
    let semi_h = (max_y.abs().max(min_y.abs())) * escala + medio;
    let (ancho, alto) = ((semi_w * 2.0).ceil() as u32, (semi_h * 2.0).ceil() as u32);
    let nombre = format!("{forma}_{}.png", (escala * 1000.0).round() as u32);
    let ruta = carpeta.join(nombre);
    if !ruta.is_file() {
        let mut camino = PathBuilder::new();
        let pt = |x: f32, y: f32| (semi_w + x * escala, semi_h + y * escala);
        let mut i = 0;
        while i < puntos.len() {
            let (orden, x, y) = puntos[i];
            match orden {
                'm' => {
                    let (x, y) = pt(x, y);
                    camino.move_to(x, y);
                    i += 1;
                }
                'b' if i + 2 < puntos.len() => {
                    let (x1, y1) = pt(x, y);
                    let (x2, y2) = pt(puntos[i + 1].1, puntos[i + 1].2);
                    let (x3, y3) = pt(puntos[i + 2].1, puntos[i + 2].2);
                    camino.cubic_to(x1, y1, x2, y2, x3, y3);
                    i += 3;
                }
                _ => {
                    let (x, y) = pt(x, y);
                    camino.line_to(x, y);
                    i += 1;
                }
            }
        }
        let camino = camino.finish().ok_or(Error::Garabato(forma.to_owned()))?;
        let mut lienzo = Pixmap::new(ancho, alto).ok_or(Error::Garabato(forma.to_owned()))?;
        let mut pintura = Paint::default();
        pintura.set_color_rgba8(255, 255, 255, 240);
        pintura.anti_alias = true;
        let linea = Stroke {
            width: grosor,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        lienzo.stroke_path(&camino, &pintura, &linea, Transform::identity(), None);
        std::fs::create_dir_all(carpeta).map_err(|e| Error::Escribir(carpeta.into(), e))?;
        lienzo
            .save_png(&ruta)
            .map_err(|e| Error::Escribir(ruta.clone(), std::io::Error::other(e.to_string())))?;
    }
    Ok(Some(Dibujo { ruta, ancho, alto }))
}

/// The drawing's points, each with the command it belongs to.
fn numeros(trazo: &str) -> Vec<(char, f32, f32)> {
    let mut puntos = Vec::new();
    let mut orden = 'm';
    let mut pendiente: Option<f32> = None;
    for pieza in trazo.split_whitespace() {
        match pieza.parse::<f32>() {
            Ok(n) => match pendiente.take() {
                Some(x) => puntos.push((orden, x, n)),
                None => pendiente = Some(n),
            },
            Err(_) => orden = pieza.chars().next().unwrap_or('l'),
        }
    }
    puntos
}

fn extremos(valores: impl Iterator<Item = f32>) -> (f32, f32) {
    valores.fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mark_parses_into_bezier_runs() {
        for forma in ["circulo", "corazon", "subrayado"] {
            let puntos = numeros(trazo(forma).unwrap());
            assert_eq!(puntos[0].0, 'm');
            assert_eq!((puntos.len() - 1) % 3, 0, "{forma}");
        }
    }

    #[test]
    fn a_mark_is_drawn_once() {
        let carpeta =
            std::env::temp_dir().join(format!("montaje-garabatos-{}", std::process::id()));
        let dibujo = dibujar("circulo", 1.5, 21.0, &carpeta).unwrap().unwrap();
        assert!(dibujo.ruta.is_file());
        assert!(dibujo.ancho > 600 && dibujo.alto > 600);
        let otra = dibujar("circulo", 1.5, 21.0, &carpeta).unwrap().unwrap();
        assert_eq!(otra, dibujo);
        assert!(dibujar("estrella", 1.5, 21.0, &carpeta).unwrap().is_none());
        let _ = std::fs::remove_dir_all(carpeta);
    }
}
