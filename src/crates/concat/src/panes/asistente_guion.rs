// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The agent's tools for a montage's script (phase F3 of
//! `docs/asistente-ia.md`): reading it, cutting words, takes, captions and
//! B-roll out of it, its versions, and "Aplicar al vídeo". They work on the
//! Guion pane's own plan ([`crate::panes::guion::asistente`]) through a
//! [`Mesa`]: the window when the agent runs, a fake one in the tests.
//!
//! An edit stays in the pane, unapplied, the same as one made by hand: it
//! is not part of the turn's Ctrl+Z, `guion_descartar` throws away every
//! edit not applied, and `guion_volver` brings back an applied version.

use asistente::{Clase, Herramienta, Salida, Tipo};
use serde_json::{Value, json};

use super::asistente_vivo::Vivo;
use super::guion::GuionPane;
use super::guion::asistente::Operacion;
use crate::studio::Studio;

/// The family every script tool belongs to.
const FAMILIA: &str = "guion";

const LEER: &str = "guion_leer";
const PALABRAS: &str = "guion_palabras";
const TOMA_QUITAR: &str = "guion_toma_quitar";
const TOMA_MOVER: &str = "guion_toma_mover";
const SUBTITULO_CAMBIAR: &str = "guion_subtitulo_cambiar";
const SUBTITULO_QUITAR: &str = "guion_subtitulo_quitar";
const RECURSO_QUITAR: &str = "guion_recurso_quitar";
const DESCARTAR: &str = "guion_descartar";
const VERSIONES: &str = "guion_versiones";
/// Brings an applied version back: asks first.
pub const VOLVER: &str = "guion_volver";
/// Lays the script out into the project: asks first.
pub const APLICAR: &str = "guion_aplicar";

const TODAS: [&str; 11] = [
    LEER,
    PALABRAS,
    TOMA_QUITAR,
    TOMA_MOVER,
    SUBTITULO_CAMBIAR,
    SUBTITULO_QUITAR,
    RECURSO_QUITAR,
    DESCARTAR,
    VERSIONES,
    VOLVER,
    APLICAR,
];

/// What the script tools act on.
pub trait Mesa {
    /// The script: every take in short, or take `toma` whole.
    fn leer(&mut self, toma: Option<usize>) -> Result<Value, String>;
    /// One edit to the script in the pane.
    fn editar(&mut self, operacion: Operacion) -> Result<Value, String>;
    /// The plan's versions.
    fn versiones(&mut self) -> Result<Value, String>;
    /// Version `n` brought back.
    fn volver(&mut self, n: u32) -> Result<Value, String>;
    /// The script laid out into the project once the turn is over.
    fn aplicar(&mut self) -> Result<Value, String>;
}

/// Whether `nombre` is one of these tools.
pub fn es_del_guion(nombre: &str) -> bool {
    TODAS.contains(&nombre)
}

/// The tools, as the agent sees them.
pub fn herramientas() -> Vec<Herramienta> {
    let toma = json!({"type": "integer", "minimum": 0, "description": "número de toma, como lo da guion_leer"});
    let k = json!({"type": "integer", "minimum": 0, "description": "el «k» que da guion_leer"});
    let objeto = |propiedades: Value, requeridas: &[&str]| {
        json!({"type": "object", "properties": propiedades, "required": requeridas})
    };
    let edicion = |nombre, descripcion, esquema| Herramienta::nueva(nombre, descripcion, esquema, Clase::Edicion, FAMILIA);
    vec![
        Herramienta::nueva(
            LEER,
            "Lee el guion del montaje con IA del proyecto: sus tomas en orden, con las palabras (i:texto; \
             ~i:texto~ si está quitada; ° si es una coletilla), subtítulos y recursos, y los avisos de la revisión. \
             Sin «toma» resume cada toma; con «toma» da todas sus palabras.",
            objeto(json!({"toma": toma}), &[]),
            Clase::Lectura,
            FAMILIA,
        ),
        edicion(
            PALABRAS,
            "Quita del vídeo las palabras «desde»..«hasta» (ambas incluidas, por su «i») de una toma, o las \
             devuelve con quitar=false. Para muletillas, repeticiones y frases que sobran.",
            objeto(
                json!({
                    "toma": toma,
                    "desde": {"type": "integer", "minimum": 0},
                    "hasta": {"type": "integer", "minimum": 0},
                    "quitar": {"type": "boolean", "description": "true por defecto"}
                }),
                &["toma", "desde", "hasta"],
            ),
        ),
        edicion(
            TOMA_QUITAR,
            "Quita una toma entera del guion, con sus subtítulos y recursos. Las tomas siguientes se renumeran.",
            objeto(json!({"toma": toma}), &["toma"]),
        ),
        edicion(
            TOMA_MOVER,
            "Mueve una toma un puesto antes (-1) o después (1), con sus subtítulos y recursos.",
            objeto(json!({"toma": toma, "hacia": {"type": "integer", "enum": [-1, 1]}}), &["toma", "hacia"]),
        ),
        edicion(
            SUBTITULO_CAMBIAR,
            "Cambia el texto de un subtítulo del guion.",
            objeto(json!({"k": k, "texto": {"type": "string"}}), &["k", "texto"]),
        ),
        edicion(
            SUBTITULO_QUITAR,
            "Quita un subtítulo del guion; los recursos puestos sobre él se van con él.",
            objeto(json!({"k": k}), &["k"]),
        ),
        edicion(
            RECURSO_QUITAR,
            "Quita un recurso (B-roll) del guion.",
            objeto(json!({"k": k}), &["k"]),
        ),
        edicion(
            DESCARTAR,
            "Tira todos los cambios del guion que aún no se han aplicado al vídeo.",
            objeto(json!({}), &[]),
        ),
        Herramienta::nueva(
            VERSIONES,
            "Lista las versiones guardadas del plan del montaje (la IA, la segunda pasada, cada vez que se aplicó).",
            objeto(json!({}), &[]),
            Clase::Lectura,
            FAMILIA,
        ),
        Herramienta::nueva(
            VOLVER,
            "Vuelve el guion a una versión guardada; el plan de ahora se guarda antes como versión. \
             Los cambios sin aplicar se pierden.",
            objeto(json!({"n": {"type": "integer", "minimum": 1}}), &["n"]),
            Clase::Destructiva,
            FAMILIA,
        ),
        Herramienta::nueva(
            APLICAR,
            "Aplica el guion al vídeo, como el botón «Crear versión con mis cambios», en cuanto acaba el turno: guarda \
             el plan, lo monta y añade líneas de tiempo nuevas al proyecto, que se guarda y se abre otra vez \
             con ellas delante. Las líneas de tiempo de antes quedan como estaban. Llámala al final, \
             cuando el guion esté como debe.",
            objeto(json!({}), &[]),
            Clase::Destructiva,
            FAMILIA,
        ),
    ]
}

/// Runs the tool `nombre` with `argumentos` on `mesa`.
pub fn ejecutar(mesa: &mut dyn Mesa, nombre: &str, argumentos: &Value) -> Salida {
    let resultado = match nombre {
        LEER => leer(mesa, argumentos),
        VERSIONES => mesa.versiones(),
        VOLVER => numero(argumentos, "n").and_then(|n| mesa.volver(n as u32)),
        APLICAR => mesa.aplicar(),
        otra => operacion(otra, argumentos).and_then(|op| mesa.editar(op)),
    };
    match resultado {
        Ok(valor) => Salida::bien(valor),
        Err(motivo) => Salida::mal(Tipo::Error, motivo).con_sugerencia("lee guion_leer para ver tomas, palabras y «k»"),
    }
}

fn leer(mesa: &mut dyn Mesa, argumentos: &Value) -> Result<Value, String> {
    let toma = match argumentos.get("toma") {
        None | Some(Value::Null) => None,
        Some(_) => Some(numero(argumentos, "toma")?),
    };
    mesa.leer(toma)
}

/// The edit a call asks for.
fn operacion(nombre: &str, argumentos: &Value) -> Result<Operacion, String> {
    let n = |clave| numero(argumentos, clave);
    Ok(match nombre {
        PALABRAS => Operacion::Palabras {
            toma: n("toma")?,
            desde: n("desde")?,
            hasta: n("hasta")?,
            quitar: argumentos["quitar"].as_bool().unwrap_or(true),
        },
        TOMA_QUITAR => Operacion::QuitarToma(n("toma")?),
        TOMA_MOVER => Operacion::MoverToma {
            toma: n("toma")?,
            hacia: argumentos["hacia"]
                .as_i64()
                .and_then(|h| i32::try_from(h).ok())
                .ok_or("falta «hacia»: -1 o 1")?,
        },
        SUBTITULO_CAMBIAR => Operacion::CambiarSubtitulo {
            k: n("k")?,
            texto: argumentos["texto"]
                .as_str()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or("falta «texto»")?
                .to_owned(),
        },
        SUBTITULO_QUITAR => Operacion::QuitarSubtitulo(n("k")?),
        RECURSO_QUITAR => Operacion::QuitarRecurso(n("k")?),
        DESCARTAR => Operacion::Descartar,
        otra => return Err(format!("no hay ninguna herramienta del guion «{otra}»")),
    })
}

fn numero(argumentos: &Value, clave: &str) -> Result<usize, String> {
    let valor = &argumentos[clave];
    valor
        .as_u64()
        .or_else(|| valor.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64))
        .map(|n| n as usize)
        .ok_or_else(|| format!("falta «{clave}», un número entero desde 0"))
}

/// What a script call to approve will do, in the person's words; None
/// for one that does not ask.
pub fn detalle(nombre: &str, argumentos: &Value) -> Option<String> {
    match nombre {
        APLICAR => Some(crate::i18n::t("asistente.approve.script")),
        VOLVER => Some(crate::i18n::tf("asistente.approve.version", &[&argumentos["n"]])),
        _ => None,
    }
}

impl Mesa for Vivo {
    fn leer(&mut self, toma: Option<usize>) -> Result<Value, String> {
        self.en_guion(move |guion, _| guion.para_asistente(toma))
    }

    fn editar(&mut self, operacion: Operacion) -> Result<Value, String> {
        self.en_guion(move |guion, _| guion.editar_para_asistente(operacion))
    }

    fn versiones(&mut self) -> Result<Value, String> {
        self.en_guion(|guion, _| guion.versiones_para_asistente())
    }

    fn volver(&mut self, n: u32) -> Result<Value, String> {
        self.en_guion(move |guion, _| guion.volver_para_asistente(n))
    }

    fn aplicar(&mut self) -> Result<Value, String> {
        self.en_guion(|guion, _| guion.pedir_aplicar())
    }
}

impl Vivo {
    /// Runs `trabajo` on the Guion pane in the window, of the project the
    /// turn is in.
    fn en_guion<T: Send + 'static>(
        &self,
        trabajo: impl FnOnce(&mut GuionPane, &mut Studio) -> Result<T, String>
        + Send
        + 'static,
    ) -> Result<T, String> {
        self.en_ventana(move |studio, _| {
            studio
                .guion_asistente(trabajo)
                .map_err(concat_api::ApiError::failed)
        })
        .map_err(|error| error.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A desk that keeps the edits it is asked for.
    #[derive(Default)]
    struct Apuntes(Vec<Operacion>);

    impl Mesa for Apuntes {
        fn leer(&mut self, toma: Option<usize>) -> Result<Value, String> {
            Ok(json!({"toma": toma}))
        }
        fn editar(&mut self, operacion: Operacion) -> Result<Value, String> {
            self.0.push(operacion);
            Ok(json!({}))
        }
        fn versiones(&mut self) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn volver(&mut self, n: u32) -> Result<Value, String> {
            Ok(json!({"n": n}))
        }
        fn aplicar(&mut self) -> Result<Value, String> {
            Err("no hay ventana".into())
        }
    }

    fn llamar(mesa: &mut Apuntes, nombre: &str, argumentos: Value) -> Salida {
        ejecutar(mesa, nombre, &argumentos)
    }

    #[test]
    fn every_script_tool_is_declared_once_in_its_family() {
        let nombres: Vec<String> = herramientas().into_iter().map(|h| h.nombre).collect();
        assert_eq!(nombres, TODAS);
        assert!(herramientas().iter().all(|h| h.familia == FAMILIA));
        let piden: Vec<&str> = TODAS
            .iter()
            .copied()
            .filter(|n| herramientas().iter().any(|h| h.nombre == *n && h.clase.pide_aprobacion()))
            .collect();
        assert_eq!(piden, [VOLVER, APLICAR]);
    }

    #[test]
    fn a_call_becomes_the_edit_it_names() {
        let mut mesa = Apuntes::default();
        assert!(llamar(&mut mesa, PALABRAS, json!({"toma": 1, "desde": 4, "hasta": 6})).ok());
        assert!(llamar(&mut mesa, PALABRAS, json!({"toma": 1, "desde": 4, "hasta": 6.0, "quitar": false})).ok());
        assert!(llamar(&mut mesa, TOMA_MOVER, json!({"toma": 2, "hacia": -1})).ok());
        assert!(llamar(&mut mesa, SUBTITULO_CAMBIAR, json!({"k": 0, "texto": " Kaixo "})).ok());
        assert!(llamar(&mut mesa, DESCARTAR, json!({})).ok());
        assert_eq!(
            mesa.0,
            [
                Operacion::Palabras { toma: 1, desde: 4, hasta: 6, quitar: true },
                Operacion::Palabras { toma: 1, desde: 4, hasta: 6, quitar: false },
                Operacion::MoverToma { toma: 2, hacia: -1 },
                Operacion::CambiarSubtitulo { k: 0, texto: "Kaixo".into() },
                Operacion::Descartar,
            ]
        );
    }

    #[test]
    fn a_call_missing_what_it_needs_fails_with_a_hint_and_edits_nothing() {
        let mut mesa = Apuntes::default();
        let salida = llamar(&mut mesa, TOMA_QUITAR, json!({"toma": -1}));
        assert_eq!(salida.fallo, Some(Tipo::Error));
        assert!(salida.sugerencia.is_some());
        assert!(!llamar(&mut mesa, SUBTITULO_CAMBIAR, json!({"k": 0, "texto": "  "})).ok());
        assert!(mesa.0.is_empty());
        assert_eq!(llamar(&mut mesa, LEER, json!({})).valor, json!({"toma": null}));
        assert_eq!(llamar(&mut mesa, LEER, json!({"toma": 3})).valor, json!({"toma": 3}));
    }
}
