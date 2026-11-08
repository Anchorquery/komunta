//! Analyses a job's clips into its `analisis.json` the way the app does
//! before the engine runs - without faces, which need ONNX Runtime - and
//! its stills and sound files. With `--cache` the clips are kept in, and
//! taken from, that folder; with `--musica` a brand kit's music is
//! analysed too.
//!
//! ```text
//! cargo run -p analisis --features escucha,imagen --example trabajo -- <models folder> <es model id> <eu model id> <job folder> [--idioma es|eu] [--cache <folder>] [--musica <file>]
//! ```

use std::path::PathBuf;

use analisis::Idioma;
use analisis::cache::Cache;
use analisis::clip::Ajustes;
use analisis::escucha::{self, Modelos, Oido};
use analisis::trabajo::{self, Analista, FOTOGRAMAS_POR_CLIP};

const USO: &str = "trabajo <models folder> <es model id> <eu model id> <job folder> \
                   [--idioma es|eu] [--cache <folder>] [--musica <file>]";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut argumentos = std::env::args().skip(1);
    let carpeta = PathBuf::from(argumentos.next().ok_or(USO)?);
    let modelo = |id: String| carpeta.join(format!("ggml-{id}.bin"));
    let castellano = modelo(argumentos.next().ok_or(USO)?);
    let euskera = modelo(argumentos.next().ok_or(USO)?);
    let trabajo = PathBuf::from(argumentos.next().ok_or(USO)?);
    let (mut idioma, mut cache, mut musica_kit) = (None, None, None);
    while let Some(bandera) = argumentos.next() {
        let valor = argumentos.next().ok_or(USO)?;
        match bandera.as_str() {
            "--idioma" if valor == "eu" => idioma = Some(Idioma::euskera()),
            "--idioma" => idioma = Some(Idioma::castellano()),
            "--cache" => cache = Some(Cache::new(valor.into(), serde_json::json!({"ejemplo": 1}))),
            "--musica" => musica_kit = Some(PathBuf::from(valor)),
            _ => return Err(USO.into()),
        }
    }
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
        cache: cache.as_ref(),
        musica_kit,
    };
    let inicio = std::time::Instant::now();
    let resumen = trabajo::analizar(&trabajo, &mut analista, |avance| {
        eprintln!("[{}/{}] {}", avance.n + 1, avance.de, avance.id);
        true
    })?;
    eprintln!("{resumen:?} en {:.1} s", inicio.elapsed().as_secs_f64());
    Ok(())
}
