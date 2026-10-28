//! Holds the Rust timeline to the Python engine's on real jobs.
//!
//! The jobs carry clients' footage and words, so they are not in the
//! repository. To run it, dump the engine's timelines with a script that
//! writes, per job, `{"trabajo": <job folder>, "linea": construir_linea(...)}`
//! into a folder, and point `MONTAJE_DORADOS` at it:
//!
//! ```sh
//! MONTAJE_DORADOS=/path/to/dorados cargo test -p montaje --test dorado
//! ```
//!
//! Without the variable the test passes without checking anything.

use std::path::PathBuf;

use montaje::{Trabajo, construir_linea};
use serde_json::Value;

const MARGEN: f64 = 1e-6;

fn cerca(a: f64, b: &Value, que: &str) {
    let b = b
        .as_f64()
        .unwrap_or_else(|| panic!("{que}: no es un número"));
    assert!(
        (a - b).abs() < MARGEN,
        "{que}: Rust {a} frente a Python {b}"
    );
}

#[test]
fn the_timeline_matches_the_python_engine() {
    let Some(carpeta) = std::env::var_os("MONTAJE_DORADOS").map(PathBuf::from) else {
        eprintln!("MONTAJE_DORADOS sin definir: no se compara nada");
        return;
    };
    let mut vistos = 0;
    for entrada in std::fs::read_dir(&carpeta).expect("carpeta de dorados") {
        let ruta = entrada.expect("entrada").path();
        if ruta.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let dorado: Value =
            serde_json::from_str(&std::fs::read_to_string(&ruta).expect("leer")).expect("json");
        let trabajo = Trabajo::leer(dorado["trabajo"].as_str().expect("trabajo").as_ref())
            .expect("leer el trabajo");
        let linea = construir_linea(&trabajo).expect("construir la línea");
        let esperada = &dorado["linea"];
        let nombre = ruta.file_stem().unwrap().to_string_lossy();

        cerca(
            linea.duracion_voz,
            &esperada["duracion_voz"],
            &format!("{nombre} duracion_voz"),
        );
        let trozos = esperada["trozos"].as_array().expect("trozos");
        assert_eq!(linea.trozos.len(), trozos.len(), "{nombre}: trozos");
        for (n, (t, e)) in linea.trozos.iter().zip(trozos).enumerate() {
            assert_eq!(t.clip, e["clip"].as_str().unwrap(), "{nombre} trozo {n}");
            cerca(t.ini, &e["ini"], &format!("{nombre} trozo {n} ini"));
            cerca(t.fin, &e["fin"], &format!("{nombre} trozo {n} fin"));
            cerca(t.t, &e["t"], &format!("{nombre} trozo {n} t"));
        }
        let subs = esperada["subtitulos"].as_array().expect("subtitulos");
        assert_eq!(linea.subtitulos.len(), subs.len(), "{nombre}: subtítulos");
        for (n, (s, e)) in linea.subtitulos.iter().zip(subs).enumerate() {
            assert_eq!(
                s.texto,
                e["texto"].as_str().unwrap(),
                "{nombre} subtítulo {n}"
            );
            cerca(s.ini, &e["ini"], &format!("{nombre} subtítulo {n} ini"));
            cerca(s.fin, &e["fin"], &format!("{nombre} subtítulo {n} fin"));
        }
        let recursos = esperada["recursos"].as_array().expect("recursos");
        assert_eq!(linea.recursos.len(), recursos.len(), "{nombre}: recursos");
        for (n, (r, e)) in linea.recursos.iter().zip(recursos).enumerate() {
            assert_eq!(r.clip, e["clip"].as_str().unwrap(), "{nombre} recurso {n}");
            cerca(r.t, &e["t"], &format!("{nombre} recurso {n} t"));
            cerca(r.dur, &e["dur"], &format!("{nombre} recurso {n} dur"));
            cerca(r.desde, &e["desde"], &format!("{nombre} recurso {n} desde"));
        }
        assert_eq!(
            linea.musica.is_some(),
            !esperada["musica"].is_null(),
            "{nombre}: música"
        );
        vistos += 1;
    }
    assert!(vistos > 0, "no hay dorados en {}", carpeta.display());
}
