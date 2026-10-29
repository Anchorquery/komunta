//! The Python engine, run for what is not in Rust yet: reading the
//! material (`python -m motor inventariar`), and what the Rust analysis
//! left, describing the clips and asking the AI for the plan
//! (`python -m motor analizar`). It reports progress on its standard
//! output as `[fase  45%] mensaje` lines, which come back here as
//! [`Avance`]s.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::receta::Receta;

/// Where the engine is and what runs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Motor {
    /// The engine's folder: `motor/`, `marcas/`, `trabajos/`, `config.yaml`.
    pub carpeta: PathBuf,
    /// The Python that runs it. Empty is the folder's own `.venv`, or
    /// `python` on the path when there is none.
    pub python: String,
}

impl Motor {
    /// The interpreter to run.
    pub fn interprete(&self) -> PathBuf {
        if !self.python.trim().is_empty() {
            return PathBuf::from(self.python.trim());
        }
        let venv = if cfg!(windows) {
            self.carpeta
                .join(".venv")
                .join("Scripts")
                .join("python.exe")
        } else {
            self.carpeta.join(".venv").join("bin").join("python")
        };
        if venv.is_file() {
            venv
        } else {
            "python".into()
        }
    }

    /// Whether the folder holds an engine.
    pub fn valido(&self) -> bool {
        self.carpeta.join("motor").join("__main__.py").is_file()
    }

    /// The brand kits the engine has, by folder name; `_comun` is not one.
    pub fn marcas(&self) -> Vec<String> {
        let mut marcas: Vec<String> = std::fs::read_dir(self.carpeta.join("marcas"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("kit.yaml").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('_'))
            .collect();
        marcas.sort();
        marcas
    }
}

/// What the team asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pedido {
    /// The folder of footage, photos, audio and documents.
    pub material: PathBuf,
    /// The job's name in `trabajos/`.
    pub id: String,
    /// What kind of video it is.
    pub receta: Receta,
    /// "¿Qué quieres?", in the team's words; may be empty.
    pub peticion: String,
    /// A script that is not in the material folder.
    pub guion: Option<PathBuf>,
    /// "es" or "eu"; None lets the transcription tell.
    pub idioma: Option<String>,
    /// Brand kit id.
    pub marca: Option<String>,
    /// The engine's AI ("claude_code", "anthropic", ...); None is its
    /// configured one.
    pub cerebro: Option<String>,
    /// For an automatic recipe, the ones the AI chooses among.
    pub candidatas: Vec<Receta>,
    /// Added to the engine's environment: the providers' keys and the
    /// run's own settings ([`crate::ia::VARIABLE_AJUSTES`]).
    pub entorno: Vec<(String, String)>,
}

impl Pedido {
    /// The recipe to lay the job's plan out with: the one asked for, or,
    /// for an automatic request, the one the AI wrote in the plan (the
    /// first candidate when it named none), in the frames asked for.
    pub fn receta_final(&self, trabajo: &Path) -> Receta {
        if !self.receta.es_automatica() {
            return self.receta.clone();
        }
        let plantilla = crate::PlanCrudo::leer(trabajo)
            .and_then(|plan| plan.plan())
            .map(|plan| plan.plantilla)
            .unwrap_or_default();
        let mut receta = crate::receta::elegida(&self.candidatas, &plantilla)
            .or_else(|| self.candidatas.iter().find(|r| !r.es_automatica()))
            .cloned()
            .unwrap_or_default();
        receta.formato = self.receta.formato.clone();
        receta.formatos_extra = self.receta.formatos_extra.clone();
        receta
    }

    /// The rules the AI gets: the recipe's, then the request.
    pub fn reglas(&self) -> String {
        let mut reglas = if self.receta.es_automatica() {
            crate::receta::reglas_eleccion(&self.candidatas)
        } else {
            self.receta.reglas()
        };
        if !self.peticion.trim().is_empty() {
            reglas.push_str("\n\n## Lo que pide el equipo para este vídeo\n");
            reglas.push_str(self.peticion.trim());
            reglas.push_str("\nSi esto contradice las reglas anteriores, manda esto.");
        }
        reglas
    }
}

/// One progress report.
#[derive(Clone, Debug, PartialEq)]
pub struct Avance {
    /// "ingesta", "analisis", "descripcion", "plan".
    /// The engine's name for the phase.
    pub fase: String,
    /// 0..=1 within the phase.
    pub fraccion: f64,
    /// What it is doing, in Spanish.
    pub mensaje: String,
}

/// A run that can be stopped from another thread.
#[derive(Clone, Default)]
pub struct Parada {
    /// The engine's process while it runs.
    pid: Arc<Mutex<Option<u32>>>,
    cancelada: Arc<AtomicBool>,
}

impl Parada {
    /// Stops the run and whatever it started (FFmpeg, the AI's CLI).
    pub fn cancelar(&self) {
        self.cancelada.store(true, Ordering::SeqCst);
        let pid = *self.pid.lock().expect("not poisoned");
        let Some(pid) = pid else {
            return;
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(SIN_VENTANA)
                .status();
        }
        #[cfg(not(windows))]
        {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }

    /// Whether [`Parada::cancelar`] was called.
    pub fn cancelada(&self) -> bool {
        self.cancelada.load(Ordering::SeqCst)
    }

    /// The flag [`Parada::cancelar`] raises, for work in this process that
    /// looks at it as it goes: whisper, between its chunks of work.
    pub fn bandera(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelada)
    }
}

#[cfg(windows)]
const SIN_VENTANA: u32 = 0x0800_0000;

/// What can go wrong running the engine.
#[derive(Debug, thiserror::Error)]
pub enum FalloMotor {
    /// Python is not where it was looked for.
    #[error("no se pudo arrancar el motor ({0}): {1}")]
    Arrancar(PathBuf, std::io::Error),
    /// The rules file could not be written.
    #[error("no se pudo preparar el pedido: {0}")]
    Preparar(std::io::Error),
    /// The engine stopped with an error; its last lines.
    #[error("el motor falló:\n{0}")]
    Fallo(String),
    /// [`Parada::cancelar`] stopped it.
    #[error("cancelado")]
    Cancelado,
    /// The engine is set to a manual AI and waits for the plan file.
    #[error("la IA no devolvió el plan; queda pendiente en {0}")]
    SinPlan(PathBuf),
}

impl FalloMotor {
    /// What to tell the person, in a sentence: the cause, not the engine's
    /// traceback, which is for the log.
    pub fn resumen(&self) -> String {
        let texto = match self {
            FalloMotor::Fallo(cola) => resumir(cola),
            otro => otro.to_string(),
        };
        let mut letras = texto.chars();
        letras.next().map_or(texto.clone(), |primera| {
            primera.to_uppercase().chain(letras).collect()
        })
    }
}

/// The last line of a Python traceback is the error itself; the ones a
/// person can do something about are said in plain words.
fn resumir(cola: &str) -> String {
    let ultima = cola
        .lines()
        .rev()
        .map(str::trim)
        .find(|linea| !linea.is_empty())
        .unwrap_or("");
    let bajo = ultima.to_lowercase();
    if bajo.contains("out of memory") {
        return "la tarjeta gráfica se quedó sin memoria. Cierra los programas que la estén usando \
                (juegos, otro editor) y vuelve a intentarlo."
            .into();
    }
    if bajo.starts_with("modulenotfounderror") {
        return format!("al motor le falta una pieza de Python ({ultima}). Revisa su instalación.");
    }
    if ultima.is_empty() {
        return "el motor se paró sin decir por qué.".into();
    }
    // `RuntimeError: lo que pasó` → `lo que pasó`.
    let mensaje = ultima
        .split_once(": ")
        .filter(|(clase, _)| {
            !clase.contains(' ') && (clase.ends_with("Error") || clase.ends_with("Exception"))
        })
        .map_or(ultima, |(_, mensaje)| mensaje);
    format!("el motor falló: {mensaje}")
}

/// Only the ingest: the engine reads the material folder and writes the
/// job's `inventario.json` - the clips, their ids and lengths - so the
/// analysis can run here before [`analizar`]. Returns the job folder.
pub fn inventariar(motor: &Motor, pedido: &Pedido, parada: &Parada) -> Result<PathBuf, FalloMotor> {
    let mut argumentos: Vec<OsString> = vec!["inventariar".into(), pedido.material.clone().into()];
    argumentos.extend(["--id".into(), pedido.id.clone().into()]);
    if let Some(guion) = &pedido.guion {
        argumentos.extend(["--guion".into(), guion.clone().into()]);
    }
    let mut trabajo = None;
    let ultimas = ejecutar(motor, pedido, argumentos, parada, |linea| {
        if let Some(inventario) = linea.strip_prefix("Inventario: ") {
            trabajo = Path::new(inventario.trim()).parent().map(PathBuf::from);
        }
    })?;
    trabajo.ok_or_else(|| FalloMotor::Fallo(ultimas.join("\n")))
}

/// Ingest, analysis and plan for a request. Calls `avisar` with each
/// progress line; returns the job folder once `ia/plan.json` is there.
/// Clips already in the job's `analisis.json` are not analysed again.
pub fn analizar(
    motor: &Motor,
    pedido: &Pedido,
    parada: &Parada,
    mut avisar: impl FnMut(Avance),
) -> Result<PathBuf, FalloMotor> {
    let reglas = std::env::temp_dir().join(format!("komunta-reglas-{}.md", pedido.id));
    std::fs::write(&reglas, pedido.reglas()).map_err(FalloMotor::Preparar)?;
    let mut argumentos: Vec<OsString> = vec!["analizar".into(), pedido.material.clone().into()];
    argumentos.extend(["--id".into(), pedido.id.clone().into()]);
    argumentos.extend(["--plantilla".into(), pedido.receta.id.clone().into()]);
    argumentos.extend(["--reglas".into(), reglas.clone().into()]);
    let opcionales = [
        ("--guion", pedido.guion.clone().map(OsString::from)),
        ("--idioma", pedido.idioma.clone().map(OsString::from)),
        ("--marca", pedido.marca.clone().map(OsString::from)),
        ("--ia", pedido.cerebro.clone().map(OsString::from)),
    ];
    for (bandera, valor) in opcionales {
        if let Some(valor) = valor {
            argumentos.extend([bandera.into(), valor]);
        }
    }
    let mut trabajo = None;
    let mut sin_plan = None;
    let corrida = ejecutar(motor, pedido, argumentos, parada, |linea| {
        if let Some(avance) = leer_avance(linea) {
            avisar(avance);
        } else if let Some(plan) = linea.strip_prefix("Plan: ") {
            trabajo = Path::new(plan.trim())
                .parent()
                .and_then(Path::parent)
                .map(PathBuf::from);
        } else if let Some(resto) = linea.strip_prefix("Pedido listo: ") {
            sin_plan = resto.split(" -> ").next().map(|p| PathBuf::from(p.trim()));
        }
    });
    let _ = std::fs::remove_file(&reglas);
    let ultimas = corrida?;
    match (trabajo, sin_plan) {
        (Some(trabajo), _) => Ok(trabajo),
        (None, Some(pedido)) => Err(FalloMotor::SinPlan(pedido)),
        (None, None) => Err(FalloMotor::Fallo(ultimas.join("\n"))),
    }
}

/// Runs `python -m motor <argumentos>` for `pedido`, handing each line of
/// its standard output to `leer`, until it ends or `parada` stops it.
/// Returns its last lines when it ends well; when it does not, the tail of
/// its standard error - the traceback - as the failure.
fn ejecutar(
    motor: &Motor,
    pedido: &Pedido,
    argumentos: Vec<OsString>,
    parada: &Parada,
    mut leer: impl FnMut(&str),
) -> Result<Vec<String>, FalloMotor> {
    let python = motor.interprete();
    let mut orden = Command::new(&python);
    orden
        .current_dir(&motor.carpeta)
        .args(["-X", "utf8", "-u", "-m", "motor"])
        .args(argumentos)
        .env("PYTHONIOENCODING", "utf-8")
        .envs(
            pedido
                .entorno
                .iter()
                .map(|(variable, valor)| (variable, valor)),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        orden.creation_flags(SIN_VENTANA);
    }
    if parada.cancelada() {
        return Err(FalloMotor::Cancelado);
    }
    let mut hijo: Child = orden
        .spawn()
        .map_err(|e| FalloMotor::Arrancar(python.clone(), e))?;
    *parada.pid.lock().expect("not poisoned") = Some(hijo.id());
    if parada.cancelada() {
        // Stopped while it was starting.
        parada.cancelar();
    }

    // Standard error on a thread of its own, so a chatty run cannot fill
    // the pipe and stall; its tail is the error message.
    let mut err = hijo.stderr.take().expect("piped");
    let errores = std::thread::spawn(move || {
        let mut texto = String::new();
        let _ = err.read_to_string(&mut texto);
        texto
    });
    let mut ultimas: Vec<String> = Vec::new();
    for linea in BufReader::new(hijo.stdout.take().expect("piped")).lines() {
        let Ok(linea) = linea else { break };
        leer(&linea);
        ultimas.push(linea);
        if ultimas.len() > 20 {
            ultimas.remove(0);
        }
    }
    let estado = hijo.wait();
    *parada.pid.lock().expect("not poisoned") = None;
    let errores = errores.join().unwrap_or_default();
    if parada.cancelada() {
        return Err(FalloMotor::Cancelado);
    }
    if estado.is_ok_and(|s| s.success()) {
        return Ok(ultimas);
    }
    let cola: Vec<&str> = errores.lines().rev().take(12).collect();
    let mut texto: Vec<&str> = cola.into_iter().rev().collect();
    if texto.is_empty() {
        texto = ultimas.iter().map(String::as_str).collect();
    }
    Err(FalloMotor::Fallo(texto.join("\n")))
}

/// `[analisis  45%] IMG_8441.MOV: 12 palabras` as an [`Avance`].
pub fn leer_avance(linea: &str) -> Option<Avance> {
    let resto = linea.strip_prefix('[')?;
    let (cabeza, mensaje) = resto.split_once(']')?;
    let mut partes = cabeza.split_whitespace();
    let fase = partes.next()?.to_owned();
    let fraccion = partes.next()?.strip_suffix('%')?.parse::<f64>().ok()? / 100.0;
    Some(Avance {
        fase,
        fraccion: fraccion.clamp(0.0, 1.0),
        mensaje: mensaje.trim().to_owned(),
    })
}

/// A job id from a name: letters, digits, `_` and `-`, with the date in
/// front so two jobs of the same name do not meet.
pub fn id_trabajo(nombre: &str, fecha: &str) -> String {
    let limpio: String = nombre
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let limpio = limpio.trim_matches('_');
    let limpio = if limpio.is_empty() { "montaje" } else { limpio };
    format!("{fecha}_{limpio}")
}

/// Now, in UTC, as `AAAAMMDD_HHMMSS`: the date in front of a job id.
pub fn ahora() -> String {
    let segundos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (dias, resto) = (segundos / 86_400, segundos % 86_400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = dias as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}_{:02}{:02}{:02}",
        resto / 3600,
        resto % 3600 / 60,
        resto % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_told_in_a_sentence() {
        let traza = "Traceback (most recent call last):\n  File \"x.py\", line 60, in pasada\n    \
                     return list(segs), info\nRuntimeError: CUDA failed with error out of memory\n";
        let dicho = FalloMotor::Fallo(traza.into()).resumen();
        assert!(
            dicho.starts_with("La tarjeta gráfica se quedó sin memoria"),
            "{dicho}"
        );
        assert!(!dicho.contains("Traceback"));

        let otro = "Traceback...\nValueError: el plan no tiene tomas\n\n";
        assert_eq!(
            FalloMotor::Fallo(otro.into()).resumen(),
            "El motor falló: el plan no tiene tomas"
        );
        assert_eq!(
            FalloMotor::Fallo("algo raro: sin clase".into()).resumen(),
            "El motor falló: algo raro: sin clase"
        );
        assert_eq!(
            FalloMotor::Fallo(String::new()).resumen(),
            "El motor se paró sin decir por qué."
        );
        assert!(
            FalloMotor::Fallo("ModuleNotFoundError: No module named 'cv2'".into())
                .resumen()
                .contains("le falta una pieza de Python")
        );
    }

    #[test]
    fn progress_lines_are_read() {
        let a = leer_avance("[analisis  45%] IMG_8441.MOV: 12 palabras").unwrap();
        assert_eq!(a.fase, "analisis");
        assert!((a.fraccion - 0.45).abs() < 1e-9);
        assert_eq!(a.mensaje, "IMG_8441.MOV: 12 palabras");
        assert_eq!(
            leer_avance("[plan 100%] Plan: 3 tomas").unwrap().fraccion,
            1.0
        );
        assert!(leer_avance("Plan: C:/x/ia/plan.json").is_none());
    }

    #[test]
    fn the_request_follows_the_recipe() {
        let pedido = Pedido {
            receta: crate::receta::de_serie().remove(0),
            peticion: "Que dure 30 segundos".into(),
            ..Pedido::default()
        };
        let reglas = pedido.reglas();
        assert!(reglas.ends_with("Si esto contradice las reglas anteriores, manda esto."));
        assert!(reglas.contains("Que dure 30 segundos"));
    }

    #[test]
    fn job_ids_are_safe_folder_names() {
        assert_eq!(
            id_trabajo("Andoain: acto/2", "20261028"),
            "20261028_Andoain__acto_2"
        );
        assert_eq!(id_trabajo("  ", "20261028"), "20261028_montaje");
    }
}
