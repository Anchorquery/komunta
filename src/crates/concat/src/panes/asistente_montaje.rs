// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The agent's tools for montage with AI (phase F4 of
//! `docs/asistente-ia.md`): how the open project was montaged, the kinds of
//! video and brand kits there are, and a montage started or stopped. They
//! work on the Montaje pane's sheet ([`crate::panes::montaje::asistente`])
//! through a [`Taller`]: the window when the agent runs, a fake one in the
//! tests. A montage starts once the turn is over: laying it out opens the
//! project again.

use asistente::{Clase, Herramienta, Salida, Tipo};
use serde_json::{Value, json};

use super::asistente_vivo::Vivo;
use super::montaje::MontajePane;
use super::montaje::asistente::Cambios;
use crate::studio::Studio;

/// The family every montage tool belongs to.
const FAMILIA: &str = "montaje";

const ESTADO: &str = "montaje_estado";
const OPCIONES: &str = "montaje_opciones";
/// Starts a montage, which spends AI calls: asks first.
pub const LANZAR: &str = "montaje_lanzar";
const CANCELAR: &str = "montaje_cancelar";

const TODAS: [&str; 4] = [ESTADO, OPCIONES, LANZAR, CANCELAR];

/// What the montage tools act on.
pub trait Taller {
    /// How the project was montaged, and the run under way.
    fn estado(&mut self) -> Result<Value, String>;
    /// The kinds of video and brand kits to choose from.
    fn opciones(&mut self) -> Result<Value, String>;
    /// A montage asked for, to start once the turn is over.
    fn lanzar(&mut self, cambios: Cambios) -> Result<Value, String>;
    /// The run under way stopped.
    fn cancelar(&mut self) -> Result<Value, String>;
}

/// Whether `nombre` is one of these tools.
pub fn es_del_montaje(nombre: &str) -> bool {
    TODAS.contains(&nombre)
}

/// The tools, as the agent sees them.
pub fn herramientas() -> Vec<Herramienta> {
    let vacio = json!({"type": "object", "properties": {}});
    vec![
        Herramienta::nueva(
            ESTADO,
            "Dice si el proyecto abierto se montó con IA y cómo se pidió (receta, «¿Qué quieres?», material, \
             marca, cuántas veces), y cómo va un montaje en marcha.",
            vacio.clone(),
            Clase::Lectura,
            FAMILIA,
        ),
        Herramienta::nueva(
            OPCIONES,
            "Lista los tipos de vídeo (recetas, con su id) y los kits de marca que se pueden pedir en un montaje.",
            vacio.clone(),
            Clase::Lectura,
            FAMILIA,
        ),
        Herramienta::nueva(
            LANZAR,
            "Monta el proyecto abierto con IA, como «Montar con IA» del panel Montaje: analiza el material, \
             pide el plan a la IA y añade el montaje al proyecto como líneas de tiempo nuevas. Empieza al \
             acabar el turno y tarda minutos. Lo que no pases queda como en el último montaje del proyecto \
             (rehacer); en un proyecto sin montaje hace falta «material». Gasta llamadas a la IA.",
            json!({
                "type": "object",
                "properties": {
                    "material": {"type": "string", "description": "carpeta con los vídeos, fotos y documentos"},
                    "peticion": {"type": "string", "description": "«¿Qué quieres?»: lo que debe contar el vídeo"},
                    "receta": {"type": "string", "description": "id de montaje_opciones; «automatica» deja elegir a la IA"},
                    "marca": {"type": "string", "description": "id del kit de marca; vacío para ninguno"},
                    "idioma": {"type": "string", "enum": ["es", "eu", "auto"]}
                }
            }),
            Clase::Gasto,
            FAMILIA,
        ),
        Herramienta::nueva(
            CANCELAR,
            "Para el montaje en marcha. Lo hecho hasta ahí no se añade al proyecto.",
            vacio,
            Clase::Edicion,
            FAMILIA,
        ),
    ]
}

/// Runs the tool `nombre` with `argumentos` on `taller`.
pub fn ejecutar(taller: &mut dyn Taller, nombre: &str, argumentos: &Value) -> Salida {
    let resultado = match nombre {
        ESTADO => taller.estado(),
        OPCIONES => taller.opciones(),
        LANZAR => taller.lanzar(cambios(argumentos)),
        CANCELAR => taller.cancelar(),
        otra => Err(format!("no hay ninguna herramienta del montaje «{otra}»")),
    };
    match resultado {
        Ok(valor) => Salida::bien(valor),
        Err(motivo) => Salida::mal(Tipo::Error, motivo),
    }
}

/// The sheet's changes a call asks for; blank text counts as not given,
/// but for the brand kit, where it means none.
fn cambios(argumentos: &Value) -> Cambios {
    let texto = |clave: &str| {
        argumentos[clave]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    Cambios {
        material: texto("material"),
        peticion: texto("peticion"),
        receta: texto("receta"),
        marca: argumentos["marca"].as_str().map(|m| m.trim().to_owned()),
        idioma: texto("idioma"),
    }
}

/// What a montage call to approve will do, in the person's words; None
/// for one that does not ask.
pub fn detalle(nombre: &str, argumentos: &Value) -> Option<String> {
    (nombre == LANZAR).then(|| {
        let receta = argumentos["receta"].as_str().unwrap_or("—");
        crate::i18n::tf("asistente.approve.montage", &[&receta])
    })
}

impl Taller for Vivo {
    fn estado(&mut self) -> Result<Value, String> {
        self.en_montaje(|montaje, studio| Ok(montaje.estado_para_asistente(studio)))
    }

    fn opciones(&mut self) -> Result<Value, String> {
        self.en_montaje(|montaje, studio| Ok(montaje.opciones_para_asistente(studio)))
    }

    fn lanzar(&mut self, cambios: Cambios) -> Result<Value, String> {
        self.en_montaje(move |montaje, studio| montaje.pedir_montaje(studio, cambios))
    }

    fn cancelar(&mut self) -> Result<Value, String> {
        self.en_montaje(|montaje, _| montaje.cancelar_para_asistente())
    }
}

impl Vivo {
    /// Runs `trabajo` on the Montaje pane in the window, of the project the
    /// turn is in.
    fn en_montaje<T: Send + 'static>(
        &self,
        trabajo: impl FnOnce(&mut MontajePane, &mut Studio) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.en_ventana(move |studio, _| {
            studio
                .montaje_asistente(trabajo)
                .map_err(concat_api::ApiError::failed)
        })
        .map_err(|error| error.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Pedidos(Vec<Cambios>);

    impl Taller for Pedidos {
        fn estado(&mut self) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn opciones(&mut self) -> Result<Value, String> {
            Ok(json!({}))
        }
        fn lanzar(&mut self, cambios: Cambios) -> Result<Value, String> {
            self.0.push(cambios);
            Ok(json!({"pendiente": true}))
        }
        fn cancelar(&mut self) -> Result<Value, String> {
            Err("no hay ningún montaje en marcha".into())
        }
    }

    #[test]
    fn every_montage_tool_is_declared_once_and_only_starting_one_asks() {
        let todas = herramientas();
        let nombres: Vec<&str> = todas.iter().map(|h| h.nombre.as_str()).collect();
        assert_eq!(nombres, TODAS);
        let piden: Vec<&str> = todas
            .iter()
            .filter(|h| h.clase.pide_aprobacion())
            .map(|h| h.nombre.as_str())
            .collect();
        assert_eq!(piden, [LANZAR]);
        assert!(todas.iter().all(|h| h.familia == FAMILIA));
    }

    #[test]
    fn what_is_left_out_stays_as_the_last_montage_had_it() {
        let mut taller = Pedidos::default();
        let argumentos = json!({"peticion": " Un vídeo de 30 s ", "receta": "", "marca": ""});
        assert!(ejecutar(&mut taller, LANZAR, &argumentos).ok());
        assert_eq!(
            taller.0,
            [Cambios {
                peticion: Some("Un vídeo de 30 s".into()),
                marca: Some(String::new()),
                ..Cambios::default()
            }]
        );
        assert_eq!(ejecutar(&mut taller, CANCELAR, &json!({})).fallo, Some(Tipo::Error));
    }
}
