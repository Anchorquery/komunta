// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Which of the editor's MCP tools the in-window agent has, what kind of
//! effect each has and the family it is offered with. The descriptions and
//! schemas stay concat-mcp's - what an agent over MCP reads - and only the
//! project's own path is taken out: the agent works on the project on
//! screen. Left out are the project's files (open, save, close, create),
//! templates, which make new projects, and exports: those stay the
//! person's.

use asistente::{Clase, Herramienta, NUCLEO};
use concat_mcp::tools::Toolbox;
use serde_json::Value;

/// One MCP tool offered: its name, its kind, its family.
pub struct Ofrecida {
    /// The MCP name.
    pub nombre: &'static str,
    /// What it does to the world.
    pub clase: Clase,
    /// The family it comes with.
    pub familia: &'static str,
}

const fn of(nombre: &'static str, clase: Clase, familia: &'static str) -> Ofrecida {
    Ofrecida { nombre, clase, familia }
}

/// Every MCP tool the agent has.
pub const OFRECIDAS: [Ofrecida; 28] = [
    of("timeline_read", Clase::Lectura, NUCLEO),
    of("edit_undo", Clase::Edicion, NUCLEO),
    of("edit_redo", Clase::Edicion, NUCLEO),
    of("clip_add", Clase::Edicion, "clips"),
    of("clips_add_sequence", Clase::Edicion, "clips"),
    of("clip_trim", Clase::Edicion, "clips"),
    of("clips_split", Clase::Edicion, "clips"),
    of("clips_move", Clase::Edicion, "clips"),
    of("clips_remove", Clase::Edicion, "clips"),
    of("clip_update", Clase::Edicion, "clips"),
    of("clip_speed", Clase::Edicion, "clips"),
    of("track_add", Clase::Edicion, "clips"),
    of("clip_transform", Clase::Edicion, "encuadre"),
    of("clip_frame", Clase::Edicion, "encuadre"),
    of("clip_mask", Clase::Edicion, "encuadre"),
    of("clip_stabilize", Clase::Edicion, "encuadre"),
    of("timeline_background", Clase::Edicion, "encuadre"),
    of("title_add", Clase::Edicion, "texto"),
    of("captions_add", Clase::Edicion, "texto"),
    of("keyframe_set", Clase::Edicion, "animacion"),
    of("keyframe_clear", Clase::Edicion, "animacion"),
    of("effects_list", Clase::Lectura, "efectos"),
    of("effect_layer_add", Clase::Edicion, "efectos"),
    of("effect_key_set", Clase::Edicion, "efectos"),
    of("media_probe", Clase::Lectura, "medios"),
    of("media_import", Clase::Archivo, "medios"),
    // The frame of the whole project: every clip's placement moves.
    of("project_set_video", Clase::Destructiva, "proyecto"),
    of("edit_apply", Clase::Edicion, "proyecto"),
];

/// Clips removed in one call before it waits for the person's yes.
pub const QUITAR_SIN_PREGUNTAR: usize = 3;

/// The offered tool named `nombre`.
pub fn ofrecida(nombre: &str) -> Option<&'static Ofrecida> {
    OFRECIDAS.iter().find(|o| o.nombre == nombre)
}

/// The MCP tools as the agent sees them.
pub fn herramientas(caja: &Toolbox) -> Vec<Herramienta> {
    OFRECIDAS
        .iter()
        .filter_map(|o| {
            let Some((tool, esquema)) = caja.find(o.nombre) else {
                log::warn!("asistente: concat-mcp has no {} tool", o.nombre);
                return None;
            };
            let herramienta =
                Herramienta::nueva(tool.name, tool.description, sin_ruta(esquema.clone()), o.clase, o.familia);
            Some(if tool.idempotent { herramienta.idempotente() } else { herramienta })
        })
        .collect()
}

/// A tool's schema without its `path`: the agent works on the open
/// project, and is not asked which.
pub fn sin_ruta(mut esquema: Value) -> Value {
    if let Some(propiedades) = esquema["properties"].as_object_mut() {
        propiedades.remove("path");
    }
    if let Some(requeridas) = esquema["required"].as_array_mut() {
        requeridas.retain(|campo| campo != "path");
    }
    esquema
}

/// Whether a call removes more clips than [`QUITAR_SIN_PREGUNTAR`].
pub fn quita_muchos(nombre: &str, argumentos: &Value) -> bool {
    nombre == "clips_remove"
        && argumentos["clipIds"]
            .as_array()
            .is_some_and(|ids| ids.len() > QUITAR_SIN_PREGUNTAR)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_tool_offered_is_one_concat_mcp_has_and_none_comes_twice() {
        let caja = Toolbox::new();
        for o in &OFRECIDAS {
            assert!(caja.find(o.nombre).is_some(), "concat-mcp has no {}", o.nombre);
        }
        let herramientas = herramientas(&caja);
        let mut nombres: Vec<&str> = herramientas.iter().map(|h| h.nombre.as_str()).collect();
        nombres.sort_unstable();
        nombres.dedup();
        assert_eq!(nombres.len(), OFRECIDAS.len());
        assert!(herramientas.iter().all(|h| h.esquema["properties"].get("path").is_none()));
    }

    #[test]
    fn the_project_s_files_templates_and_exports_stay_the_person_s() {
        for nombre in ["project_open", "project_save", "export_start", "template_instantiate"] {
            assert!(ofrecida(nombre).is_none(), "{nombre}");
        }
    }

    #[test]
    fn the_path_is_taken_out_of_the_schema() {
        let esquema = json!({
            "type": "object",
            "properties": {"path": {"type": "string"}, "time": {"type": "number"}},
            "required": ["path", "time"],
        });
        let limpio = sin_ruta(esquema);
        assert!(limpio["properties"].get("path").is_none());
        assert_eq!(limpio["required"], json!(["time"]));
    }

    #[test]
    fn removing_many_clips_asks_first() {
        assert!(!quita_muchos("clips_remove", &json!({"clipIds": ["a", "b"]})));
        assert!(quita_muchos("clips_remove", &json!({"clipIds": ["a", "b", "c", "d"]})));
        assert!(!quita_muchos("clips_split", &json!({"clipIds": ["a", "b", "c", "d"]})));
    }
}
