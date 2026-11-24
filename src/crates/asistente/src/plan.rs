// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The agent's plan: a list of tasks it writes whole each time, which the
//! window shows above the chat and the harness reminds it of at every step.

use serde_json::{Value, json};

use crate::herramienta::{Clase, Herramienta, NUCLEO};

/// The tool the agent writes its plan with.
pub const PLAN_ESCRIBIR: &str = "plan_escribir";

/// Where a task stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Estado {
    /// Not started.
    Pendiente,
    /// Under way; one at a time.
    EnCurso,
    /// Done.
    Hecha,
}

/// One task of the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Tarea {
    /// What it is, in a few words.
    pub texto: String,
    /// Where it stands.
    pub estado: Estado,
}

/// `plan_escribir` as the AI sees it.
pub fn herramienta() -> Herramienta {
    Herramienta::nueva(
        PLAN_ESCRIBIR,
        "Escribe tu plan para este encargo: la lista entera de tareas, en orden, cada una con su estado \
         (pendiente, en_curso, hecha). Reemplaza la anterior. Úsalo en encargos de más de dos pasos, antes \
         de empezar, y vuelve a llamarlo al terminar cada tarea, a la vez que la siguiente herramienta. \
         La persona ve el plan en la ventana.",
        json!({
            "type": "object",
            "properties": {
                "tareas": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "texto": {"type": "string"},
                            "estado": {"type": "string", "enum": ["pendiente", "en_curso", "hecha"]}
                        },
                        "required": ["texto", "estado"]
                    }
                }
            },
            "required": ["tareas"]
        }),
        Clase::Lectura,
        NUCLEO,
    )
}

/// The plan in `argumentos`, or why it cannot be read.
pub fn leer(argumentos: &Value) -> Result<Vec<Tarea>, String> {
    let tareas = argumentos["tareas"]
        .as_array()
        .ok_or("falta «tareas»: una lista de {texto, estado}")?;
    tareas.iter().map(tarea).collect()
}

fn tarea(valor: &Value) -> Result<Tarea, String> {
    let texto = valor["texto"].as_str().unwrap_or_default().trim();
    if texto.is_empty() {
        return Err("una tarea sin texto".into());
    }
    let estado = match valor["estado"].as_str().unwrap_or("pendiente") {
        "en_curso" => Estado::EnCurso,
        "hecha" => Estado::Hecha,
        _ => Estado::Pendiente,
    };
    Ok(Tarea {
        texto: texto.to_owned(),
        estado,
    })
}

/// The plan as a reminder for the AI; empty when there is none, or when
/// it is all done.
pub fn recordatorio(plan: &[Tarea]) -> String {
    if plan.iter().all(|t| t.estado == Estado::Hecha) {
        return String::new();
    }
    let lineas: Vec<String> = plan
        .iter()
        .map(|t| {
            let marca = match t.estado {
                Estado::Pendiente => "[ ]",
                Estado::EnCurso => "[>]",
                Estado::Hecha => "[x]",
            };
            format!("{marca} {}", t.texto)
        })
        .collect();
    format!("Tu plan vigente:\n{}", lineas.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_is_read_with_its_states_and_reminded_while_unfinished() {
        let plan = leer(&json!({"tareas": [
            {"texto": "Leer la línea de tiempo", "estado": "hecha"},
            {"texto": "Zoom en la frase clave", "estado": "en_curso"},
            {"texto": "Revisar el fotograma", "estado": "pendiente"}
        ]}))
        .unwrap();
        assert_eq!(plan[1].estado, Estado::EnCurso);
        let recuerdo = recordatorio(&plan);
        assert!(recuerdo.contains("[x] Leer"), "{recuerdo}");
        assert!(recuerdo.contains("[>] Zoom"), "{recuerdo}");

        let hecho: Vec<Tarea> = plan.into_iter().map(|t| Tarea { estado: Estado::Hecha, ..t }).collect();
        assert_eq!(recordatorio(&hecho), "");
    }

    #[test]
    fn a_plan_without_tasks_or_with_an_empty_one_is_refused() {
        assert!(leer(&json!({})).is_err());
        assert!(leer(&json!({"tareas": [{"texto": " ", "estado": "pendiente"}]})).is_err());
    }
}
