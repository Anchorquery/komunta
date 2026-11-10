//! End to end on real jobs, the way the app goes: the montage wizard lays a
//! job out as a project and links it (`Encargo::crear`); the script pane
//! opens that project again, reads its job and plan back and runs the
//! quality check (`Encargo::revisar`); an edit to the plan runs it again;
//! «Apply» lays the plan out as a new timeline (`Encargo::aplicar`).
//!
//! It reads the same dumps as `tests/dorado.rs` (see there) and holds the
//! warnings to the Python engine's. The projects go to a folder of their
//! own under the system's temporary folder, removed at the end.
//!
//! ```sh
//! MONTAJE_DORADOS=/path/to/dorados cargo test -p montaje --test extremo
//! ```
//!
//! Without the variable the test passes without checking anything.

use std::path::{Path, PathBuf};

use montaje::{Encargo, PlanCrudo, Receta, Trabajo, receta, revision::Aviso};
use serde_json::Value;

const MARGEN: f64 = 1e-6;

#[test]
fn a_montage_project_opens_with_the_python_engines_warnings() {
    let Some(dorados) = std::env::var_os("MONTAJE_DORADOS").map(PathBuf::from) else {
        eprintln!("MONTAJE_DORADOS sin definir: no se prueba nada");
        return;
    };
    let raiz = std::env::temp_dir().join(format!("montaje-extremo-{}", std::process::id()));
    let mut vistos = 0;
    for entrada in std::fs::read_dir(&dorados).expect("carpeta de dorados") {
        let ruta = entrada.expect("entrada").path();
        if ruta.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let dorado: Value =
            serde_json::from_str(&std::fs::read_to_string(&ruta).expect("leer")).expect("json");
        let nombre = ruta.file_stem().unwrap().to_string_lossy().into_owned();
        let trabajo = PathBuf::from(dorado["trabajo"].as_str().expect("trabajo"));
        probar(&trabajo, &raiz.join(&nombre), &raiz.join("datos"), &dorado, &nombre);
        vistos += 1;
    }
    let _ = std::fs::remove_dir_all(&raiz);
    assert!(vistos > 0, "no hay dorados en {}", dorados.display());
}

fn probar(trabajo: &Path, proyecto: &Path, datos: &Path, dorado: &Value, nombre: &str) {
    // The wizard: lay the job out and link it.
    let receta = receta_del_trabajo(trabajo);
    let mut encargo = Encargo {
        trabajo: trabajo.to_path_buf(),
        datos: datos.to_path_buf(),
        formatos: std::iter::once(receta.formato.clone())
            .chain(receta.formatos_extra.iter().cloned())
            .collect(),
        receta,
        ..Encargo::default()
    };
    let concat = encargo
        .crear(proyecto, nombre)
        .unwrap_or_else(|e| panic!("{nombre}: crear el proyecto: {e}"));
    let lineas = lineas_de_tiempo(&concat, nombre);
    assert!(lineas > 0, "{nombre}: el proyecto no tiene líneas de tiempo");

    // The script pane: the project opens again as a montage.
    let leido = Encargo::leer(proyecto).unwrap_or_else(|| panic!("{nombre}: sin montaje.json"));
    assert_eq!(leido, encargo, "{nombre}: el enlace no vuelve igual");
    let mut plan = PlanCrudo::leer(&leido.trabajo).expect("leer el plan");
    let mut actual = Trabajo::leer(&leido.trabajo).expect("leer el trabajo");
    actual.plan = plan.plan().expect("el plan");
    let avisos = leido.revisar(&actual).expect("revisar");
    if let Some(esperados) = dorado["avisos"].as_array() {
        comparar(&avisos, esperados, nombre);
    }

    // An edit in the pane: a word cut out runs the check again.
    if !actual.plan.tomas.is_empty() {
        plan.alternar_palabra(0, actual.plan.tomas[0].desde.unwrap_or(0));
        actual.plan = plan.plan().expect("el plan editado");
        leido.revisar(&actual).expect("revisar tras editar");
    }

    // «Apply»: the saved plan laid out again as a new timeline.
    encargo.aplicar(proyecto).expect("aplicar el plan otra vez");
    assert!(
        lineas_de_tiempo(&concat, nombre) > lineas,
        "{nombre}: aplicar no añade línea de tiempo"
    );
    assert_eq!(Encargo::leer(proyecto).map(|e| e.version), Some(2), "{nombre}: versión");
}

/// The engine's template for the job, as a built-in recipe.
fn receta_del_trabajo(trabajo: &Path) -> Receta {
    let estado: Value = std::fs::read_to_string(trabajo.join("estado.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let id = estado["plantilla"].as_str().unwrap_or("mensaje_a_camara");
    let serie = receta::de_serie();
    serie
        .iter()
        .find(|r| r.id == id)
        .or_else(|| serie.iter().find(|r| r.id == "mensaje_a_camara"))
        .cloned()
        .expect("la receta de serie")
}

/// How many timelines the saved project opens with in the editor.
fn lineas_de_tiempo(concat: &Path, nombre: &str) -> usize {
    let documento: Value =
        serde_json::from_str(&std::fs::read_to_string(concat).expect("leer concat.json"))
            .expect("concat.json es JSON");
    concat_project::from_document(&documento)
        .unwrap_or_else(|| panic!("{nombre}: el editor no abre el proyecto"))
        .timelines
        .len()
}

fn comparar(avisos: &[Aviso], esperados: &[Value], nombre: &str) {
    let textos: Vec<&str> = avisos.iter().map(|a| a.texto.as_str()).collect();
    let esperados_textos: Vec<&str> = esperados
        .iter()
        .map(|e| e["texto"].as_str().unwrap())
        .collect();
    assert_eq!(textos, esperados_textos, "{nombre}: avisos");
    for (n, (a, e)) in avisos.iter().zip(esperados).enumerate() {
        assert_eq!(a.instruccion.as_deref(), e["instruccion"].as_str(), "{nombre} aviso {n}");
        let peso = e["peso"].as_f64().expect("peso");
        assert!((a.peso - peso).abs() < MARGEN, "{nombre} aviso {n}: peso {} frente a {peso}", a.peso);
    }
}
