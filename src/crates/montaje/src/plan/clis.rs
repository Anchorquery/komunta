//! Planners through a command-line AI, on the subscription already signed
//! in on this computer - no API key. Each runs inside the job's folder,
//! read-only: it reads `ia/pedido.md` and the sheets in `hojas/` (Codex
//! gets the prompt on its standard input and the sheets attached) and
//! answers with the plan. What is long goes in files; the order on the
//! command line is one line, as Windows' `.cmd` wrappers need. The
//! engine's `motor/cerebros/clis.py`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// The same for Codex, which gets it all on its standard input: its
/// sandbox on Windows will not let it run the command that reads a file
/// of the job (November 2026).
const ORDEN_CODEX: &str = "Eres el cerebro de un motor de montaje de vídeo. Debajo van las instrucciones y los datos; \
     las hojas de contacto de los clips van adjuntas como imágenes. No hace falta leer ningún archivo. Responde \
     ÚNICAMENTE con el JSON del plan de montaje, sin texto antes ni después.";
const ORDEN_CODEX_CORRECCION: &str = "Tu plan anterior tenía errores. Debajo van las instrucciones y los datos de \
     antes, tu plan y los errores. No hace falta leer ningún archivo. Responde ÚNICAMENTE con el JSON completo \
     corregido.";

/// What every command-line planner keeps.
struct Comun {
    nombre: &'static str,
    modelo: Option<String>,
    /// How hard it thinks (`ia.<nombre>.esfuerzo`), when the team says.
    esfuerzo: Option<String>,
    /// The program, when the team names it (`ia.<nombre>.ruta`).
    ruta: Option<String>,
    limite: Duration,
    parada: Arc<AtomicBool>,
    uso: Vec<Value>,
}

impl Comun {
    fn new(nombre: &'static str, entorno: &Entorno) -> Self {
        let opciones = entorno.opciones(nombre);
        Self {
            nombre,
            modelo: texto(opciones, "modelo"),
            esfuerzo: texto(opciones, "esfuerzo"),
            ruta: texto(opciones, "ruta"),
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

/// Where Codex writes its last message: a file of its own in the system's
/// temporary folder. Codex 0.171 will not write it inside the folder it
/// works in under its read-only sandbox - it says the path "contains a
/// reparse point", with none there (November 2026).
fn destino_temporal() -> PathBuf {
    static CUENTA: AtomicU64 = AtomicU64::new(0);
    let n = CUENTA.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("komunta-codex-{}-{n}.md", std::process::id()))
}

/// The text option `clave` of `opciones`, when it says something.
fn texto(opciones: &Value, clave: &str) -> Option<String> {
    opciones[clave].as_str().filter(|t| !t.trim().is_empty()).map(str::to_owned)
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

/// `codex exec` in a read-only sandbox: the order and the prompt on its
/// standard input, the sheets with `-i`. It keeps no session: a correction
/// carries the prompt again, its last answer and what is wrong with it.
pub struct Codex {
    comun: Comun,
    /// The prompt of the first call, for a correction.
    pedido: String,
    /// What it last answered.
    previa: Option<String>,
}

impl Codex {
    /// Codex as `ia.codex` sets it.
    pub fn new(entorno: &Entorno) -> Self {
        Self { comun: Comun::new("codex", entorno), pedido: String::new(), previa: None }
    }

    fn llamar(&mut self, trabajo: &Path, orden: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        let exe = ejecutable_codex(self.comun.ruta.as_deref()).map_err(Fallo::Otro)?;
        let destino = destino_temporal();
        let como = ComoCodex {
            modelo: self.comun.modelo.as_deref(),
            esfuerzo: self.comun.esfuerzo.as_deref(),
        };
        let argumentos = como.argumentos(trabajo, &destino, hojas);
        let salida = self.comun.correr(&exe, &argumentos, trabajo, &["OPENAI_API_KEY"], Some(orden));
        let leido = std::fs::read_to_string(&destino);
        let _ = std::fs::remove_file(&destino);
        let salida = salida?;
        match leido {
            Ok(texto) if salida.codigo == Some(0) => {
                // The job keeps the last answer, for whoever looks into it.
                let _ = std::fs::write(trabajo.join("ia").join("codex_ultima.md"), &texto);
                Ok(texto)
            }
            _ => Err(self.comun.fallo("falló", &salida)),
        }
    }
}

impl Conector for Codex {
    fn nombre(&self) -> &'static str {
        self.comun.nombre
    }

    fn primera(&mut self, trabajo: &Path, pedido: &str, hojas: &[PathBuf]) -> Result<String, Fallo> {
        self.pedido = pedido.to_owned();
        let respuesta = self.llamar(trabajo, &orden_codex(pedido), hojas)?;
        self.previa = Some(respuesta.clone());
        Ok(respuesta)
    }

    fn corregir(&mut self, trabajo: &Path, mensaje: &str) -> Result<String, Fallo> {
        let orden = correccion_codex(&self.pedido, self.previa.as_deref().unwrap_or_default(), mensaje);
        let respuesta = self.llamar(trabajo, &orden, &[])?;
        self.previa = Some(respuesta.clone());
        Ok(respuesta)
    }

    fn uso(&self) -> &[Value] {
        &self.comun.uso
    }
}

/// What Codex is sent for the plan: the order, then `pedido`.
fn orden_codex(pedido: &str) -> String {
    format!("{ORDEN_CODEX}\n\n{pedido}")
}

/// What Codex is sent to mend its plan `previa`, wrong as `errores` says.
fn correccion_codex(pedido: &str, previa: &str, errores: &str) -> String {
    format!(
        "{ORDEN_CODEX_CORRECCION}\n\n# Instrucciones y datos\n\n{pedido}\n\n# Tu plan anterior\n\n{previa}\n\n# Errores\n\n{errores}"
    )
}

/// How Codex is run: its model and effort, each the one it is set to when
/// absent.
struct ComoCodex<'a> {
    modelo: Option<&'a str>,
    esfuerzo: Option<&'a str>,
}

impl ComoCodex<'_> {
    /// `codex exec` reading its order from standard input, read-only in
    /// `carpeta`, its last message written to `destino`, the pictures
    /// `imagenes` attached.
    fn argumentos(&self, carpeta: &Path, destino: &Path, imagenes: &[PathBuf]) -> Vec<String> {
        let mut argumentos = cadenas(&["exec", "-", "--skip-git-repo-check", "-s", "read-only", "-C"]);
        argumentos.push(carpeta.display().to_string());
        argumentos.extend(["-o".to_owned(), destino.display().to_string()]);
        if let Some(modelo) = self.modelo {
            argumentos.extend(["-m".to_owned(), modelo.to_owned()]);
        }
        if let Some(esfuerzo) = self.esfuerzo {
            argumentos.extend(["-c".to_owned(), format!("model_reasoning_effort=\"{esfuerzo}\"")]);
        }
        for imagen in imagenes {
            argumentos.extend(["-i".to_owned(), imagen.display().to_string()]);
        }
        argumentos
    }
}

/// The Codex to run: `ruta` when the team names one, else the newest
/// Komunta's chat brought along (codex-acp's own, in npm's `npx` cache),
/// else the one on the PATH. npm's global `codex` can lag far behind: its
/// 0.122 could not read the config a ChatGPT sign-in leaves today
/// (November 2026), and the chat's 0.171 could.
pub fn ejecutable_codex(ruta: Option<&str>) -> Result<PathBuf, String> {
    if let Some(ruta) = ruta {
        let ruta = PathBuf::from(ruta);
        return if ruta.is_file() {
            Ok(ruta)
        } else {
            Err(format!("No encuentro Codex en {}", ruta.display()))
        };
    }
    match del_chat() {
        Some(codex) => Ok(codex),
        None => ejecutable("codex", &[]),
    }
}

/// The newest `codex` in npm's `npx` cache: what `npx
/// @agentclientprotocol/codex-acp` fetched, under
/// `node_modules/@openai/codex-<platform>/vendor/<target>/bin`.
fn del_chat() -> Option<PathBuf> {
    let cache = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("npm-cache")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".npm")
    };
    let programa = if cfg!(windows) { "codex.exe" } else { "codex" };
    let mut hallados: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for npx in dentro(&cache.join("_npx")) {
        let paquetes = dentro(&npx.join("node_modules").join("@openai"))
            .into_iter()
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("codex-")));
        for paquete in paquetes {
            for destino in dentro(&paquete.join("vendor")) {
                let exe = destino.join("bin").join(programa);
                if let Ok(cuando) = exe.metadata().and_then(|m| m.modified()) {
                    hallados.push((cuando, exe));
                }
            }
        }
    }
    hallados.into_iter().max_by_key(|(cuando, _)| *cuando).map(|(_, exe)| exe)
}

/// What is in `carpeta`; nothing when it cannot be read.
fn dentro(carpeta: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(carpeta)
        .map(|entradas| entradas.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Where a describing CLI finds what it is asked, when its order has to
/// stay one line (Gemini CLI, Claude Code): relative to the job's folder.
const ARCHIVO_MIRAR: &str = "ia/mirar.md";

/// A command-line AI asked once about some pictures - the description of
/// the material - in the job's folder.
trait Pregunta: Send {
    /// Its answer to `instruccion` over `imagenes`, run in `trabajo`.
    fn preguntar(&mut self, trabajo: &Path, instruccion: &str, imagenes: &[PathBuf]) -> Result<String, Fallo>;
}

/// `imagenes` relative to `trabajo`, with forward slashes, as a CLI is
/// told of a file.
fn relativas(trabajo: &Path, imagenes: &[PathBuf]) -> Vec<String> {
    imagenes
        .iter()
        .map(|imagen| imagen.strip_prefix(trabajo).unwrap_or(imagen).to_string_lossy().replace('\\', "/"))
        .collect()
}

impl Pregunta for Codex {
    fn preguntar(&mut self, trabajo: &Path, instruccion: &str, imagenes: &[PathBuf]) -> Result<String, Fallo> {
        self.llamar(trabajo, instruccion, imagenes)
    }
}

impl Pregunta for GeminiCli {
    fn preguntar(&mut self, trabajo: &Path, instruccion: &str, imagenes: &[PathBuf]) -> Result<String, Fallo> {
        std::fs::write(trabajo.join(ARCHIVO_MIRAR), instruccion)?;
        let adjuntos: Vec<String> = std::iter::once(ARCHIVO_MIRAR.to_owned()).chain(relativas(trabajo, imagenes)).collect();
        let orden = format!("Lee {ARCHIVO_MIRAR} (instrucciones) y mira las imágenes adjuntas. Responde ÚNICAMENTE con lo que pide, sin texto antes ni después.");
        self.llamar(trabajo, &orden, &adjuntos)
    }
}

impl Pregunta for ClaudeCode {
    fn preguntar(&mut self, trabajo: &Path, instruccion: &str, imagenes: &[PathBuf]) -> Result<String, Fallo> {
        std::fs::write(trabajo.join(ARCHIVO_MIRAR), instruccion)?;
        let orden = format!(
            "Lee {ARCHIVO_MIRAR} (instrucciones) y mira con Read TODAS estas imágenes: {}. Responde ÚNICAMENTE con lo que pide, sin texto antes ni después.",
            relativas(trabajo, imagenes).join(", ")
        );
        self.llamar(trabajo, &orden, None)
    }
}

/// A command-line AI describing the material from sheets of frames, on
/// the subscription signed in on this computer: no API key. Codex takes
/// the sheets attached, Gemini CLI as `@` files, Claude Code reads them.
pub struct MiradorCli {
    nombre: &'static str,
    trabajo: PathBuf,
    cli: std::sync::Mutex<Box<dyn Pregunta>>,
}

impl MiradorCli {
    /// The modes that can describe.
    pub const MODOS: [&'static str; 3] = ["codex", "gemini_cli", "claude_code"];

    /// Sheets one request takes: 80 clips' strips.
    const HOJAS: usize = 20;

    /// The command-line AI `modo` ([`MiradorCli::MODOS`]) as `entorno` sets
    /// it, describing the job in `trabajo`; None for a mode that is not one.
    pub fn new(modo: &str, trabajo: &Path, entorno: &Entorno) -> Option<Self> {
        let (nombre, cli): (&'static str, Box<dyn Pregunta>) = match modo {
            "codex" => ("codex", Box::new(Codex::new(entorno))),
            "gemini_cli" => ("gemini_cli", Box::new(GeminiCli::new(entorno))),
            "claude_code" => ("claude_code", Box::new(ClaudeCode::new(entorno))),
            _ => return None,
        };
        Some(Self {
            nombre,
            trabajo: trabajo.to_owned(),
            cli: std::sync::Mutex::new(cli),
        })
    }
}

impl analisis::descripcion::Mirador for MiradorCli {
    fn nombre(&self) -> &'static str {
        self.nombre
    }

    fn hojas_por_peticion(&self) -> usize {
        Self::HOJAS
    }

    fn mirar(&self, hojas: &[PathBuf], instruccion: &str) -> Result<String, String> {
        let mut cli = self.cli.lock().map_err(|_| format!("{} quedó a medias", self.nombre))?;
        cli.preguntar(&self.trabajo, instruccion, hojas).map_err(|fallo| fallo.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_is_told_its_model_and_effort_only_when_set() {
        let carpeta = Path::new("trabajo");
        let destino = Path::new("trabajo/ia/codex.md");
        let hojas = [PathBuf::from("h1.jpg")];
        let ninguno = ComoCodex { modelo: None, esfuerzo: None }.argumentos(carpeta, destino, &hojas);
        assert!(!ninguno.iter().any(|a| a == "-m" || a == "-c"));
        assert_eq!(ninguno[ninguno.len() - 2..], ["-i", "h1.jpg"]);
        let luna = ComoCodex { modelo: Some("gpt-6-luna"), esfuerzo: Some("low") }.argumentos(carpeta, destino, &[]);
        let junto = luna.join(" ");
        assert!(junto.contains("-m gpt-6-luna"));
        assert!(junto.contains("-c model_reasoning_effort=\"low\""));
        assert!(junto.contains("-s read-only"), "it only reads");
    }

    #[test]
    fn codex_gets_the_prompt_itself_not_a_file_to_read() {
        let orden = orden_codex("DATOS DEL PEDIDO");
        assert!(orden.ends_with("DATOS DEL PEDIDO"));
        assert!(!orden.contains("ia/pedido.md"));
        let correccion = correccion_codex("DATOS DEL PEDIDO", "{\"plan\":1}", "- falta la toma");
        for parte in ["DATOS DEL PEDIDO", "{\"plan\":1}", "- falta la toma"] {
            assert!(correccion.contains(parte), "{parte} is not in the correction");
        }
        assert!(!correccion.contains("ia/correccion.md"));
    }

    #[test]
    fn a_codex_named_that_is_not_there_is_said() {
        assert!(ejecutable_codex(Some("/no/esta/codex.exe")).is_err());
    }

    #[test]
    fn every_command_line_ai_can_describe_and_nothing_else() {
        let entorno = Entorno::default();
        for modo in MiradorCli::MODOS {
            let mirador = MiradorCli::new(modo, Path::new("trabajo"), &entorno).expect(modo);
            assert_eq!(analisis::descripcion::Mirador::nombre(&mirador), modo);
        }
        assert!(MiradorCli::new("google", Path::new("trabajo"), &entorno).is_none(), "an API has its own way");
    }

    #[test]
    fn a_cli_is_told_of_the_sheets_inside_the_job() {
        let trabajo = Path::new("/t/trabajo");
        let hojas = [trabajo.join("ia").join("tiras").join("hoja_01.jpg")];
        assert_eq!(relativas(trabajo, &hojas), ["ia/tiras/hoja_01.jpg"]);
    }
}
