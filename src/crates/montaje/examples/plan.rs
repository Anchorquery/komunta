//! The planner's prompt and the plan's checks, without asking any AI: to
//! hold them to the Python engine's.
//!
//! ```text
//! cargo run -p montaje --example plan -- pedido <job> <template> <rules file>
//! cargo run -p montaje --example plan -- validar <job> <plan.json | @list>...
//! ```
//!
//! `pedido` writes the prompt to standard output, with the job's sheets as
//! they are in `hojas/`. `validar` writes, for each plan, a line of JSON:
//! the list of what is wrong with it.

use std::path::{Path, PathBuf};

use montaje::plan::{pedido, validar};
use serde_json::Value;

const USO: &str = "plan pedido <job> <template> <rules> | plan validar <job> <plan.json>...";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    let (orden, trabajo) = match argumentos.as_slice() {
        [orden, trabajo, ..] => (orden.as_str(), Path::new(trabajo)),
        _ => return Err(USO.into()),
    };
    let inventario = leer(&trabajo.join("inventario.json"))?;
    let analisis = leer(&trabajo.join("analisis.json"))?;
    match (orden, &argumentos[2..]) {
        ("pedido", [plantilla, reglas]) => {
            let reglas = std::fs::read_to_string(reglas)?;
            let texto = pedido::construir(trabajo, &inventario, &analisis, &hojas(trabajo)?, plantilla, &reglas);
            std::io::Write::write_all(&mut std::io::stdout(), texto.as_bytes())?;
        }
        ("validar", planes) => {
            for plan in planes.iter().flat_map(|p| rutas(p)) {
                let errores = validar::validar(&leer(Path::new(&plan))?, &analisis, &inventario);
                println!("{}", serde_json::to_string(&errores)?);
            }
        }
        _ => return Err(USO.into()),
    }
    Ok(())
}

/// `ruta`, or the paths listed in file `lista` when given as `@lista`.
fn rutas(argumento: &str) -> Vec<String> {
    match argumento.strip_prefix('@') {
        Some(lista) => std::fs::read_to_string(lista)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect(),
        None => vec![argumento.to_owned()],
    }
}

fn leer(ruta: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(ruta)?)?)
}

fn hojas(trabajo: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut hojas: Vec<PathBuf> = std::fs::read_dir(trabajo.join("hojas"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .collect();
    hojas.sort();
    Ok(hojas)
}
