//! A command-line AI run to the end, or stopped: when it takes longer than
//! its limit, or the montage is cancelled.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How a run ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Salida {
    /// Its exit code; None when it was killed.
    pub codigo: Option<i32>,
    /// What it wrote, lossily as UTF-8.
    pub stdout: String,
    /// What it complained of.
    pub stderr: String,
}

/// Runs `orden` - its standard input `entrada` - until it ends, `limite`
/// passes or `parada` is set; the last two kill it and are errors.
pub fn correr(
    orden: &mut Command,
    entrada: Option<&str>,
    limite: Duration,
    parada: &Arc<AtomicBool>,
) -> Result<Salida, String> {
    orden.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut hijo = orden.spawn().map_err(|e| format!("no se pudo lanzar {:?}: {e}", orden.get_program()))?;
    let datos = entrada.unwrap_or_default().to_owned();
    if let Some(mut stdin) = hijo.stdin.take() {
        std::thread::spawn(move || stdin.write_all(datos.as_bytes()));
    }
    let stdout = leer_en_hilo(hijo.stdout.take());
    let stderr = leer_en_hilo(hijo.stderr.take());
    let codigo = esperar(&mut hijo, limite, parada)?;
    Ok(Salida {
        codigo,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

fn leer_en_hilo(fuente: Option<impl Read + Send + 'static>) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut fuente) = fuente {
            let _ = fuente.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn esperar(hijo: &mut Child, limite: Duration, parada: &Arc<AtomicBool>) -> Result<Option<i32>, String> {
    let inicio = Instant::now();
    loop {
        if let Some(estado) = hijo.try_wait().map_err(|e| e.to_string())? {
            return Ok(estado.code());
        }
        let motivo = if parada.load(Ordering::Relaxed) {
            Some("cancelado".to_owned())
        } else if inicio.elapsed() > limite {
            Some(format!("no respondió en {} s", limite.as_secs()))
        } else {
            None
        };
        if let Some(motivo) = motivo {
            let _ = hijo.kill();
            let _ = hijo.wait();
            return Err(motivo);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Where program `nombre` is: on the `PATH` (with Windows' extensions), or
/// the first of `alternativas` that exists.
pub fn ejecutable(nombre: &str, alternativas: &[PathBuf]) -> Result<PathBuf, String> {
    let extensiones = std::env::var("PATHEXT").unwrap_or_default();
    let extensiones: Vec<&str> = std::iter::once("").chain(extensiones.split(';').filter(|e| !e.is_empty())).collect();
    let rutas = std::env::var_os("PATH").unwrap_or_default();
    let en_path = std::env::split_paths(&rutas).find_map(|dir| {
        extensiones
            .iter()
            .map(|ext| dir.join(format!("{nombre}{}", ext.to_lowercase())))
            .find(|ruta| ruta.is_file())
    });
    en_path
        .or_else(|| alternativas.iter().find(|r| r.is_file()).cloned())
        .ok_or_else(|| format!("No encuentro el programa «{nombre}». ¿Está instalado y en el PATH?"))
}

/// The last `n` characters of `texto`: the end of a program's complaint.
pub fn cola(texto: &str, n: usize) -> String {
    let total = texto.chars().count();
    texto.chars().skip(total.saturating_sub(n)).collect()
}

/// A command for `exe` run in `carpeta`, without the environment variables
/// `quitar`: the API keys, so the AI uses the team's subscription and does
/// not charge by use.
pub fn orden(exe: &Path, carpeta: &Path, quitar: &[&str]) -> Command {
    let mut orden = Command::new(exe);
    orden.current_dir(carpeta);
    for variable in quitar {
        orden.env_remove(variable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up from the app.
        orden.creation_flags(0x0800_0000);
    }
    orden
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_of_a_complaint_is_kept() {
        assert_eq!(cola("abcdef", 3), "def");
        assert_eq!(cola("ab", 3), "ab");
    }

    #[test]
    fn a_missing_program_is_told_by_name() {
        let error = ejecutable("no-existe-komunta", &[]).unwrap_err();
        assert!(error.contains("«no-existe-komunta»"));
    }

    #[test]
    fn a_program_runs_and_a_slow_one_is_stopped() {
        let shell = if cfg!(windows) { ("cmd", ["/C", "echo hola"]) } else { ("sh", ["-c", "echo hola"]) };
        let parada = Arc::new(AtomicBool::new(false));
        let mut eco = Command::new(shell.0);
        eco.args(shell.1);
        let salida = correr(&mut eco, None, Duration::from_secs(20), &parada).unwrap();
        assert_eq!((salida.codigo, salida.stdout.trim()), (Some(0), "hola"));

        let lento = if cfg!(windows) { ("powershell", ["-Command", "Start-Sleep 30"]) } else { ("sh", ["-c", "sleep 30"]) };
        let mut espera = Command::new(lento.0);
        espera.args(lento.1);
        let inicio = Instant::now();
        let error = correr(&mut espera, None, Duration::from_millis(300), &parada).unwrap_err();
        assert!(error.starts_with("no respondió"));
        assert!(inicio.elapsed() < Duration::from_secs(10));
    }
}
