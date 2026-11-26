// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Which AI the agent talks to: the one the montage plans with, when it
//! plans through an API - Claude, GPT or Gemini - and has its key; Gemini
//! otherwise. Gemini goes through the harness's own adapter, the rest
//! through genai's.

use asistente::{Esfuerzo, Gemini, Generico, Proveedor};
use montaje::Ia;
use montaje::llavero::{self, LlaveroSistema};

use crate::i18n::t;
use crate::panes::montaje::{datos, env_viejo};
use crate::studio::Studio;

/// The Gemini models the agent asks, in order, before the montage's:
/// Flash, good with tools and quick with little thinking, the newest
/// first. A hang that looked like 3.8's was a reused connection waiting
/// with no limit, and is gone (November 2026).
const GEMINI: [&str; 2] = ["gemini-3.8-flash", "gemini-3.5-flash"];

/// The AI for the agent's next turn, or why there is none: the model
/// chosen under the field asked first, thinking as much as chosen there.
pub fn proveedor(studio: &Studio) -> Result<Box<dyn Proveedor>, String> {
    let (proveedor, clave) = cuenta(studio)?;
    let elegido = &studio.prefs.asistente;
    let esfuerzo = Esfuerzo::desde_id(&elegido.effort);
    let modelos = primero(modelos_de(studio, proveedor), &elegido.model);
    if proveedor == montaje::Proveedor::Google {
        let cuota = datos(studio).carpeta_cache();
        return Ok(Box::new(Gemini::new(clave, modelos, cuota).con_esfuerzo(esfuerzo)));
    }
    log::info!("asistente: talking to {} through genai", proveedor.id());
    Ok(Box::new(Generico::new(Some(clave), modelos)?.con_esfuerzo(esfuerzo)))
}

/// The models the agent can ask, in the order it asks them when none is
/// chosen; none without a key.
pub fn modelos(studio: &Studio) -> Vec<String> {
    cuenta(studio).map_or_else(|_| Vec::new(), |(proveedor, _)| modelos_de(studio, proveedor))
}

/// Komunta's Google key, for an outside agent that runs on Gemini.
pub fn clave_google(studio: &Studio) -> Option<String> {
    llavero::clave(montaje::Proveedor::Google, &LlaveroSistema, &env_viejo(studio))
}

/// The provider the agent talks to and its key: the montage's, when it
/// plans through an API whose key there is; Gemini otherwise.
fn cuenta(studio: &Studio) -> Result<(montaje::Proveedor, String), String> {
    let clave = |proveedor| llavero::clave(proveedor, &LlaveroSistema, &env_viejo(studio));
    let elegido = elegido(&studio.prefs.montaje.ia);
    if elegido != montaje::Proveedor::Google
        && let Some(clave) = clave(elegido)
    {
        return Ok((elegido, clave));
    }
    let google = montaje::Proveedor::Google;
    clave(google).map(|clave| (google, clave)).ok_or_else(|| t("asistente.noKey"))
}

/// The provider the montage plans with, when it is one reached by API.
fn elegido(ia: &Ia) -> montaje::Proveedor {
    ia.cerebro
        .as_deref()
        .and_then(montaje::Proveedor::desde_id)
        .unwrap_or(montaje::Proveedor::Google)
}

/// The models of `proveedor` in the order they are asked: the montage's
/// one for an API other than Google's; for Gemini, the agent's own first,
/// then the montage's.
fn modelos_de(studio: &Studio, proveedor: montaje::Proveedor) -> Vec<String> {
    let ia = &studio.prefs.montaje.ia;
    if proveedor != montaje::Proveedor::Google {
        return vec![ia.modelo(proveedor).to_owned()];
    }
    let del_montaje = montaje::descripcion::modelos(proveedor, ia, &datos(studio).ajustes(ia));
    con_los_del_montaje(del_montaje)
}

/// [`GEMINI`], and then the montage's models that are neither those nor
/// of a generation before Gemini 3: the montage keeps 2.5 Flash as its
/// last resort, which the person asked the chat not to fall back to - it
/// says the quota is spent instead.
fn con_los_del_montaje(del_montaje: Vec<String>) -> Vec<String> {
    let viejo = |modelo: &str| modelo.starts_with("gemini-1") || modelo.starts_with("gemini-2");
    let mut modelos: Vec<String> = GEMINI.map(str::to_owned).into();
    modelos.extend(
        del_montaje
            .into_iter()
            .filter(|modelo| !GEMINI.contains(&modelo.as_str()) && !viejo(modelo)),
    );
    modelos
}

/// `modelos` with `elegido` first, when it is one of them; the rest stay
/// behind it, for when it fails.
fn primero(mut modelos: Vec<String>, elegido: &str) -> Vec<String> {
    if let Some(indice) = modelos.iter().position(|modelo| modelo == elegido) {
        let modelo = modelos.remove(indice);
        modelos.insert(0, modelo);
    }
    modelos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con_cerebro(cerebro: &str) -> Ia {
        Ia {
            cerebro: Some(cerebro.to_owned()),
            ..Ia::default()
        }
    }

    #[test]
    fn the_agent_talks_to_the_api_the_montage_plans_with_and_gemini_otherwise() {
        assert_eq!(elegido(&con_cerebro("anthropic")), montaje::Proveedor::Anthropic);
        assert_eq!(elegido(&con_cerebro("openai")), montaje::Proveedor::OpenAi);
        assert_eq!(elegido(&con_cerebro("claude_code")), montaje::Proveedor::Google);
        assert_eq!(elegido(&Ia::default()), montaje::Proveedor::Google);
    }

    #[test]
    fn the_chat_asks_the_newest_flash_first_and_never_an_old_generation() {
        let del_montaje = ["gemini-flash-latest", "gemini-2.5-flash", "gemini-3.5-flash"].map(str::to_owned);
        assert_eq!(
            con_los_del_montaje(del_montaje.into()),
            ["gemini-3.8-flash", "gemini-3.5-flash", "gemini-flash-latest"]
        );
    }

    #[test]
    fn the_model_chosen_is_asked_first_and_the_rest_stay_behind_it() {
        let modelos = || vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(primero(modelos(), "c"), ["c", "a", "b"]);
        assert_eq!(primero(modelos(), ""), ["a", "b", "c"]);
        assert_eq!(primero(modelos(), "otro"), ["a", "b", "c"]);
    }
}
