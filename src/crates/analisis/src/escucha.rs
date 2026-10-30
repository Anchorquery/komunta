//! The listening half: whisper.cpp hears the clip, finds where the voice
//! is (its own Silero detector) and what is said, token by token, each
//! token aligned to the sound. What it hears goes to the plain modules,
//! which decide.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext,
    WhisperContextParameters, WhisperSegment, WhisperState, WhisperTokenId, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
};

use crate::idioma::{self, Segunda};
use crate::recorte::{self, Mapa};
use crate::tokens::{self, TokenOido};
use crate::{Idioma, Tramo, Transcripcion};

/// A pause this long splits the voice into stretches, for the analysis.
const PAUSA_VOZ_MS: i32 = 300;

/// Only pauses this long are cut out of what whisper hears: shorter ones
/// are part of how somebody talks.
const PAUSA_RECORTE_MS: i32 = 2000;

/// Why listening failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// whisper's CPU kernels are missing from the install.
    #[error("{0}")]
    Cpu(String),
    /// A model file would not load.
    #[error("could not load {}: {motivo}", ruta.display())]
    Modelo {
        /// The file.
        ruta: PathBuf,
        /// What whisper said.
        motivo: String,
    },
    /// whisper failed mid-run.
    #[error("whisper failed: {0}")]
    Whisper(#[from] whisper_rs::WhisperError),
    /// The flag given to [`Oido::parar_con`] was raised.
    #[error("stopped")]
    Parado,
}

/// The engine's opening line for Spanish (`transcripcion.prompt_es`).
pub const PROMPT_CASTELLANO: &str = "Eh, pues, vale, bueno, o sea, este... Hoy quiero explicarte algo.";

/// The engine's opening line for Basque (`transcripcion.prompt_eu`).
pub const PROMPT_EUSKERA: &str = "Ba, beno, eh, ba... Gaur zerbait azaldu nahi dizut.";

/// The model files and the prompts a run uses.
#[derive(Clone, Debug)]
pub struct Modelos {
    /// The whisper model for Spanish, and for guessing the language.
    pub castellano: PathBuf,
    /// The whisper model for Basque; the same file as `castellano` for
    /// one model serving both.
    pub euskera: PathBuf,
    /// The Silero voice detector, as whisper.cpp ships it.
    pub voz: PathBuf,
    /// An opening line in Spanish with its fillers: without one whisper
    /// tidies away the "eh" and "pues" a cut needs to see.
    pub prompt_castellano: Option<String>,
    /// The same, in Basque.
    pub prompt_euskera: Option<String>,
}

/// whisper, with its models loaded as they are first needed and kept.
pub struct Oido {
    modelos: Modelos,
    cargados: HashMap<PathBuf, WhisperContext>,
    detector: Option<WhisperVadContext>,
    hilos: usize,
    parada: Arc<AtomicBool>,
}

/// One run of whisper over a clip's speech, in one language.
struct Pasada {
    idioma: Idioma,
    probabilidad: f64,
    segmentos: Vec<Vec<TokenOido>>,
    /// From the speech whisper heard back to the clip.
    mapa: Mapa,
}

impl Pasada {
    fn tramos(&self) -> Vec<Tramo> {
        self.segmentos.iter().map(|segmento| tokens::tramo(segmento)).collect()
    }

    fn transcripcion(&self) -> Transcripcion {
        let oidas = self
            .segmentos
            .iter()
            .flat_map(|segmento| tokens::palabras_de(segmento))
            .map(|palabra| self.mapa.devolver(palabra));
        Transcripcion::nueva(oidas, &self.idioma, self.probabilidad)
    }
}

impl Oido {
    /// Ready to listen; nothing is loaded until it is first needed.
    pub fn new(modelos: Modelos) -> Result<Self, Error> {
        concat_speech::transcribe::cpu_backend().map_err(Error::Cpu)?;
        Ok(Self {
            modelos,
            cargados: HashMap::new(),
            detector: None,
            hilos: hilos(),
            parada: Arc::default(),
        })
    }

    /// Stops whisper where it stands once `parada` is raised: the clip
    /// being heard then fails with [`Error::Parado`].
    pub fn parar_con(&mut self, parada: Arc<AtomicBool>) {
        self.parada = parada;
    }

    /// Where in `audio` (16 kHz mono) somebody speaks, in seconds.
    pub fn voz(&mut self, audio: &[f32]) -> Result<Vec<(f64, f64)>, Error> {
        self.tramos_de_voz(audio, PAUSA_VOZ_MS)
    }

    /// The voice's stretches, split where it pauses for `pausa_ms`.
    fn tramos_de_voz(&mut self, audio: &[f32], pausa_ms: i32) -> Result<Vec<(f64, f64)>, Error> {
        let detector = match &mut self.detector {
            Some(detector) => detector,
            None => self.detector.insert(cargar_detector(&self.modelos.voz, self.hilos)?),
        };
        let segmentos = detector.segments_from_samples(deteccion(pausa_ms), audio)?;
        Ok(segmentos
            .map(|segmento| (f64::from(segmento.start) / 100.0, f64::from(segmento.end) / 100.0))
            .collect())
    }

    /// What is said in `audio` (16 kHz mono). With no `idioma`, whisper
    /// guesses it, and a guess of Basque, or an unsure Spanish one, gets a
    /// second pass with the Basque model ([`crate::idioma`]).
    pub fn transcribir(&mut self, audio: &[f32], idioma: Option<&Idioma>) -> Result<Transcripcion, Error> {
        let primera = self.pasada(audio, idioma)?;
        let un_solo_modelo = self.modelos.castellano == self.modelos.euskera;
        let segunda = idioma::segunda_pasada(&primera.idioma, primera.probabilidad, idioma.is_some(), un_solo_modelo);
        let elegida = match segunda {
            Segunda::Ninguna => primera,
            Segunda::Euskera => self.pasada(audio, Some(&Idioma::euskera()))?,
            Segunda::Contrastar => {
                let euskera = self.pasada(audio, Some(&Idioma::euskera()))?;
                if idioma::gana_la_segunda(&primera.tramos(), &euskera.tramos()) {
                    euskera
                } else {
                    primera
                }
            }
        };
        Ok(elegida.transcripcion())
    }

    /// One run over the clip's speech, in `idioma` or in the language
    /// whisper guesses. A given language is sure; with no speech, nothing
    /// is heard.
    fn pasada(&mut self, audio: &[f32], idioma: Option<&Idioma>) -> Result<Pasada, Error> {
        let euskera = idioma.is_some_and(|idioma| *idioma == Idioma::euskera());
        let (ruta, prompt) = if euskera {
            (self.modelos.euskera.clone(), self.modelos.prompt_euskera.clone())
        } else {
            (self.modelos.castellano.clone(), self.modelos.prompt_castellano.clone())
        };
        let voz = self.tramos_de_voz(audio, PAUSA_RECORTE_MS)?;
        let (habla, mapa) = recorte::recortar(audio, &voz);
        if habla.is_empty() {
            let idioma = idioma.cloned().unwrap_or_else(Idioma::castellano);
            return Ok(Pasada { idioma, probabilidad: 0.0, segmentos: Vec::new(), mapa });
        }
        let hilos = self.hilos;
        let parada = Arc::clone(&self.parada);
        let contexto = self.contexto(&ruta)?;
        let mut estado = contexto.create_state()?;
        let (idioma, probabilidad) = match idioma {
            Some(idioma) => (idioma.clone(), 1.0),
            None => adivinar(&mut estado, &habla, hilos)?,
        };
        let mut parametros = parametros(idioma.codigo(), prompt.as_deref(), hilos);
        // SAFETY: `abort_when_set` reads the `AtomicBool` inside `parada`,
        // which is held until `full` has returned.
        unsafe {
            parametros.set_abort_callback(Some(concat_speech::transcribe::abort_when_set));
            parametros.set_abort_callback_user_data(Arc::as_ptr(&parada) as *mut std::ffi::c_void);
        }
        let oido = estado.full(parametros, &habla);
        if parada.load(Ordering::Relaxed) {
            return Err(Error::Parado);
        }
        oido?;
        let fin = contexto.token_eot();
        let segmentos = estado.as_iter().map(|segmento| tokens_de(&segmento, fin)).collect();
        Ok(Pasada { idioma, probabilidad, segmentos, mapa })
    }

    fn contexto(&mut self, ruta: &Path) -> Result<&WhisperContext, Error> {
        if !self.cargados.contains_key(ruta) {
            let mut parametros = WhisperContextParameters::default();
            // On the graphics card when ggml has one (a Vulkan module beside
            // the CPU ones); whisper-rs leaves it off unless built for one.
            parametros.use_gpu(concat_speech::transcribe::gpu_backend().is_some());
            if let Some(model_preset) = alineacion(ruta) {
                parametros.dtw_parameters(DtwParameters {
                    mode: DtwMode::ModelPreset { model_preset },
                    ..DtwParameters::default()
                });
            }
            let contexto = WhisperContext::new_with_params(ruta, parametros)
                .map_err(|error| Error::Modelo { ruta: ruta.to_owned(), motivo: error.to_string() })?;
            log::info!("analisis: whisper model loaded from {}", ruta.display());
            self.cargados.insert(ruta.to_owned(), contexto);
        }
        Ok(&self.cargados[ruta])
    }
}

/// whisper's guess at the language, from the first thirty seconds, between
/// Spanish and Basque ([`idioma::entre_los_dos`]), and how sure it is.
fn adivinar(estado: &mut WhisperState, audio: &[f32], hilos: usize) -> Result<(Idioma, f64), Error> {
    estado.pcm_to_mel(audio, hilos)?;
    let (_, probabilidades) = estado.lang_detect(0, hilos)?;
    let de = |codigo: &str| {
        whisper_rs::get_lang_id(codigo)
            .and_then(|id| probabilidades.get(usize::try_from(id).ok()?))
            .map_or(0.0, |&p| f64::from(p))
    };
    Ok(idioma::entre_los_dos(de("es"), de("eu")))
}

/// The engine's settings: beam of five, no temperature fallback, each
/// window heard on its own, token times on.
fn parametros<'a>(idioma: &'a str, prompt: Option<&str>, hilos: usize) -> FullParams<'a, 'a> {
    let mut parametros = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 });
    parametros.set_n_threads(hilos as i32);
    parametros.set_language(Some(idioma));
    parametros.set_temperature(0.0);
    parametros.set_temperature_inc(0.0);
    parametros.set_no_context(true);
    parametros.set_token_timestamps(true);
    parametros.set_suppress_blank(true);
    if let Some(prompt) = prompt {
        parametros.set_initial_prompt(prompt);
    }
    parametros.set_print_special(false);
    parametros.set_print_progress(false);
    parametros.set_print_realtime(false);
    parametros.set_print_timestamps(false);
    parametros
}

/// The alignment heads for the model at `ruta`, by its name: what times
/// each token against the sound. A fine-tune keeps its base's heads.
fn alineacion(ruta: &Path) -> Option<DtwModelPreset> {
    let nombre = ruta.file_stem()?.to_string_lossy().into_owned();
    let es = |fragmento: &str| nombre.contains(fragmento);
    Some(if es("large-v3-turbo") {
        DtwModelPreset::LargeV3Turbo
    } else if es("large-v3") {
        DtwModelPreset::LargeV3
    } else if es("large-v2") {
        DtwModelPreset::LargeV2
    } else if es("large") {
        DtwModelPreset::LargeV1
    } else if es("medium.en") {
        DtwModelPreset::MediumEn
    } else if es("medium") {
        DtwModelPreset::Medium
    } else if es("small.en") {
        DtwModelPreset::SmallEn
    } else if es("small") {
        DtwModelPreset::Small
    } else if es("base.en") {
        DtwModelPreset::BaseEn
    } else if es("base") {
        DtwModelPreset::Base
    } else if es("tiny.en") {
        DtwModelPreset::TinyEn
    } else if es("tiny") {
        DtwModelPreset::Tiny
    } else {
        return None;
    })
}

/// The voice detector's settings, as the engine had them: any speech
/// counts, padded by 0.4 s, and a pause splits it once it lasts
/// `pausa_ms`.
fn deteccion(pausa_ms: i32) -> WhisperVadParams {
    let mut parametros = WhisperVadParams::new();
    parametros.set_threshold(0.5);
    parametros.set_min_speech_duration(0);
    parametros.set_min_silence_duration(pausa_ms);
    parametros.set_speech_pad(400);
    parametros
}

fn cargar_detector(ruta: &Path, hilos: usize) -> Result<WhisperVadContext, Error> {
    let mut parametros = WhisperVadContextParams::new();
    parametros.set_n_threads(hilos as i32);
    WhisperVadContext::new(&ruta.to_string_lossy(), parametros)
        .map_err(|error| Error::Modelo { ruta: ruta.to_owned(), motivo: error.to_string() })
}

fn tokens_de(segmento: &WhisperSegment<'_>, fin: WhisperTokenId) -> Vec<TokenOido> {
    (0..segmento.n_tokens())
        .filter_map(|indice| segmento.get_token(indice))
        .map(|token| {
            let datos = token.token_data();
            TokenOido {
                bytes: token.to_bytes().map(<[u8]>::to_vec).unwrap_or_default(),
                probabilidad: f64::from(datos.p),
                logprob: f64::from(datos.plog),
                t0: datos.t0,
                t1: datos.t1,
                alineado: (datos.t_dtw >= 0).then_some(datos.t_dtw),
                especial: datos.id >= fin,
            }
        })
        .collect()
}

/// Two hardware threads left to the rest of the app, as the transcriber
/// does: the window and playback stay smooth while whisper runs.
fn hilos() -> usize {
    std::thread::available_parallelism()
        .map(|cuenta| cuenta.get().saturating_sub(2).clamp(1, 8))
        .unwrap_or(4)
}
