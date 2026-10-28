//! `montaje <trabajo> <salida> [--formato vertical|16x9|1x1] [--fuentes <carpeta>]`
//!
//! Turns a job folder of the montage engine into a project folder Komunta
//! opens. Until the montage screen exists, this is how a plan reaches the
//! editor.

use std::path::PathBuf;
use std::process::ExitCode;

use montaje::{KitTexto, Opciones, escribir, montar};

fn main() -> ExitCode {
    match ejecutar() {
        Ok(ruta) => {
            println!("{}", ruta.display());
            ExitCode::SUCCESS
        }
        Err(mensaje) => {
            eprintln!("montaje: {mensaje}");
            ExitCode::FAILURE
        }
    }
}

fn ejecutar() -> Result<PathBuf, String> {
    let mut posicionales = Vec::new();
    let mut formato = "vertical".to_owned();
    let mut fuentes = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--formato" => formato = args.next().ok_or("falta el formato")?,
            "--fuentes" => {
                fuentes = Some(PathBuf::from(
                    args.next().ok_or("falta la carpeta de fuentes")?,
                ))
            }
            _ => posicionales.push(PathBuf::from(arg)),
        }
    }
    let [trabajo, salida] = posicionales.as_slice() else {
        return Err(
            "uso: montaje <trabajo> <salida> [--formato vertical|16x9|1x1] [--fuentes <carpeta>]"
                .to_owned(),
        );
    };
    let (ancho, alto) = match formato.as_str() {
        "vertical" => (1080, 1920),
        "16x9" => (1920, 1080),
        "1x1" => (1080, 1080),
        otro => return Err(format!("formato desconocido: {otro}")),
    };
    // The engine keeps its fonts two folders above the jobs.
    let fuentes = fuentes.unwrap_or_else(|| {
        trabajo
            .parent()
            .and_then(|p| p.parent())
            .map(|raiz| raiz.join("marcas").join("_comun").join("fuentes"))
            .unwrap_or_default()
    });
    let opciones = Opciones {
        ancho,
        alto,
        fps: 30,
        texto: KitTexto::neutro(&fuentes),
    };
    let nombre = trabajo.file_name().map_or_else(
        || "Montaje".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let (project, settings) = montar(trabajo, &nombre, &opciones).map_err(|e| e.to_string())?;
    escribir(salida, &settings, &project).map_err(|e| e.to_string())
}
