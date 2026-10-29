//! The Python engine, run for what is not in Rust yet: reading the
//! material, transcribing it and asking the AI for the plan
//! (`python -m motor analizar`). It reports progress on its standard output
//! as `[fase  45%] mensaje` lines, which come back here as [`Avance`]s.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
pub struct Parada(Arc<Mutex<(Option<u32>, bool)>>);

impl Parada {
    /// Stops the run and whatever it started (FFmpeg, the AI's CLI).
    pub fn cancelar(&self) {
        let pid = {
            let mut estado = self.0.lock().expect("not poisoned");
            estado.1 = true;
            estado.0
        };
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
        self.0.lock().expect("not poisoned").1
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

/// Ingest, analysis and plan for a request. Calls `avisar` with each
/// progress line; returns the job folder once `ia/plan.json` is there.
pub fn analizar(
    motor: &Motor,
    pedido: &Pedido,
    parada: &Parada,
    mut avisar: impl FnMut(Avance),
) -> Result<PathBuf, FalloMotor> {
    let reglas = std::env::temp_dir().join(format!("komunta-reglas-{}.md", pedido.id));
    std::fs::write(&reglas, pedido.reglas()).map_err(FalloMotor::Preparar)?;
    let python = motor.interprete();
    let mut orden = Command::new(&python);
    orden
        .current_dir(&motor.carpeta)
        .args(["-X", "utf8", "-u", "-m", "motor", "analizar"])
        .arg(&pedido.material)
        .args(["--id", &pedido.id, "--plantilla", &pedido.receta.id])
        .arg("--reglas")
        .arg(&reglas)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(guion) = &pedido.guion {
        orden.arg("--guion").arg(guion);
    }
    if let Some(idioma) = &pedido.idioma {
        orden.args(["--idioma", idioma]);
    }
    if let Some(marca) = &pedido.marca {
        orden.args(["--marca", marca]);
    }
    if let Some(cerebro) = &pedido.cerebro {
        orden.args(["--ia", cerebro]);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        orden.creation_flags(SIN_VENTANA);
    }
    let mut hijo: Child = orden
        .spawn()
        .map_err(|e| FalloMotor::Arrancar(python.clone(), e))?;
    parada.0.lock().expect("not poisoned").0 = Some(hijo.id());
    if parada.cancelada() {
        // Stopped before it had started.
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
    let mut trabajo = None;
    let mut sin_plan = None;
    let mut ultimas: Vec<String> = Vec::new();
    for linea in BufReader::new(hijo.stdout.take().expect("piped")).lines() {
        let Ok(linea) = linea else { break };
        if let Some(avance) = leer_avance(&linea) {
            avisar(avance);
        } else if let Some(plan) = linea.strip_prefix("Plan: ") {
            trabajo = Path::new(plan.trim())
                .parent()
                .and_then(Path::parent)
                .map(PathBuf::from);
        } else if let Some(resto) = linea.strip_prefix("Pedido listo: ") {
            sin_plan = resto.split(" -> ").next().map(|p| PathBuf::from(p.trim()));
        }
        ultimas.push(linea);
        if ultimas.len() > 20 {
            ultimas.remove(0);
        }
    }
    let estado = hijo.wait();
    parada.0.lock().expect("not poisoned").0 = None;
    let _ = std::fs::remove_file(&reglas);
    let errores = errores.join().unwrap_or_default();
    if parada.cancelada() {
        return Err(FalloMotor::Cancelado);
    }
    match (estado, trabajo, sin_plan) {
        (Ok(s), Some(trabajo), _) if s.success() => Ok(trabajo),
        (Ok(s), None, Some(pedido)) if s.success() => Err(FalloMotor::SinPlan(pedido)),
        _ => {
            let cola: Vec<&str> = errores.lines().rev().take(12).collect();
            let mut texto: Vec<&str> = cola.into_iter().rev().collect();
            if texto.is_empty() {
                texto = ultimas.iter().map(String::as_str).collect();
            }
            Err(FalloMotor::Fallo(texto.join("\n")))
        }
    }
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
