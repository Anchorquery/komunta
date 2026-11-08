//! A montage's material listed and its clips analysed: the folder into
//! the job's `inventario.json`, then whisper and the voice detector from
//! the app's models folder, YuNet for the faces, into its `analisis.json`.
//! Only the clips the file lacks are analysed, and a clip another job
//! analysed comes from the cache.
//!
//! Whisper hears on the graphics card when ggml has one, on the processor
//! otherwise: there, with each clip encoded once for its language and its
//! words (see `patches/README.md`), five clips take about as long as the
//! Python engine's faster-whisper did.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use analisis::cache::Cache;
use analisis::caras::{BuscaCaras, Detector};
use analisis::clip::Ajustes;
use analisis::escucha::{self, Modelos, Oido};
use analisis::trabajo::{self, Analista, ErrorTrabajo, FOTOGRAMAS_POR_CLIP};
use analisis::{Idioma, Inventario, ingesta};
use concat_host::AppDirs;
use concat_speech::transcribe;
use montaje::ia::Ia;
use montaje::avance::{Avance, Parada};

use crate::i18n::tf;

/// Lists the folder `material`, and `guion` when the script is elsewhere,
/// into the job folder `trabajo`, telling `avisar` how it starts and ends.
pub(crate) fn inventariar(
    material: &Path,
    guion: Option<&Path>,
    trabajo: &Path,
    mut avisar: impl FnMut(Avance),
) -> io::Result<Inventario> {
    let paso = |fraccion: f64, mensaje: String| Avance {
        fase: "ingesta".into(),
        fraccion,
        mensaje,
    };
    avisar(paso(0.0, "Leyendo la carpeta".into()));
    let mut inventario = ingesta::inventariar(material, trabajo, &ingesta::sondear)?;
    if let Some(guion) = guion {
        ingesta::anadir_guion(&mut inventario, guion, trabajo)?;
    }
    avisar(paso(1.0, resumen(&inventario)));
    Ok(inventario)
}

/// What was found, as the Python engine said it.
fn resumen(inventario: &Inventario) -> String {
    let mut dicho = format!(
        "{} vídeos, {} imágenes, {} documentos",
        inventario.videos.len(),
        inventario.imagenes.len(),
        inventario.documentos.len()
    );
    if !inventario.descartados.is_empty() {
        dicho.push_str(&format!(" ({} descartados)", inventario.descartados.len()));
    }
    dicho
}

/// A job to analyse here.
pub(crate) struct Trabajo {
    /// Its folder, with the inventory in it.
    pub carpeta: PathBuf,
    /// "es", "eu"; None lets whisper tell.
    pub idioma: Option<String>,
    /// The clips other jobs analysed.
    pub cache: PathBuf,
    /// The chosen brand kit's music.
    pub musica_kit: Option<PathBuf>,
}

/// Analyses the clips of `trabajo` that its `analisis.json` lacks, and its
/// stills and sound files, telling `avisar` as each clip starts. A clip
/// that fails is logged and left out; whisper that cannot start, or a
/// stop, is an error - a stop an empty one.
pub(crate) fn analizar(
    dirs: &AppDirs,
    ia: &Ia,
    trabajo: &Trabajo,
    parada: &Parada,
    mut avisar: impl FnMut(Avance),
) -> Result<(), String> {
    let idioma = trabajo.idioma.as_deref();
    let dispositivo = dispositivo().map_err(|motivo| tf("montaje.noWhisper", &[&motivo]))?;
    log::info!("montaje: the clips are analysed with whisper on {dispositivo}");
    let modelos = modelos(dirs, ia, idioma)?;
    let mut detector = Detector::new()
        .inspect_err(|error| log::warn!("montaje: the clips are analysed without faces: {error}"))
        .ok();
    let ajustes = Ajustes {
        idioma: idioma.map(Idioma::from),
        ..Ajustes::default()
    };
    let cache = Cache::new(
        trabajo.cache.clone(),
        ajustes_de_cache(&modelos, &ajustes, idioma, detector.is_some()),
    );
    let mut oido = Oido::new(modelos).map_err(|error| tf("montaje.noWhisper", &[&error.to_string()]))?;
    oido.parar_con(parada.bandera());
    let mut analista = Analista {
        oido: &mut oido,
        caras: detector.as_mut().map(|d| d as &mut dyn BuscaCaras),
        ajustes,
        fotogramas: FOTOGRAMAS_POR_CLIP,
        cache: Some(&cache),
        musica_kit: trabajo.musica_kit.clone(),
    };
    let resultado = trabajo::analizar(&trabajo.carpeta, &mut analista, |avance| {
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
                "montaje: {} clips analysed ({} seen in other jobs), {} failed: {:?}",
                resumen.analizados,
                resumen.de_cache,
                resumen.fallidos.len(),
                resumen.fallidos
            );
            Ok(())
        }
        Err(ErrorTrabajo::Cancelado) => Err(String::new()),
        Err(error) => Err(tf("montaje.analysisFailed", &[&error.to_string()])),
    }
}

/// What a cached clip depends on besides its file: the models, by file
/// name, the prompts, the thresholds, the language asked for, and whether
/// faces were looked for.
fn ajustes_de_cache(
    modelos: &Modelos,
    ajustes: &Ajustes,
    idioma: Option<&str>,
    con_caras: bool,
) -> serde_json::Value {
    let nombre = |ruta: &Path| ruta.file_name().map(|n| n.to_string_lossy().into_owned());
    serde_json::json!({
        "modelo_es": nombre(&modelos.castellano),
        "modelo_eu": nombre(&modelos.euskera),
        "voz": nombre(&modelos.voz),
        "prompt_es": modelos.prompt_castellano,
        "prompt_eu": modelos.prompt_euskera,
        "idioma": idioma,
        "umbral_voz": ajustes.umbral_voz,
        "limpiar": ajustes.limpiar,
        "umbral_ruido_db": ajustes.umbral_ruido_db,
        "fotogramas": FOTOGRAMAS_POR_CLIP,
        "caras": con_caras,
    })
}

/// What whisper would run on - the graphics card, or else the processor -
/// or why it cannot run.
fn dispositivo() -> Result<String, String> {
    let procesador = transcribe::cpu_backend()?;
    Ok(transcribe::gpu_backend().unwrap_or(procesador))
}

/// The whisper models a montage in `idioma` needs, by id, that are not
/// downloaded. Spanish is always needed: it is the one that guesses the
/// language. Basque is not, for a shoot known to be in Spanish.
pub(crate) fn faltan(dirs: &AppDirs, ia: &Ia, idioma: Option<&str>) -> Vec<String> {
    let (es, eu) = ia.whisper();
    let necesarios: &[&str] = if idioma == Some("es") { &[es] } else { &[es, eu] };
    necesarios
        .iter()
        .filter(|id| descargado(dirs, id).is_err())
        .map(|id| (*id).to_owned())
        .collect()
}

/// The model files for `idioma`, or why the analysis cannot run.
fn modelos(dirs: &AppDirs, ia: &Ia, idioma: Option<&str>) -> Result<Modelos, String> {
    let faltan = faltan(dirs, ia, idioma);
    if !faltan.is_empty() {
        return Err(tf("montaje.modelsMissing", &[&faltan.join(", ")]));
    }
    let (es, eu) = ia.whisper();
    let castellano = descargado(dirs, es)?;
    let euskera = descargado(dirs, eu).unwrap_or_else(|_| castellano.clone());
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
