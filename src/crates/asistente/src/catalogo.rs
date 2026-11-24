// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Which tools the AI sees. Every tool belongs to a family; the core is
//! always offered, the others once the AI asks for them with
//! `herramientas_buscar`, which keeps the request small - a schema the AI
//! will not use this turn is tokens paid at every step. The system prompt
//! names every family and its tools, so the AI knows what it may ask for.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::herramienta::{Clase, Herramienta, NUCLEO};

/// The tool the AI asks for families with.
pub const BUSCAR: &str = "herramientas_buscar";

/// `herramientas_buscar` as the AI sees it.
pub fn herramienta() -> Herramienta {
    Herramienta::nueva(
        BUSCAR,
        "Activa herramientas que aún no tienes. Pasa nombres de familia (los del índice de herramientas) \
         o palabras de lo que quieres hacer; activa las familias que coinciden y devuelve sus herramientas. \
         Siguen activas el resto de la conversación.",
        json!({
            "type": "object",
            "properties": {
                "consulta": {"type": "string", "description": "familias o palabras, separadas por espacios"}
            },
            "required": ["consulta"]
        }),
        Clase::Lectura,
        NUCLEO,
    )
}

/// The families `consulta` names, by family name, tool name or a word of
/// a tool's description.
pub fn buscar(todas: &[Herramienta], consulta: &str) -> BTreeSet<String> {
    let palabras: Vec<String> = consulta
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|p| p.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect();
    todas
        .iter()
        .filter(|h| h.familia != NUCLEO)
        .filter(|h| palabras.iter().any(|p| coincide(h, p)))
        .map(|h| h.familia.clone())
        .collect()
}

fn coincide(herramienta: &Herramienta, palabra: &str) -> bool {
    herramienta.familia == palabra
        || herramienta.nombre.contains(palabra)
        || herramienta.descripcion.to_lowercase().contains(palabra)
}

/// What `herramientas_buscar` answers: the tools of `familias`, one line
/// each.
pub fn respuesta(todas: &[Herramienta], familias: &BTreeSet<String>) -> Value {
    let activadas: Vec<Value> = todas
        .iter()
        .filter(|h| familias.contains(&h.familia))
        .map(|h| json!({"nombre": h.nombre, "familia": h.familia, "que_hace": primera_frase(&h.descripcion)}))
        .collect();
    json!({"activadas": activadas})
}

/// The index of families for the system prompt: each family and its
/// tools' names, so the AI asks for the right one.
pub fn indice(todas: &[Herramienta]) -> String {
    let familias: BTreeSet<&str> = todas.iter().map(|h| h.familia.as_str()).collect();
    let lineas: Vec<String> = familias
        .into_iter()
        .map(|familia| {
            let nombres: Vec<&str> = todas
                .iter()
                .filter(|h| h.familia == familia)
                .map(|h| h.nombre.as_str())
                .collect();
            let nota = if familia == NUCLEO { " (siempre activas)" } else { "" };
            format!("- {familia}{nota}: {}", nombres.join(", "))
        })
        .collect();
    format!("Índice de herramientas por familia:\n{}", lineas.join("\n"))
}

/// The tools declared: the core and the families asked for. The list only
/// grows within a conversation - a tool a guard has stopped is refused when
/// called, not taken off the list - so the request's prefix stays cached.
pub fn visibles(todas: &[Herramienta], familias: &BTreeSet<String>) -> Vec<Herramienta> {
    todas
        .iter()
        .filter(|h| h.familia == NUCLEO || familias.contains(&h.familia))
        .cloned()
        .collect()
}

/// The names of the tools among `herramientas` that only read, and that no
/// guard has stopped: what a step close to the end may call.
pub fn de_lectura(herramientas: &[Herramienta], bloqueadas: &BTreeSet<String>) -> Vec<String> {
    herramientas
        .iter()
        .filter(|h| h.clase.solo_lee() && !bloqueadas.contains(&h.nombre))
        .map(|h| h.nombre.clone())
        .collect()
}

fn primera_frase(texto: &str) -> &str {
    texto.split_inclusive(". ").next().unwrap_or(texto).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogo() -> Vec<Herramienta> {
        vec![
            Herramienta::nueva("timeline_read", "Lee la línea de tiempo.", json!({}), Clase::Lectura, NUCLEO),
            Herramienta::nueva("keyframe_set", "Pone una llave de animación. Más texto.", json!({}), Clase::Edicion, "animacion"),
            Herramienta::nueva("zoom_en", "Acerca la cámara en una frase.", json!({}), Clase::Edicion, "animacion"),
            Herramienta::nueva("effect_layer_add", "Añade un efecto.", json!({}), Clase::Edicion, "efectos"),
        ]
    }

    #[test]
    fn a_family_is_found_by_its_name_a_tool_s_name_or_a_word_it_does() {
        let todas = catalogo();
        assert_eq!(buscar(&todas, "animacion"), BTreeSet::from(["animacion".to_owned()]));
        assert_eq!(buscar(&todas, "effect_layer_add"), BTreeSet::from(["efectos".to_owned()]));
        assert_eq!(buscar(&todas, "quiero acercar la cámara"), BTreeSet::from(["animacion".to_owned()]));
        assert!(buscar(&todas, "lee").is_empty(), "the core is never searched for");
    }

    #[test]
    fn the_core_is_always_offered_the_rest_once_asked_for() {
        let todas = catalogo();
        let nada = BTreeSet::new();
        let nombres = |v: Vec<Herramienta>| v.into_iter().map(|h| h.nombre).collect::<Vec<_>>();
        assert_eq!(nombres(visibles(&todas, &nada)), ["timeline_read"]);

        let animacion = BTreeSet::from(["animacion".to_owned()]);
        let ofrecidas = visibles(&todas, &animacion);
        assert_eq!(nombres(ofrecidas.clone()), ["timeline_read", "keyframe_set", "zoom_en"]);
        assert_eq!(de_lectura(&ofrecidas, &nada), ["timeline_read"]);
        let parada = BTreeSet::from(["timeline_read".to_owned()]);
        assert!(de_lectura(&ofrecidas, &parada).is_empty());
    }

    #[test]
    fn the_index_names_every_family_and_its_tools() {
        let indice = indice(&catalogo());
        assert!(indice.contains("- animacion: keyframe_set, zoom_en"), "{indice}");
        assert!(indice.contains("- nucleo (siempre activas): timeline_read"), "{indice}");
        let respuesta = respuesta(&catalogo(), &BTreeSet::from(["efectos".to_owned()]));
        assert_eq!(respuesta["activadas"][0]["nombre"], "effect_layer_add");
    }
}
