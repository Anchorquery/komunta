//! Has an AI describe what a job's `analisis.json` has not described yet,
//! as the app does after the analysis; with `--ver <folder>`, only writes
//! what the AI would be shown - the contact video and the strips of
//! frames - into that folder, and calls no one. The keys come from
//! `GEMINI_API_KEY` and `ANTHROPIC_API_KEY`.
//!
//! ```text
//! cargo run -p analisis --features describir --example describir -- <job folder> [--ver <folder>] [--ia google|anthropic] [--cache <folder>]
//! ```

use std::path::PathBuf;

use analisis::Inventario;
use analisis::cache::Cache;
use analisis::descripcion::cuota::Cuota;
use analisis::descripcion::describir::{self, Encargo, Proveedor};
use analisis::descripcion::{contacto, lotes, pendientes, tiras};
use analisis::trabajo::Archivo;

const USO: &str = "describir <job folder> [--ver <folder>] [--ia google|anthropic] [--cache <folder>]";

/// The library's log lines, to stderr with how long since the start.
struct Registro(std::time::Instant);

impl log::Log for Registro {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, registro: &log::Record) {
        if self.enabled(registro.metadata()) && registro.target().starts_with("analisis") {
            eprintln!("[{:6.1} s {}] {}", self.0.elapsed().as_secs_f64(), registro.level(), registro.args());
        }
    }

    fn flush(&self) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registro: &'static Registro = Box::leak(Box::new(Registro(std::time::Instant::now())));
    log::set_logger(registro).map_err(|e| e.to_string())?;
    log::set_max_level(log::LevelFilter::Info);
    let mut argumentos = std::env::args().skip(1);
    let trabajo = PathBuf::from(argumentos.next().ok_or(USO)?);
    let (mut ver, mut ia, mut cache) = (None, "google".to_owned(), None);
    while let Some(bandera) = argumentos.next() {
        let valor = argumentos.next().ok_or(USO)?;
        match bandera.as_str() {
            "--ver" => ver = Some(PathBuf::from(valor)),
            "--ia" => ia = valor,
            "--cache" => cache = Some(PathBuf::from(valor)),
            _ => return Err(USO.into()),
        }
    }
    if let Some(carpeta) = ver {
        return mostrar(&trabajo, &carpeta);
    }
    let cache_carpeta = cache.unwrap_or_else(|| trabajo.join("cache"));
    let cache = Cache::new(cache_carpeta.clone(), serde_json::json!({"ejemplo": 1}));
    let encargo = Encargo {
        trabajo: &trabajo,
        proveedores: proveedores(&ia),
        cache: Some(&cache),
        cuota: Cuota::en(cache_carpeta),
    };
    let inicio = std::time::Instant::now();
    let resumen = describir::describir(&encargo, |aviso| eprintln!("{aviso}"))?;
    eprintln!("{resumen:?} en {:.1} s", inicio.elapsed().as_secs_f64());
    Ok(())
}

/// The providers whose keys are set, in the order `ia` asks for.
fn proveedores(ia: &str) -> Vec<Proveedor> {
    let clave = |variable: &str| std::env::var(variable).ok().filter(|c| !c.is_empty());
    let orden = if ia == "anthropic" { ["anthropic", "google"] } else { ["google", "anthropic"] };
    orden
        .into_iter()
        .filter_map(|id| match id {
            "google" => Some(Proveedor::Google {
                clave: clave("GEMINI_API_KEY").or_else(|| clave("GOOGLE_API_KEY"))?,
                modelos: vec!["gemini-flash-latest".into()],
                limite_diario: None,
            }),
            _ => Some(Proveedor::Anthropic {
                clave: clave("ANTHROPIC_API_KEY")?,
                modelo: "claude-opus-5-5".into(),
            }),
        })
        .collect()
}

/// Writes what the AI would be shown into `carpeta`.
fn mostrar(trabajo: &std::path::Path, carpeta: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let inventario = Inventario::leer(trabajo)?;
    let archivo = Archivo::abrir(trabajo)?;
    let pendientes = pendientes(archivo.datos(), &inventario);
    eprintln!("{} sin describir", pendientes.len());
    for (n, lote) in lotes(pendientes.clone()).iter().enumerate() {
        let video = contacto::video(lote, &carpeta.join(format!("contacto_{}.mp4", n + 1)))?;
        eprintln!("{}: {:?}", video.ruta.display(), video.tabla);
    }
    for hoja in tiras::hojas(&pendientes, &carpeta.join("tiras"))? {
        eprintln!("{}", hoja.display());
    }
    Ok(())
}
