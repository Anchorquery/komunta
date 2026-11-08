//! Lists a material folder into a job's `inventario.json` the way the app
//! does before the engine runs, and with `--hojas` draws the job's contact
//! sheets from its `analisis.json` - to compare with the Python engine's.
//!
//! ```text
//! cargo run -p analisis --features imagen --example inventariar -- <material> <job folder> [--guion <file>] [--hojas]
//! ```

use std::path::PathBuf;

use analisis::{hojas, ingesta};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let uso = "inventariar <material> <job folder> [--guion <file>] [--hojas]";
    let mut argumentos = std::env::args().skip(1);
    let material = PathBuf::from(argumentos.next().ok_or(uso)?);
    let trabajo = PathBuf::from(argumentos.next().ok_or(uso)?);
    let (mut guion, mut con_hojas) = (None, false);
    while let Some(argumento) = argumentos.next() {
        match argumento.as_str() {
            "--guion" => guion = Some(PathBuf::from(argumentos.next().ok_or(uso)?)),
            "--hojas" => con_hojas = true,
            _ => return Err(uso.into()),
        }
    }

    let inicio = std::time::Instant::now();
    let mut inventario = ingesta::inventariar(&material, &trabajo, &ingesta::sondear)?;
    if let Some(guion) = guion {
        ingesta::anadir_guion(&mut inventario, &guion, &trabajo)?;
    }
    println!(
        "{} vídeos, {} imágenes, {} audios, {} documentos, {} descartados en {:.1} s",
        inventario.videos.len(),
        inventario.imagenes.len(),
        inventario.audios.len(),
        inventario.documentos.len(),
        inventario.descartados.len(),
        inicio.elapsed().as_secs_f64()
    );
    if con_hojas {
        let celdas = hojas::celdas(&trabajo, &inventario)?;
        for hoja in hojas::generar(&trabajo, &celdas)? {
            println!("{}", hoja.display());
        }
    }
    Ok(())
}
