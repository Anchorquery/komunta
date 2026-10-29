//! Hears clips the way the montage analysis does and prints what it heard,
//! as `analisis.json` would keep it - to set beside the Python engine's.
//!
//! ```text
//! cargo run -p analisis --features escucha --example escuchar -- <models folder> <es model id> <eu model id> <clip>...
//! ```

use std::path::{Path, PathBuf};

use analisis::clip::{self, Ajustes};
use analisis::escucha::{self, Modelos, Oido};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut argumentos = std::env::args().skip(1);
    let uso = "escuchar <models folder> <es model id> <eu model id> <clip>...";
    let carpeta = PathBuf::from(argumentos.next().ok_or(uso)?);
    let modelo = |id: String| carpeta.join(format!("ggml-{id}.bin"));
    let castellano = modelo(argumentos.next().ok_or(uso)?);
    let euskera = modelo(argumentos.next().ok_or(uso)?);
    let mut oido = Oido::new(Modelos {
        castellano,
        euskera,
        voz: carpeta.join("ggml-silero-v5.1.2.bin"),
        prompt_castellano: Some(escucha::PROMPT_CASTELLANO.into()),
        prompt_euskera: Some(escucha::PROMPT_EUSKERA.into()),
    })?;
    for ruta in argumentos {
        let inicio = std::time::Instant::now();
        let escucha = clip::escuchar(
            &mut oido,
            Path::new(&ruta),
            duracion(&ruta),
            true,
            &Ajustes::default(),
        )?;
        eprintln!("{ruta}: {:.1} s", inicio.elapsed().as_secs_f64());
        println!("{}", serde_json::to_string(&escucha)?);
    }
    Ok(())
}

/// The clip's length, which the inventory gives the engine.
fn duracion(ruta: &str) -> f64 {
    concat_media::probe(ruta)
        .ok()
        .and_then(|info| info.duration)
        .map_or(0.0, |duracion| duracion.as_f64())
}
