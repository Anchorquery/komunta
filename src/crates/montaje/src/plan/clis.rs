//! Planners through a command-line AI, on the subscription already signed
//! in on this computer - no API key. Each runs inside the job's folder,
//! read-only: it reads `ia/pedido.md` and the sheets in `hojas/` and
//! answers with the plan. What is long goes in files; the order on the
//! command line is one line, as Windows' `.cmd` wrappers need. The
//! engine's `motor/cerebros/clis.py`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::{Value, json};

use super::cadena::Entorno;
use super::cerebro::{Conector, Fallo};
use super::proceso::{Salida, cola, correr, ejecutable, orden};

/// What the planner is told to do, `{archivo}` being the prompt's file.
const ORDEN: &str = "Eres el cerebro de un motor de montaje de vídeo. Lee {archivo} (instrucciones y datos) y mira TODAS \
     las imágenes de hojas/ (hojas de contacto de los clips). Responde ÚNICAMENTE con el JSON del plan de \
     montaje, sin texto antes ni después.";
const ORDEN_CORRECCION: &str = "Lee ia/correccion.md (errores de tu plan anterior, que está en ia/respuesta_previa.md) y \
     responde ÚNICAMENTE con el JSON completo corregido.";
const ARCHIVO_PEDIDO: &str = "ia/pedido.md";

/// What every command-line planner keeps.
struct Comun {
    nombre: &'static str,
    modelo: Option<String>,
    limite: Duration,
    parada: Arc<AtomicBool>,
    uso: Vec<Value>,
}

impl Comun {
    fn new(nombre: &'static str, entorno: &Entorno) -> Self {
        Self {
            nombre,
            modelo: entorno.opciones(nombre)["modelo"].as_str().map(str::to_owned),
            limite: entorno.limite(),
            parada: entorno.parada.clone(),
            uso: Vec::new(),
        }
    }

    /// Runs `exe` with `argumentos` in `trabajo`, without the keys `quitar`.
    fn correr(
        &mut self,
        exe: &Path,
        argumentos: &[String],
        trabajo: &Path,
        quitar: &[&str],
        entrada: Option<&str>,
    ) -> Result<Salida, Fallo> {
        let mut comando = orden(exe, trabajo, quitar);
        comando.args(argumentos);
        let salida = correr(&mut comando, entrada, self.limite, &self.parada)
            .map_err(|e| Fallo::Otro(format!("{}: {e}", self.nombre)))?;
        let programa = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.uso.push(json!({"cmd": programa, "codigo": salida.codigo}));
        Ok(salida)
    }

    fn fallo(&self, que: &str, salida: &Salida) -> Fallo {
        let detalle = if salida.stderr.trim().is_empty() { &salida.stdout } else { &salida.stderr };
        let codigo = salida.codigo.map_or("ninguno".into(), |c| c.to_string());
        Fallo::Otro(format!("{}: {que} (código {codigo}): {}", self.nombre, cola(detalle, 1500)))
    }

    /// `ia/respuesta_previa.md` - its last answer - and `ia/correccion.md`
    /// - what is wrong with it - for a planner that keeps no session.
    fn preparar_correccion(&self, trabajo: &Path, mensaje: &str) -> std::io::Result<()> {
        let ia = trabajo.join("ia");
        let prefijo = format!("respuesta_{}_", self.nombre);
        let mut previas: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&ia)?
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefijo))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        previas.sort();
        if let Some((_, ultima)) = previas.last() {
            std::fs::copy(ultima, ia.join("respuesta_previa.md"))?;
        }
        std::fs::write(ia.join("correccion.md"), mensaje)
    }

    fn con_modelo(&self, argumentos: &mut Vec<String>, bandera: &str) {
        if let Some(modelo) = &self.modelo {
            argumentos.extend([bandera.to_owned(), modelo.clone()]);
        }
    }
}

fn cadenas(argumentos: &[&str]) -> Vec<String> {
    argumentos.iter().map(|a| (*a).to_owned()).collect()
}

/// `claude -p` with read-only tools; the correction resumes its session.
pub struct ClaudeCode {
    comun: Comun,
    sesion: Option<String>,
}

impl ClaudeCode {
    /// Claude Code as `ia.claude_code` sets it.
    pub fn new(entorno: &Entorno) -> Self {
        Self { comun: Comun::new("claude_code", entorno), sesion: None }
    }

    fn llamar(&mut self, trabajo: &Path, orden: &str, sesion: Option<String>) -> Result<String, Fallo> {
        let casa = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
        let alternativa = PathBuf::from(casa).join(".local").join("bin").join("claude.exe");
        let exe = ejecutable("claude", &[alternativa]).map_err(Fallo::Otro)?;
        let mut argumentos = cadenas(&["-p", "--output-format", "json", "--allowedTools", "Read", "Glob",
            "--permission-mode", "dontAsk"]);
        self.comun.con_modelo(&mut argumentos, "--model");
        if let Some(sesion) = sesion {
            argumentos.extend(["--resume".to_owned(), sesion]);
        }
        argumentos.push(orden.to_owned());
        let salida = self.comun.correr(&exe, &argumentos, trabajo, &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"], None)?;
        let respuesta: Value = serde_json::from_str(&salida.stdout)
            .map_err(|_| self.comun.fallo("respuesta no JSON", &salida))?;
        if let Some(uso) = self.comun.uso.last_mut() {
            uso["sesion"] = respuesta["session_id"].clone();
            uso["turnos"] = respuesta["num_turns"].clone();
            uso["duracion_ms"] = respuesta["duration_ms"].clone();
            uso["coste_usd"] = respuesta["total_cost_usd"].clone();
        }
        if respuesta["is_error"].as_bool() == Some(true) {
            return Err(Fallo::Otro(format!(
                "claude_code: {} (si la sesión expiró: `claude auth login`)",
                crate::python::texto(&respuesta["result"])
            )));
        }
        self.sesion = respuesta["session_id"].as_str().map(str::to_owned);
        Ok(respuesta["result"].as_str().unwrap_or_default().to_owned())
    }
}

impl Conector for ClaudeCode {
    fn nombre(&self) -> &'static str {
        self.comun.nombre
    }

    fn primera(&mut self, trabajo: &Path, _: &str, _: &[PathBuf]) -> Result<String, Fallo> {
        self.llamar(trabajo, &ORDEN.replace("{archivo}", ARCHIVO_PEDIDO), None)
    }

    fn corregir(&mut self, trabajo: &Path, mensaje: &str) -> Result<String, Fallo> {
        self.comun.preparar_correccion(trabajo, mensaje)?;
        self.llamar(trabajo, ORDEN_CORRECCION, self.sesion.clone())
    }

    fn uso(&self) -> &[Value] {
        &self.comun.uso
    }
}

/// `codex exec` in a read-only sandbox: the order on its standard input,
/// the sheets with `-i`. It keeps no session: a correction carries its
/// last answer in a file.
pub struct Codex {
    comun: Comun,
}

impl Codex {
    /// Codex as `ia.codex` sets it.
    pub fn new(entorno: &Entorno) -> Self {
        Self { comun: Comun::new("codex", entorno) }
    }

    fn llamar(&mut self, trabajo: &Path, orden: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        let exe = ejecutable("codex", &[]).map_err(Fallo::Otro)?;
        let destino = trabajo.join("ia").join("codex_ultima.md");
        let _ = std::fs::remove_file(&destino);
        let mut argumentos = cadenas(&["exec", "-", "--skip-git-repo-check", "-s", "read-only", "-C"]);
        argumentos.push(trabajo.display().to_string());
        argumentos.extend(["-o".to_owned(), destino.display().to_string()]);
        self.comun.con_modelo(&mut argumentos, "-m");
        for hoja in hojas {
            argumentos.extend(["-i".to_owned(), hoja.display().to_string()]);
        }
        let salida = self.comun.correr(&exe, &argumentos, trabajo, &["OPENAI_API_KEY"], Some(orden))?;
        match std::fs::read_to_string(&destino) {
            Ok(texto) if salida.codigo == Some(0) => Ok(texto),
            _ => Err(self.comun.fallo("falló", &salida)),
        }
    }
}

impl Conector for Codex {
    fn nombre(&self) -> &'static str {
        self.comun.nombre
    }

    fn primera(&mut self, trabajo: &Path, _: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        self.llamar(trabajo, &ORDEN.replace("{archivo}", ARCHIVO_PEDIDO), hojas)
    }

    fn corregir(&mut self, trabajo: &Path, mensaje: &str) -> Result<String, Fallo> {
        self.comun.preparar_correccion(trabajo, mensaje)?;
        self.llamar(trabajo, ORDEN_CORRECCION, &[])
    }

    fn uso(&self) -> &[Value] {
        &self.comun.uso
    }
}

/// `gemini -p` in plan mode (read-only); files go along as `@ruta`.
pub struct GeminiCli {
    comun: Comun,
}

impl GeminiCli {
    /// Gemini CLI as `ia.gemini_cli` sets it.
    pub fn new(entorno: &Entorno) -> Self {
        Self { comun: Comun::new("gemini_cli", entorno) }
    }

    fn llamar(&mut self, trabajo: &Path, orden: &str, adjuntos: &[String]) -> Result<String, Fallo> {
        let exe = ejecutable("gemini", &[]).map_err(Fallo::Otro)?;
        let con_adjuntos: Vec<String> = adjuntos.iter().map(|a| format!("@{a}")).collect();
        let mut argumentos = vec!["-p".to_owned(), format!("{orden} {}", con_adjuntos.join(" "))];
        argumentos.extend(cadenas(&["-o", "json", "--approval-mode", "plan"]));
        self.comun.con_modelo(&mut argumentos, "-m");
        let salida = self.comun.correr(&exe, &argumentos, trabajo, &["GEMINI_API_KEY", "GOOGLE_API_KEY"], None)?;
        let desde = salida.stdout.find('{').unwrap_or(salida.stdout.len());
        let respuesta: Value = serde_json::from_str(&salida.stdout[desde..])
            .map_err(|_| self.comun.fallo("respuesta no JSON", &salida))?;
        if crate::python::lleno(&respuesta["error"]) {
            return Err(Fallo::Otro(format!("gemini_cli: {}", crate::python::texto(&respuesta["error"]))));
        }
        Ok(respuesta["response"].as_str().unwrap_or_default().to_owned())
    }
}

impl Conector for GeminiCli {
    fn nombre(&self) -> &'static str {
        self.comun.nombre
    }

    fn primera(&mut self, trabajo: &Path, _: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        let relativas = hojas.iter().map(|h| {
            let relativa = h.strip_prefix(trabajo).unwrap_or(h);
            relativa.to_string_lossy().replace('\\', "/")
        });
        let adjuntos: Vec<String> = std::iter::once(ARCHIVO_PEDIDO.to_owned()).chain(relativas).collect();
        self.llamar(trabajo, &ORDEN.replace("{archivo}", ARCHIVO_PEDIDO), &adjuntos)
    }

    fn corregir(&mut self, trabajo: &Path, mensaje: &str) -> Result<String, Fallo> {
        self.comun.preparar_correccion(trabajo, mensaje)?;
        let adjuntos = cadenas(&["ia/correccion.md", "ia/respuesta_previa.md"]);
        self.llamar(trabajo, ORDEN_CORRECCION, &adjuntos)
    }

    fn uso(&self) -> &[Value] {
        &self.comun.uso
    }
}
