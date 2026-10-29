//! Looks at clips the way the montage analysis does and prints what it saw,
//! as `analisis.json` would keep it - to set beside the Python engine's.
//!
//! ```text
//! cargo run -p analisis --features vision --example mirar -- [--sin-caras] <job folder> <clip>...
//! ```
//!
//! With only `imagen` (no ONNX Runtime) it looks without faces:
//!
//! ```text
//! cargo run -p analisis --features imagen --example mirar -- --sin-caras <job folder> <clip>...
//! ```

use std::path::{Path, PathBuf};

use analisis::caras::BuscaCaras;
use analisis::mirada;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut argumentos = std::env::args().skip(1).peekable();
    let sin_caras = argumentos.next_if_eq("--sin-caras").is_some();
    let trabajo = PathBuf::from(
        argumentos
            .next()
            .ok_or("mirar [--sin-caras] <job folder> <clip>...")?,
    );
    let mut detector = if sin_caras { None } else { Some(detector()?) };
    for ruta in argumentos {
        let ruta = Path::new(&ruta);
        let id = ruta
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let inicio = std::time::Instant::now();
        let duracion = concat_media::probe(ruta)?
            .duration
            .map_or(0.0, |d| d.as_f64());
        let mirada = mirada::mirar(
            detector.as_mut().map(|d| d.as_mut() as &mut dyn BuscaCaras),
            ruta,
            &id,
            duracion,
            3,
            &trabajo,
        )?;
        let resultado = serde_json::json!({
            "id": id,
            "mejor_tramo": mirada.mejor_tramo,
            "caras": mirada.fotogramas.iter().map(|f| f.caras.len()).collect::<Vec<_>>(),
            "cara_cerca": mirada.cara_cerca,
            "encuadre": mirada.encuadre,
            "muestras": mirada.visual.nitidez.len(),
        });
        eprintln!(
            "{}: {:.1} s",
            ruta.display(),
            inicio.elapsed().as_secs_f64()
        );
        println!("{resultado}");
    }
    Ok(())
}

#[cfg(feature = "vision")]
fn detector() -> Result<Box<dyn BuscaCaras>, String> {
    Ok(Box::new(analisis::caras::Detector::new()?))
}

#[cfg(not(feature = "vision"))]
fn detector() -> Result<Box<dyn BuscaCaras>, String> {
    Err("built without `vision`: pass --sin-caras".into())
}
