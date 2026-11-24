// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! What an agent lets the person choose for the session - its model, how
//! much it thinks, how it works - as ACP's session config options
//! advertise them. Only the lists of the three kinds the pane offers are
//! kept; a switch, or a choice of a kind the pane does not know, is the
//! agent's own business. An agent that still speaks the older session
//! modes (Gemini CLI, November 2026) has them offered as its mode.

use agent_client_protocol::v1::schema::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigSelectOptions, SessionModeState,
};

/// The id the older session modes go by: not a config option's, so set
/// with `session/set_mode`.
pub(super) const MODO_ANTIGUO: &str = "acp:session-mode";

/// What a choice is about, in the order the pane shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Faceta {
    /// The AI the agent runs on.
    Modelo,
    /// How much it thinks before it answers.
    Razonamiento,
    /// How it works: asks first, plans, edits freely.
    Modo,
}

/// One value a choice can take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Valor {
    /// What the agent is told.
    pub id: String,
    /// What the person reads.
    pub nombre: String,
}

/// A choice the agent offers, and what it is set to now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ajuste {
    /// The option's id, for the agent.
    pub id: String,
    /// Its name, for the person.
    pub nombre: String,
    /// What it is about.
    pub faceta: Faceta,
    /// What it can be set to.
    pub valores: Vec<Valor>,
    /// The id of the value it has.
    pub actual: String,
}

impl Ajuste {
    /// Where the value it has sits among its values, if it is one of them.
    pub fn indice(&self) -> Option<usize> {
        self.valores.iter().position(|valor| valor.id == self.actual)
    }
}

/// The choices among `opciones` the pane can offer, model first; the
/// older `modos` as the mode when the options have none.
pub(super) fn de_acp(opciones: &[SessionConfigOption], modos: Option<&SessionModeState>) -> Vec<Ajuste> {
    let mut ajustes: Vec<Ajuste> = opciones.iter().filter_map(ajuste).collect();
    if !ajustes.iter().any(|ajuste| ajuste.faceta == Faceta::Modo) {
        ajustes.extend(modos.and_then(modo_antiguo));
    }
    ajustes.sort_by_key(|ajuste| ajuste.faceta);
    ajustes
}

fn modo_antiguo(modos: &SessionModeState) -> Option<Ajuste> {
    let valores: Vec<Valor> = modos
        .available_modes
        .iter()
        .map(|modo| Valor {
            id: modo.id.to_string(),
            nombre: modo.name.clone(),
        })
        .collect();
    (!valores.is_empty()).then(|| Ajuste {
        id: MODO_ANTIGUO.to_owned(),
        nombre: String::new(),
        faceta: Faceta::Modo,
        valores,
        actual: modos.current_mode_id.to_string(),
    })
}

fn ajuste(opcion: &SessionConfigOption) -> Option<Ajuste> {
    let faceta = match opcion.category.as_ref()? {
        SessionConfigOptionCategory::Model => Faceta::Modelo,
        SessionConfigOptionCategory::ThoughtLevel => Faceta::Razonamiento,
        SessionConfigOptionCategory::Mode => Faceta::Modo,
        _ => return None,
    };
    let SessionConfigKind::Select(lista) = &opcion.kind else {
        return None;
    };
    let valores: Vec<Valor> = match &lista.options {
        SessionConfigSelectOptions::Ungrouped(valores) => valores.iter().map(valor).collect(),
        SessionConfigSelectOptions::Grouped(grupos) => {
            grupos.iter().flat_map(|grupo| grupo.options.iter().map(valor)).collect()
        }
        _ => return None,
    };
    (!valores.is_empty()).then(|| Ajuste {
        id: opcion.id.to_string(),
        nombre: opcion.name.clone(),
        faceta,
        valores,
        actual: lista.current_value.to_string(),
    })
}

fn valor(opcion: &SessionConfigSelectOption) -> Valor {
    Valor {
        id: opcion.value.to_string(),
        nombre: opcion.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::v1::schema::{SessionConfigBoolean, SessionConfigSelect};

    fn lista(id: &str, categoria: SessionConfigOptionCategory, valores: &[&str]) -> SessionConfigOption {
        let valores: Vec<SessionConfigSelectOption> = valores
            .iter()
            .map(|valor| SessionConfigSelectOption::new(valor.to_string(), valor.to_uppercase()))
            .collect();
        SessionConfigOption::new(id.to_owned(), id.to_owned(), SessionConfigKind::Select(SessionConfigSelect::new(
            valores[0].value.clone(),
            valores,
        )))
        .category(categoria)
    }

    #[test]
    fn the_model_comes_first_and_switches_and_unknown_kinds_are_left_out() {
        let opciones = [
            lista("esfuerzo", SessionConfigOptionCategory::ThoughtLevel, &["low", "high"]),
            lista("otra", SessionConfigOptionCategory::Other("x".into()), &["a"]),
            lista("modelo", SessionConfigOptionCategory::Model, &["gpt", "o"]),
            SessionConfigOption::new(
                "rapido".to_owned(),
                "rapido".to_owned(),
                SessionConfigKind::Boolean(SessionConfigBoolean::new(false)),
            )
            .category(SessionConfigOptionCategory::Mode),
        ];
        let ajustes = de_acp(&opciones, None);
        let ids: Vec<&str> = ajustes.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["modelo", "esfuerzo"]);
        assert_eq!(ajustes[0].valores[1].nombre, "O");
        assert_eq!(ajustes[0].indice(), Some(0));
    }

    #[test]
    fn the_older_modes_are_the_mode_unless_the_options_have_one() {
        use agent_client_protocol::v1::schema::SessionMode;
        let modos = SessionModeState::new(
            "default".to_owned(),
            vec![SessionMode::new("default".to_owned(), "Default"), SessionMode::new("plan".to_owned(), "Plan")],
        );
        let solo = de_acp(&[], Some(&modos));
        assert_eq!(solo.len(), 1);
        assert_eq!((solo[0].id.as_str(), solo[0].faceta), (MODO_ANTIGUO, Faceta::Modo));
        let con_modo = [lista("modo", SessionConfigOptionCategory::Mode, &["ask", "code"])];
        let ids: Vec<String> = de_acp(&con_modo, Some(&modos)).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["modo"]);
    }
}
