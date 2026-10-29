//! Analyses a job's clips into its `analisis.json` the way the app does
//! before the engine runs - without faces, which need ONNX Runtime.
//!
//! ```text
//! cargo run -p analisis --features escucha,imagen --example trabajo -- <models folder> <es model id> <eu model id> <job folder> [es|eu]
//! ```

use std::path::{Path, PathBuf};

use analisis::Idioma;
use analisis::clip::Ajustes;
use analisis::escucha::{self, Modelos, Oido};
use analisis::trabajo::{self, Analista, FOTOGRAMAS_POR_CLIP};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut argumentos = std::env::args().skip(1);
    let uso = "trabajo <models folder> <es model id> <eu model id> <job folder> [es|eu]";
    let carpeta = PathBuf::from(argumentos.next().ok_or(uso)?);
    let modelo = |id: String| carpeta.join(format!("ggml-{id}.bin"));
    let castellano = modelo(argumentos.next().ok_or(uso)?);
    let euskera = modelo(argumentos.next().ok_or(uso)?);
    let trabajo = PathBuf::from(argumentos.next().ok_or(uso)?);
    let idioma = argumentos.next().map(|codigo| match codigo.as_str() {
        "eu" => Idioma::euskera(),
        _ => Idioma::castellano(),
    });
    let mut oido = Oido::new(Modelos {
        castellano,
        euskera,
        voz: carpeta.join("ggml-silero-v5.1.2.bin"),
        prompt_castellano: Some(escucha::PROMPT_CASTELLANO.into()),
        prompt_euskera: Some(escucha::PROMPT_EUSKERA.into()),
    })?;
    let mut analista = Analista {
        oido: &mut oido,
        caras: None,
        ajustes: Ajustes {
            idioma,
            ..Ajustes::default()
        },
        fotogramas: FOTOGRAMAS_POR_CLIP,
    };
    let inicio = std::time::Instant::now();
    let resumen = trabajo::analizar(Path::new(&trabajo), &mut analista, |avance| {
        eprintln!("[{}/{}] {}", avance.n + 1, avance.de, avance.id);
        true
    })?;
    eprintln!("{resumen:?} en {:.1} s", inicio.elapsed().as_secs_f64());
    Ok(())
}
