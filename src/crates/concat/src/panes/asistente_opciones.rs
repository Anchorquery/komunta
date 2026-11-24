// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The choices under the assistant's field, as its composer shows them:
//! for Komunta's own agent, the model it asks first and how much it
//! thinks; for an outside one, whatever its session offers - its model,
//! its thinking, its mode - once it has one open.

use asistente::Esfuerzo;
use asistente::externo::{Ajuste, Faceta};

use crate::i18n::t;
use crate::prefs::AsistentePrefs;

/// A choice as the composer draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Opcion {
    pub faceta: Faceta,
    /// Its values, as the person reads them.
    pub valores: Vec<String>,
    /// Which of them it has.
    pub actual: usize,
}

/// Komunta's own: the model - the automatic order, or one of `modelos`
/// first - and how much it thinks. Without models, no key: no choice of
/// model either.
pub fn propias(modelos: &[String], prefs: &AsistentePrefs) -> Vec<Opcion> {
    let mut opciones = Vec::new();
    if !modelos.is_empty() {
        let mut valores = vec![t("asistente.model.auto")];
        valores.extend(modelos.iter().map(|modelo| legible(modelo)));
        let actual = modelos.iter().position(|modelo| *modelo == prefs.model).map_or(0, |i| i + 1);
        opciones.push(Opcion {
            faceta: Faceta::Modelo,
            valores,
            actual,
        });
    }
    let esfuerzo = Esfuerzo::desde_id(&prefs.effort);
    opciones.push(Opcion {
        faceta: Faceta::Razonamiento,
        valores: Esfuerzo::TODOS.map(|e| t(&format!("asistente.effort.{}", e.id()))).into(),
        actual: Esfuerzo::TODOS.iter().position(|e| *e == esfuerzo).unwrap_or(0),
    });
    opciones
}

/// Sets Komunta's own choice `faceta` to its value at `valor`.
pub fn elegir_propia(prefs: &mut AsistentePrefs, modelos: &[String], faceta: Faceta, valor: usize) {
    match faceta {
        Faceta::Modelo => {
            prefs.model = valor.checked_sub(1).and_then(|i| modelos.get(i)).cloned().unwrap_or_default();
        }
        Faceta::Razonamiento => {
            prefs.effort = Esfuerzo::TODOS.get(valor).copied().unwrap_or_default().id().to_owned();
        }
        Faceta::Modo => {}
    }
}

/// An outside agent's, as its session offers them.
pub fn externas(ajustes: &[Ajuste]) -> Vec<Opcion> {
    ajustes
        .iter()
        .map(|ajuste| Opcion {
            faceta: ajuste.faceta,
            valores: ajuste.valores.iter().map(|valor| valor.nombre.clone()).collect(),
            actual: ajuste.indice().unwrap_or(0),
        })
        .collect()
}

/// A model's id as the person reads it: "gemini-3.5-flash" is "Gemini
/// 3.5 Flash", "claude-sonnet-5-5" is "Claude Sonnet 5.5".
fn legible(modelo: &str) -> String {
    let mut palabras: Vec<String> = Vec::new();
    for parte in modelo.split('-').filter(|parte| !parte.is_empty()) {
        let numero = parte.chars().all(|c| c.is_ascii_digit() || c == '.');
        match palabras.last_mut() {
            Some(anterior) if numero && anterior.chars().all(|c| c.is_ascii_digit() || c == '.') => {
                anterior.push('.');
                anterior.push_str(parte);
            }
            _ => palabras.push(mayuscula(parte)),
        }
    }
    palabras.join(" ")
}

fn mayuscula(palabra: &str) -> String {
    let mut letras = palabra.chars();
    letras.next().map_or_else(String::new, |primera| primera.to_uppercase().chain(letras).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_s_id_reads_as_its_name() {
        assert_eq!(legible("gemini-3.5-flash"), "Gemini 3.5 Flash");
        assert_eq!(legible("claude-sonnet-5-5"), "Claude Sonnet 5.5");
        assert_eq!(legible("gpt-6"), "Gpt 6");
    }

    #[test]
    fn the_model_chosen_and_the_thinking_round_trip_through_the_preferences() {
        let modelos = ["gemini-3.5-flash".to_owned(), "gemini-3.8-flash".to_owned()];
        let mut prefs = AsistentePrefs::default();
        let actual = |prefs: &AsistentePrefs| propias(&modelos, prefs).iter().map(|o| o.actual).collect::<Vec<_>>();
        assert_eq!(actual(&prefs), [0, 0]);
        elegir_propia(&mut prefs, &modelos, Faceta::Modelo, 2);
        elegir_propia(&mut prefs, &modelos, Faceta::Razonamiento, 2);
        assert_eq!((prefs.model.as_str(), prefs.effort.as_str()), ("gemini-3.8-flash", "high"));
        assert_eq!(actual(&prefs), [2, 2]);
        elegir_propia(&mut prefs, &modelos, Faceta::Modelo, 0);
        assert_eq!(prefs.model, "");
    }

    #[test]
    fn without_a_key_there_is_no_model_to_choose() {
        let opciones = propias(&[], &AsistentePrefs::default());
        assert_eq!(opciones.len(), 1);
        assert_eq!(opciones[0].faceta, Faceta::Razonamiento);
    }
}
