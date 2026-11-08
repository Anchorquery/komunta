//! The montage plan, asked of an AI: the port of the engine's `motor/ia.py`,
//! `motor/cerebros/` and `motor/validar.py`.
//!
//! [`preparar`] does what the engine's `preparar` did after the analysis:
//! the contact sheets, the prompt ([`pedido`]) and the plan from the
//! planner `ia.modo` names - an API or a command-line AI on the team's
//! subscription ([`cadena`]) - checked ([`validar`]) and corrected once
//! when it is broken ([`cerebro`]). The plan is kept as the job's first
//! version.

pub mod anthropic;
pub mod cadena;
pub mod cerebro;
pub mod clis;
pub mod google;
pub mod openai;
pub mod pedido;
pub mod proceso;
pub mod validar;

use std::path::{Path, PathBuf};

use analisis::Inventario;
use serde_json::{Map, Value, json};

pub use cadena::{Cadena, Entorno};
pub use cerebro::{Fallo, Material};

use crate::estado;
use crate::avance::Avance;

/// A job ready to be planned.
#[derive(Clone, Debug)]
pub struct Encargo<'a> {
    /// The job's folder: its inventory and analysis are there.
    pub trabajo: &'a Path,
    /// The template's name, as the prompt says it.
    pub plantilla: &'a str,
    /// The recipe's rules, and the team's request.
    pub reglas: &'a str,
    /// The engine's settings and the keys.
    pub entorno: Entorno,
}

/// How the planning ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Resultado {
    /// The plan, now the job's `ia/plan.json`, and who made it.
    Plan {
        /// The plan.
        plan: Value,
        /// The planner that made it.
        cerebro: String,
    },
    /// `ia.modo` is "manual" and no one has written `ia/plan.json` yet:
    /// the prompt waits in `ia/pedido.md`.
    Esperando(PathBuf),
}

/// Draws the job's contact sheets, writes the prompt and asks for the plan,
/// telling `avisar` how it goes.
pub fn preparar(encargo: &Encargo, mut avisar: impl FnMut(Avance)) -> Result<Resultado, Fallo> {
    let trabajo = encargo.trabajo;
    let modo = encargo.entorno.config["ia"]["modo"].as_str().unwrap_or("manual").to_owned();
    let inventario = leer_json(&trabajo.join("inventario.json"))?;
    let analisis = leer_json(&trabajo.join("analisis.json"))?;
    let hojas = hojas(trabajo)?;
    aviso(&mut avisar, 0.0, format!("Pidiendo el plan ({modo})"));
    estado::guardar(trabajo, "planificando", Map::new())?;
    let pedido = pedido::construir(trabajo, &inventario, &analisis, &hojas, encargo.plantilla, encargo.reglas);
    let ia = trabajo.join("ia");
    std::fs::create_dir_all(&ia)?;
    std::fs::write(ia.join("pedido.md"), &pedido)?;
    let material = Material { analisis: &analisis, inventario: &inventario };
    let (plan, cerebro, fallos) = match Cadena::crear(&encargo.entorno)? {
        Some(mut cadena) => {
            let plan = cadena.plan(trabajo, &pedido, &hojas, material)?;
            (plan, cadena.nombre().to_owned(), cadena.fallos)
        }
        None => match leer_json(&ia.join("plan.json")) {
            Ok(plan) => (plan, modo.clone(), Vec::new()),
            Err(_) => return Ok(esperar(trabajo, &mut avisar)?),
        },
    };
    crate::versiones::guardar(trabajo, "ia", Some(&format!("Plan inicial ({cerebro})")), &[])?;
    if cerebro != modo {
        let mut extra = Map::new();
        extra.insert("cerebro_usado".into(), json!(cerebro));
        extra.insert("fallos_cerebro".into(), json!(fallos.iter().take(5).collect::<Vec<_>>()));
        estado::anotar(trabajo, extra)?;
        aviso(&mut avisar, 0.9, format!("{modo} no pudo; el plan lo hizo {cerebro}"));
    }
    aviso(&mut avisar, 1.0, resumen(&plan));
    Ok(Resultado::Plan { plan, cerebro })
}

/// The job's contact sheets, drawn afresh from its analysis.
fn hojas(trabajo: &Path) -> std::io::Result<Vec<PathBuf>> {
    let inventario = Inventario::leer(trabajo)?;
    let celdas = analisis::hojas::celdas(trabajo, &inventario)?;
    analisis::hojas::generar(trabajo, &celdas)
}

fn esperar(trabajo: &Path, avisar: &mut impl FnMut(Avance)) -> std::io::Result<Resultado> {
    estado::guardar(trabajo, "esperando_plan", Map::new())?;
    aviso(avisar, 0.0, "Esperando plan manual (ia/plan.json)".into());
    Ok(Resultado::Esperando(trabajo.join("ia").join("pedido.md")))
}

fn aviso(avisar: &mut impl FnMut(Avance), fraccion: f64, mensaje: String) {
    avisar(Avance { fase: "plan".into(), fraccion, mensaje });
}

/// `Plan: 12 tomas, 20 subtítulos, 9 recursos`.
fn resumen(plan: &Value) -> String {
    let cuantos = |clave: &str| plan[clave].as_array().map_or(0, Vec::len);
    format!(
        "Plan: {} tomas, {} subtítulos, {} recursos",
        cuantos("tomas"),
        cuantos("subtitulos"),
        cuantos("recursos")
    )
}

fn leer_json(ruta: &Path) -> Result<Value, Fallo> {
    let texto = std::fs::read_to_string(ruta).map_err(|e| Fallo::Otro(format!("{}: {e}", ruta.display())))?;
    serde_json::from_str(&texto).map_err(|e| Fallo::Otro(format!("{}: {e}", ruta.display())))
}
