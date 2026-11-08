//! What every planner shares, whoever it is: it is asked once, its answer
//! is read and checked, and a broken plan goes back to it once with the
//! list of what is wrong, in the same conversation. The engine's
//! `motor/cerebros/__init__.py`.

use std::fmt;
use std::path::{Path, PathBuf};

use analisis::descripcion::google::ErrorGemini;
use analisis::descripcion::respuesta::leer_json;
use chrono::{DateTime, FixedOffset};
use serde_json::Value;

use super::validar::validar;

/// How many times a broken plan goes back to be corrected.
const MAX_CORRECCIONES: usize = 1;
const CORRECCION: &str =
    "El plan tiene estos errores; devuelve solo el JSON completo corregido, sin texto alrededor:\n- ";

/// Why a planner gave no plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Fallo {
    /// Gemini's free quota is spent; it renews then.
    Cuota(DateTime<FixedOffset>),
    /// Anything else, in a sentence.
    Otro(String),
}

impl fmt::Display for Fallo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fallo::Cuota(renovacion) => ErrorGemini::CuotaAgotada(*renovacion).fmt(f),
            Fallo::Otro(texto) => f.write_str(texto),
        }
    }
}

impl std::error::Error for Fallo {}

impl From<std::io::Error> for Fallo {
    fn from(error: std::io::Error) -> Self {
        Fallo::Otro(error.to_string())
    }
}

/// One planner: an API or a command-line AI, keeping its own conversation.
pub trait Conector: Send {
    /// Its `ia.modo` name: "anthropic", "claude_code"...
    fn nombre(&self) -> &'static str;
    /// Asks for the plan: the prompt `pedido` and the contact sheets
    /// `hojas` of the job in `trabajo`. Returns what it answered.
    fn primera(&mut self, trabajo: &Path, pedido: &str, hojas: &[PathBuf]) -> Result<String, Fallo>;
    /// Goes on with the same conversation: `mensaje` after its last
    /// answer.
    fn corregir(&mut self, trabajo: &Path, mensaje: &str) -> Result<String, Fallo>;
    /// What each call cost or said, for `ia/uso_<nombre>.json`.
    fn uso(&self) -> &[Value];
}

/// What a plan is checked against: the job's JSON files.
#[derive(Clone, Copy, Debug)]
pub struct Material<'a> {
    /// `analisis.json`.
    pub analisis: &'a Value,
    /// `inventario.json`.
    pub inventario: &'a Value,
}

/// Asks `conector` for the plan and checks it, with one round of
/// corrections; writes it as the job's `ia/plan.json`.
pub fn planificar(
    conector: &mut dyn Conector,
    trabajo: &Path,
    pedido: &str,
    hojas: &[PathBuf],
    material: Material,
) -> Result<Value, Fallo> {
    let texto = conector.primera(trabajo, pedido, hojas)?;
    let (plan, errores) = con_correccion(conector, trabajo, texto, "", material)?;
    match plan {
        Some(plan) if errores.is_empty() => {
            escribir_plan(trabajo, &plan)?;
            Ok(plan)
        }
        _ => Err(Fallo::Otro(format!(
            "{}: no devolvió un plan válido:\n- {}",
            conector.nombre(),
            errores.join("\n- ")
        ))),
    }
}

/// Reads and checks the plan in `texto`; when it is broken, asks once
/// more. Every answer is kept as `ia/respuesta_<nombre>_<etiqueta><n>.md`.
fn con_correccion(
    conector: &mut dyn Conector,
    trabajo: &Path,
    mut texto: String,
    etiqueta: &str,
    material: Material,
) -> Result<(Option<Value>, Vec<String>), Fallo> {
    let ia = trabajo.join("ia");
    let mut intento = 0;
    let resultado = loop {
        let respuesta = ia.join(format!("respuesta_{}_{etiqueta}{intento}.md", conector.nombre()));
        std::fs::write(respuesta, &texto)?;
        let (plan, errores) = leer(&texto, material);
        if errores.is_empty() || intento == MAX_CORRECCIONES {
            break (plan, errores);
        }
        intento += 1;
        texto = conector.corregir(trabajo, &format!("{CORRECCION}{}", errores.join("\n- ")))?;
    };
    let uso = serde_json::to_string_pretty(conector.uso()).unwrap_or_default();
    std::fs::write(ia.join(format!("uso_{}.json", conector.nombre())), uso)?;
    Ok(resultado)
}

/// The plan in an answer and what is wrong with it.
fn leer(texto: &str, material: Material) -> (Option<Value>, Vec<String>) {
    match leer_json(texto) {
        Ok(plan) => {
            let errores = validar(&plan, material.analisis, material.inventario);
            (Some(plan), errores)
        }
        Err(error) => (None, vec![format!("La respuesta no es JSON válido: {error}")]),
    }
}

/// Writes `plan` as the job's `ia/plan.json`, indented as the engine does.
pub fn escribir_plan(trabajo: &Path, plan: &Value) -> std::io::Result<()> {
    let mut texto = Vec::new();
    let formato = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut escritor = serde_json::Serializer::with_formatter(&mut texto, formato);
    serde::Serialize::serialize(plan, &mut escritor).map_err(std::io::Error::other)?;
    std::fs::write(trabajo.join("ia").join("plan.json"), texto)
}

#[cfg(test)]
pub(crate) mod pruebas {
    //! A planner that answers what it is given, for the tests.

    use super::*;
    use std::collections::VecDeque;

    pub struct Guionizado {
        pub nombre: &'static str,
        pub respuestas: VecDeque<Result<String, Fallo>>,
        pub mensajes: Vec<String>,
    }

    impl Guionizado {
        pub fn new(nombre: &'static str, respuestas: Vec<Result<&str, Fallo>>) -> Self {
            let respuestas = respuestas.into_iter().map(|r| r.map(str::to_owned)).collect();
            Self { nombre, respuestas, mensajes: Vec::new() }
        }

        fn siguiente(&mut self) -> Result<String, Fallo> {
            self.respuestas.pop_front().unwrap_or(Err(Fallo::Otro("sin respuesta".into())))
        }
    }

    impl Conector for Guionizado {
        fn nombre(&self) -> &'static str {
            self.nombre
        }

        fn primera(&mut self, _: &Path, pedido: &str, _: &[PathBuf]) -> Result<String, Fallo> {
            self.mensajes.push(pedido.to_owned());
            self.siguiente()
        }

        fn corregir(&mut self, _: &Path, mensaje: &str) -> Result<String, Fallo> {
            self.mensajes.push(mensaje.to_owned());
            self.siguiente()
        }

        fn uso(&self) -> &[Value] {
            &[]
        }
    }

    pub fn trabajo(nombre: &str) -> PathBuf {
        let trabajo = std::env::temp_dir().join(format!("komunta-plan-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&trabajo);
        std::fs::create_dir_all(trabajo.join("ia")).unwrap();
        trabajo
    }

    pub fn material() -> (Value, Value) {
        let analisis = serde_json::json!({"clips": [{"id": "A", "duracion": 5.0,
            "transcripcion": {"palabras": [{}, {}, {}]}}]});
        (analisis, serde_json::json!({"imagenes": []}))
    }

    pub const BUENO: &str = "```json\n{\"tomas\": [{\"clip\": \"A\", \"desde\": 0, \"hasta\": 2}]} // fin\n```";
}

#[cfg(test)]
mod tests {
    use super::pruebas::*;
    use super::*;

    #[test]
    fn a_broken_plan_goes_back_once_with_what_is_wrong() {
        let trabajo = trabajo("correccion");
        let (analisis, inventario) = material();
        let material = Material { analisis: &analisis, inventario: &inventario };
        let mut ia = Guionizado::new("prueba", vec![Ok("{\"tomas\": [{\"clip\": \"B\", \"desde\": 0, \"hasta\": 1}]}"), Ok(BUENO)]);
        let plan = planificar(&mut ia, &trabajo, "pedido", &[], material).unwrap();
        assert_eq!(plan["tomas"][0]["clip"], "A");
        assert_eq!(ia.mensajes[1], format!("{CORRECCION}tomas[0]: el clip «B» no existe."));
        assert!(trabajo.join("ia/respuesta_prueba_0.md").exists());
        assert!(trabajo.join("ia/respuesta_prueba_1.md").exists());
        let escrito = std::fs::read_to_string(trabajo.join("ia/plan.json")).unwrap();
        assert!(escrito.starts_with("{\n \"tomas\": [\n  {"), "{escrito}");
        let _ = std::fs::remove_dir_all(&trabajo);
    }

    #[test]
    fn a_plan_still_broken_after_the_correction_is_a_failure() {
        let trabajo = trabajo("roto");
        let (analisis, inventario) = material();
        let material = Material { analisis: &analisis, inventario: &inventario };
        let mut ia = Guionizado::new("prueba", vec![Ok("no sé"), Ok("{\"tomas\": []}")]);
        let fallo = planificar(&mut ia, &trabajo, "pedido", &[], material).unwrap_err();
        assert_eq!(fallo, Fallo::Otro("prueba: no devolvió un plan válido:\n- El plan no tiene tomas.".into()));
        assert!(ia.mensajes[1].contains("La respuesta no es JSON válido: no hay ningún objeto JSON"));
        assert!(!trabajo.join("ia/plan.json").exists());
        let _ = std::fs::remove_dir_all(&trabajo);
    }
}
