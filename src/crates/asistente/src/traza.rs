// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! A trace of every turn, one JSON line each, for the team to read back
//! what the agent did and why a task went wrong - and the stuff the
//! assistant's golden tasks are made from. A line holds the person's words,
//! each step's words and calls with their arguments and results (pictures
//! left out), how the turn ended and how long it took.
//!
//! The file grows to [`TOPE`] and then starts again, the old one kept
//! beside it as `.1`.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::bucle::Fin;
use crate::conversacion::{Mensaje, Parte, Rol};

/// How big a trace file grows before it is put aside.
pub const TOPE: u64 = 8 * 1024 * 1024;

/// The longest text kept of a tool's result: a timeline read is long, and
/// the trace is for reading what happened, not for replaying it.
const LARGO_RESULTADO: usize = 2000;

/// A turn as one line of the trace: what the person asked, and the turn's
/// own `mensajes` - none when it failed, as the loop takes them back.
pub fn linea(
    pedido: &str,
    mensajes: &[Mensaje],
    resultado: &Result<Fin, String>,
    duracion: Duration,
    agente: &str,
) -> Value {
    let (fin, error) = match resultado {
        Ok(Fin::Respuesta) => ("respuesta", None),
        Ok(Fin::Parada) => ("parada", None),
        Err(motivo) => ("error", Some(motivo.as_str())),
    };
    let pasos: Vec<Value> = mensajes.iter().filter_map(paso).collect();
    let fecha = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    json!({
        "fecha": fecha,
        "agente": agente,
        "pedido": pedido,
        "pasos": pasos,
        "fin": fin,
        "error": error,
        "ms": duracion.as_millis() as u64,
    })
}

/// What a message adds to the trace: the AI's words and calls, the
/// results the tools gave. The person's words are the line's `pedido`.
fn paso(mensaje: &Mensaje) -> Option<Value> {
    let partes: Vec<Value> = mensaje
        .partes
        .iter()
        .filter_map(|parte| match parte {
            Parte::Texto(texto) if mensaje.rol == Rol::Modelo && !texto.trim().is_empty() => {
                Some(json!({"dice": texto}))
            }
            Parte::Texto(_) => None,
            Parte::Llamada { nombre, argumentos, .. } => Some(json!({"llama": nombre, "con": argumentos})),
            Parte::Resultado {
                nombre,
                valor,
                error,
                imagen,
                ..
            } => Some(json!({
                "resultado": nombre,
                "ok": !error,
                "valor": recortado(valor),
                "imagen": imagen.as_ref().map(|i| i.nota.as_str()),
            })),
        })
        .collect();
    (!partes.is_empty()).then(|| json!(partes))
}

/// `valor`, as text past [`LARGO_RESULTADO`] characters.
fn recortado(valor: &Value) -> Value {
    let texto = valor.to_string();
    if texto.chars().count() <= LARGO_RESULTADO {
        return valor.clone();
    }
    let corto: String = texto.chars().take(LARGO_RESULTADO).collect();
    json!(format!("{corto}…"))
}

/// Adds `linea` to the trace in `ruta`, putting the file aside first once
/// it has reached [`TOPE`].
pub fn anotar(ruta: &Path, linea: &Value) -> std::io::Result<()> {
    if let Some(carpeta) = ruta.parent() {
        std::fs::create_dir_all(carpeta)?;
    }
    if std::fs::metadata(ruta).is_ok_and(|m| m.len() >= TOPE) {
        std::fs::rename(ruta, ruta.with_extension("1.jsonl"))?;
    }
    let mut archivo = std::fs::OpenOptions::new().create(true).append(true).open(ruta)?;
    writeln!(archivo, "{linea}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herramienta::Imagen;

    fn turno() -> Vec<Mensaje> {
        vec![
            Mensaje::del_usuario("quita la muletilla"),
            Mensaje {
                rol: Rol::Modelo,
                partes: vec![
                    Parte::Texto("Miro el guion.".into()),
                    Parte::Llamada {
                        id: "1".into(),
                        nombre: "guion_leer".into(),
                        argumentos: json!({}),
                    },
                ],
                crudo: None,
            },
            Mensaje {
                rol: Rol::Usuario,
                partes: vec![Parte::Resultado {
                    id: "1".into(),
                    nombre: "guion_leer".into(),
                    valor: json!({"ok": true, "datos": "x".repeat(3000)}),
                    error: false,
                    imagen: Some(Imagen {
                        mime: "image/jpeg".into(),
                        datos: vec![1, 2, 3],
                        nota: "fotograma en 2 s".into(),
                    }),
                }],
                crudo: None,
            },
        ]
    }

    #[test]
    fn a_turn_is_one_line_with_its_words_calls_and_results_but_no_pictures() {
        let linea = linea("quita la muletilla", &turno(), &Ok(Fin::Respuesta), Duration::from_millis(1500), "gemini");
        assert_eq!(linea["pedido"], "quita la muletilla");
        assert_eq!(linea["fin"], "respuesta");
        assert_eq!(linea["ms"], 1500);
        let pasos = linea["pasos"].as_array().unwrap();
        assert_eq!(pasos.len(), 2, "the person's words are not a step");
        assert_eq!(pasos[0][0]["dice"], "Miro el guion.");
        assert_eq!(pasos[0][1]["llama"], "guion_leer");
        let resultado = &pasos[1][0];
        assert_eq!(resultado["imagen"], "fotograma en 2 s");
        assert!(resultado["valor"].as_str().unwrap().ends_with('…'), "a long result is cut");
        assert!(!linea.to_string().contains("[1,2,3]"));
    }

    #[test]
    fn a_failed_turn_says_why_and_lines_are_appended() {
        let linea = linea("quita la muletilla", &[], &Err("sin cuota".into()), Duration::ZERO, "gemini");
        assert_eq!((linea["fin"].as_str(), linea["error"].as_str()), (Some("error"), Some("sin cuota")));
        let carpeta = std::env::temp_dir().join(format!("komunta-traza-{}", std::process::id()));
        let ruta = carpeta.join("trazas.jsonl");
        let _ = std::fs::remove_dir_all(&carpeta);
        anotar(&ruta, &linea).unwrap();
        anotar(&ruta, &linea).unwrap();
        let texto = std::fs::read_to_string(&ruta).unwrap();
        assert_eq!(texto.lines().count(), 2);
        assert!(texto.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()));
        let _ = std::fs::remove_dir_all(&carpeta);
    }
}
