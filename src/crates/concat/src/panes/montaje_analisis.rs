//! A montage's clips analysed here, before the engine runs: whisper and
//! the voice detector from the app's models folder, YuNet for the faces,
//! into the job's `analisis.json`. What is not done here the engine does
//! after, as it always did: it only analyses the clips the file lacks.
//!
//! Only with a graphics card under whisper. On the processor whisper.cpp
//! encodes each clip three or four times (the language, the words, the
//! Basque contrast) and took five times the engine's CTranslate2 on CUDA
//! for the same five clips - slower than the engine on the processor too.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use analisis::Idioma;
use analisis::caras::{BuscaCaras, Detector};
use analisis::clip::Ajustes;
use analisis::escucha::{self, Modelos, Oido};
use analisis::trabajo::{self, Analista, ErrorTrabajo, FOTOGRAMAS_POR_CLIP};
use concat_host::AppDirs;
use concat_speech::transcribe;
use montaje::ia::Ia;
use montaje::motor::{Avance, Parada};

/// The run was stopped.
#[derive(Debug)]
pub(crate) struct Cancelado;

/// Analyses the clips of the job in `trabajo` that its `analisis.json`
/// lacks, in `idioma` ("es", "eu"; None lets whisper tell), telling
/// `avisar` as each starts. Only a stop is an error: anything else is
/// logged and left to the engine.
pub(crate) fn analizar(
    dirs: &AppDirs,
    ia: &Ia,
    idioma: Option<&str>,
    trabajo: &Path,
    parada: &Parada,
    mut avisar: impl FnMut(Avance),
) -> Result<(), Cancelado> {
    match grafica() {
        Ok(grafica) => log::info!("montaje: the clips are analysed here, whisper on {grafica}"),
        Err(motivo) => {
            log::info!("montaje: the engine analyses the clips: {motivo}");
            return Ok(());
        }
    }
    let modelos = match modelos(dirs, ia, idioma) {
        Ok(modelos) => modelos,
        Err(motivo) => {
            log::info!("montaje: the engine analyses the clips: {motivo}");
            return Ok(());
        }
    };
    let mut oido = match Oido::new(modelos) {
        Ok(oido) => oido,
        Err(error) => {
            log::warn!("montaje: whisper would not start, the engine analyses the clips: {error}");
            return Ok(());
        }
    };
    oido.parar_con(parada.bandera());
    let mut detector = Detector::new()
        .inspect_err(|error| log::warn!("montaje: the clips are analysed without faces: {error}"))
        .ok();
    let mut analista = Analista {
        oido: &mut oido,
        caras: detector.as_mut().map(|d| d as &mut dyn BuscaCaras),
        ajustes: Ajustes {
            idioma: idioma.map(Idioma::from),
            ..Ajustes::default()
        },
        fotogramas: FOTOGRAMAS_POR_CLIP,
    };
    let resultado = trabajo::analizar(trabajo, &mut analista, |avance| {
        avisar(Avance {
            fase: "analisis".into(),
            fraccion: avance.n as f64 / avance.de as f64,
            mensaje: format!("{} ({}/{})", avance.id, avance.n + 1, avance.de),
        });
        !parada.cancelada()
    });
    match resultado {
        Ok(resumen) => {
            log::info!(
                "montaje: {} clips analysed here, {} left to the engine",
                resumen.analizados,
                resumen.fallidos.len()
            );
            Ok(())
        }
        Err(ErrorTrabajo::Cancelado) => Err(Cancelado),
        Err(error) => {
            log::warn!("montaje: the analysis stopped, the engine finishes it: {error}");
            Ok(())
        }
    }
}

/// The graphics card whisper would run on, or why it would not.
fn grafica() -> Result<String, String> {
    transcribe::cpu_backend()?;
    transcribe::gpu_backend().ok_or_else(|| {
        "whisper has no graphics card here, and on the processor the engine is faster".into()
    })
}

/// The model files for `idioma`, or why the analysis cannot run here.
/// Spanish is always needed: it is the one that guesses the language.
/// Basque is not, for a shoot known to be in Spanish.
fn modelos(dirs: &AppDirs, ia: &Ia, idioma: Option<&str>) -> Result<Modelos, String> {
    let (es, eu) = ia.whisper();
    let castellano = descargado(dirs, es)?;
    let euskera = match idioma {
        Some("es") => descargado(dirs, eu).unwrap_or_else(|_| castellano.clone()),
        _ => descargado(dirs, eu)?,
    };
    let voz = transcribe::voice_detector(dirs, &AtomicBool::new(false))?;
    Ok(Modelos {
        castellano,
        euskera,
        voz,
        prompt_castellano: Some(escucha::PROMPT_CASTELLANO.into()),
        prompt_euskera: Some(escucha::PROMPT_EUSKERA.into()),
    })
}

/// Whisper model `id`'s file, if it has been downloaded.
fn descargado(dirs: &AppDirs, id: &str) -> Result<PathBuf, String> {
    let ruta = transcribe::model_file(dirs, id)?;
    if ruta.is_file() {
        Ok(ruta)
    } else {
        Err(format!("whisper {id} is not downloaded"))
    }
}
