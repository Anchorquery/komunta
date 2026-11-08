//! Asks for the plan of an analysed job as the app does: the settings of
//! a montage folder (the app's, or the old engine's), the keys from the
//! system's credential store, the folder's `.env` or the environment. `--ia manual` calls no one: it leaves the prompt in
//! `ia/pedido.md`, or takes the `ia/plan.json` already there. `--solo` asks
//! that planner alone, with no fallback.
//!
//! ```text
//! cargo run -p montaje --example planificar -- <montage folder> <job> [--ia <modo>] [--plantilla <id>] [--reglas <file>] [--solo]
//! ```

use std::path::PathBuf;

use montaje::ia::Ia;
use montaje::llavero::{self, LlaveroSistema};
use montaje::Datos;
use montaje::plan::{self, Encargo, Entorno};

const USO: &str = "planificar <montage folder> <job> [--ia <modo>] [--plantilla <id>] [--reglas <file>] [--solo]";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut argumentos = std::env::args().skip(1);
    let datos = Datos { carpeta: PathBuf::from(argumentos.next().ok_or(USO)?) };
    let trabajo = PathBuf::from(argumentos.next().ok_or(USO)?);
    let (mut ia, mut plantilla, mut reglas) = (Ia::default(), "mensaje_a_camara".to_owned(), String::new());
    let mut solo = false;
    while let Some(bandera) = argumentos.next() {
        if bandera == "--solo" {
            solo = true;
            continue;
        }
        let valor = argumentos.next().ok_or(USO)?;
        match bandera.as_str() {
            "--ia" => ia.cerebro = Some(valor),
            "--plantilla" => plantilla = valor,
            "--reglas" => reglas = std::fs::read_to_string(valor)?,
            _ => return Err(USO.into()),
        }
    }
    let mut config = datos.ajustes(&ia);
    if solo {
        config["ia"]["respaldo"] = serde_json::json!([]);
    }
    let encargo = Encargo {
        trabajo: &trabajo,
        plantilla: &plantilla,
        reglas: &reglas,
        entorno: Entorno {
            config,
            claves: llavero::claves(&LlaveroSistema, &datos.carpeta.join(".env")),
            cuota: datos.carpeta_cache(),
            parada: Default::default(),
        },
    };
    let inicio = std::time::Instant::now();
    let resultado = plan::preparar(&encargo, |avance| eprintln!("[{}] {}", avance.fase, avance.mensaje))?;
    match resultado {
        plan::Resultado::Plan { cerebro, .. } => eprintln!("plan de {cerebro}"),
        plan::Resultado::Esperando(pedido) => eprintln!("esperando plan: {}", pedido.display()),
    }
    eprintln!("{:.1} s", inicio.elapsed().as_secs_f64());
    Ok(())
}
