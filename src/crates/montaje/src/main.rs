//! `montaje <trabajo> <salida> [opciones]`
//!
//! Turns a job folder of the montage engine into a project folder Komunta
//! opens. Until the montage screen exists, this is how a plan reaches the
//! editor.
//!
//! Options:
//!   --receta <id>            recipe (default: the job's template, or
//!                            mensaje_a_camara)
//!   --recetas <carpeta>      the team's own recipes (JSON)
//!   --formatos <a,b>         frames, main first: vertical, 16x9, 1x1
//!                            (default: the recipe's)
//!   --marca <id>             brand kit (default: the job's)
//!   --marcas <carpeta>       the engine's `marcas` folder (default: two
//!                            folders above the job)
//!
//! `montaje nuevo <material> <proyecto> --motor <carpeta> [opciones]` does
//! what the "Nuevo montaje" screen does: runs the engine on the material
//! for the plan, then lays it out. Besides the options above it takes
//! `--peticion <texto>`, `--guion <archivo>`, `--idioma es|eu` and
//! `--ia <cerebro>`.

use std::path::PathBuf;
use std::process::ExitCode;

use montaje::{Formato, Kit, Opciones, escribir, montar, receta, sondeo};

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

const USO: &str = "uso: montaje <trabajo> <salida> [--receta id] [--recetas carpeta] \
[--formatos vertical,16x9,1x1] [--marca id] [--marcas carpeta]";

fn ejecutar() -> Result<PathBuf, String> {
    let mut posicionales = Vec::new();
    let mut opcion = std::collections::HashMap::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(nombre) = arg.strip_prefix("--") {
            let valor = args.next().ok_or(format!("falta el valor de --{nombre}"))?;
            opcion.insert(nombre.to_owned(), valor);
        } else {
            posicionales.push(PathBuf::from(arg));
        }
    }
    if posicionales
        .first()
        .is_some_and(|p| p.as_os_str() == "nuevo")
    {
        return nuevo(&posicionales[1..], &opcion);
    }
    let [trabajo, salida] = posicionales.as_slice() else {
        return Err(USO.to_owned());
    };
    // What the engine chose for the job: template and brand kit.
    let estado: serde_json::Value = std::fs::read_to_string(trabajo.join("estado.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let raiz = trabajo
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or_default();

    let recetas = receta::cargar(&opcion.get("recetas").map(PathBuf::from).unwrap_or_default());
    let id_receta = opcion
        .get("receta")
        .cloned()
        .or_else(|| estado["plantilla"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "mensaje_a_camara".to_owned());
    let receta = recetas
        .into_iter()
        .find(|r| r.id == id_receta)
        .ok_or(format!("no hay receta «{id_receta}»"))?;

    let nombres: Vec<String> = match opcion.get("formatos") {
        Some(lista) => lista.split(',').map(|f| f.trim().to_owned()).collect(),
        None => std::iter::once(receta.formato.clone())
            .chain(receta.formatos_extra.iter().cloned())
            .collect(),
    };
    let formatos = nombres
        .iter()
        .map(|n| Formato::de(n).ok_or(format!("formato desconocido: {n}")))
        .collect::<Result<Vec<_>, _>>()?;

    let marcas = opcion
        .get("marcas")
        .map(PathBuf::from)
        .unwrap_or_else(|| raiz.join("marcas"));
    let id_marca = opcion
        .get("marca")
        .cloned()
        .or_else(|| estado["marca"].as_str().map(str::to_owned));
    let kit = match id_marca {
        // The engine's own default flare when the kit leaves it open.
        Some(id) => Kit::cargar(&marcas, &id, 0.45).map_err(|e| e.to_string())?,
        None => Kit::neutro(&marcas.join("_comun").join("fuentes")),
    };

    let opciones = Opciones {
        formatos,
        fps: 30,
        kit,
        carpeta: salida.clone(),
        sondear: sondeo::ffprobe,
    };
    let nombre = estado["nombre"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            trabajo
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Montaje".to_owned());
    let (project, settings) =
        montar(trabajo, &nombre, &receta, &opciones).map_err(|e| e.to_string())?;
    escribir(salida, &settings, &project).map_err(|e| e.to_string())
}

/// `montaje nuevo`: the engine's analysis and plan, then the project.
fn nuevo(
    posicionales: &[PathBuf],
    opcion: &std::collections::HashMap<String, String>,
) -> Result<PathBuf, String> {
    let [material, proyecto] = posicionales else {
        return Err("uso: montaje nuevo <material> <proyecto> --motor <carpeta> [...]".into());
    };
    let motor = montaje::Motor {
        carpeta: opcion
            .get("motor")
            .map(PathBuf::from)
            .ok_or("falta --motor")?,
        python: opcion.get("python").cloned().unwrap_or_default(),
    };
    if !motor.valido() {
        return Err(format!("{} no tiene el motor", motor.carpeta.display()));
    }
    let id_receta = opcion
        .get("receta")
        .map_or("mensaje_a_camara", String::as_str);
    let candidatas = receta::cargar(&opcion.get("recetas").map(PathBuf::from).unwrap_or_default());
    let receta = if id_receta == receta::AUTOMATICA {
        receta::automatica()
    } else {
        candidatas
            .iter()
            .find(|r| r.id == id_receta)
            .cloned()
            .ok_or(format!("no hay receta «{id_receta}»"))?
    };
    let nombre = proyecto
        .file_name()
        .map_or("montaje".into(), |n| n.to_string_lossy().into_owned());
    let pedido = montaje::Pedido {
        material: material.clone(),
        id: montaje::motor::id_trabajo(&nombre, &montaje::motor::ahora()),
        receta: receta.clone(),
        peticion: opcion.get("peticion").cloned().unwrap_or_default(),
        guion: opcion.get("guion").map(PathBuf::from),
        idioma: opcion.get("idioma").cloned(),
        marca: opcion.get("marca").cloned(),
        cerebro: opcion.get("ia").cloned(),
        candidatas,
    };
    let trabajo = montaje::motor::analizar(&motor, &pedido, &Default::default(), |a| {
        eprintln!("[{} {:>3.0}%] {}", a.fase, a.fraccion * 100.0, a.mensaje);
    })
    .map_err(|e| e.to_string())?;
    let receta = pedido.receta_final(&trabajo);
    if pedido.receta.es_automatica() {
        eprintln!("la IA eligió: {}", receta.nombre);
    }
    let formatos = match opcion.get("formatos") {
        Some(lista) => lista.split(',').map(|f| f.trim().to_owned()).collect(),
        None => std::iter::once(receta.formato.clone())
            .chain(receta.formatos_extra.iter().cloned())
            .collect(),
    };
    let mut encargo = montaje::Encargo {
        trabajo,
        motor: motor.carpeta.clone(),
        receta,
        marca: pedido.marca.clone(),
        formatos,
        version: 0,
        material: pedido.material.clone(),
        pedida: pedido.receta.id.clone(),
        peticion: pedido.peticion.clone(),
    };
    encargo.crear(proyecto, &nombre).map_err(|e| e.to_string())
}
