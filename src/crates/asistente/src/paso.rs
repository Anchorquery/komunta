// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Getting each step ready: the one place that decides what the AI is
//! shown before it is asked - the pictures it may still see, the tools
//! declared and which of them it may call.
//!
//! Everything here keeps the request's prefix as it was, which is what the
//! provider caches: the tool list only grows, a step near the end narrows
//! what may be called through [`Modo`] rather than by taking tools away,
//! and old pictures are dropped in a batch, now and then, rather than one
//! at every step.

use crate::catalogo;
use crate::conversacion::{Conversacion, Mensaje, Parte};
use crate::guardia::{Guardia, Margen};
use crate::herramienta::Herramienta;
use crate::proveedor::Modo;

/// Frames kept as pictures before the old ones are dropped...
pub const IMAGENES: usize = 4;
/// ...and how many stay after.
pub const QUEDAN: usize = 1;

/// What a step is asked with.
pub struct Paso {
    /// The messages, as the AI is shown them.
    pub mensajes: Vec<Mensaje>,
    /// The tools declared.
    pub herramientas: Vec<Herramienta>,
    /// Which of them it may call.
    pub modo: Modo,
}

/// Step `n` of a turn over `conversacion`, with `todas` the tools there
/// are.
pub fn preparar(conversacion: &mut Conversacion, todas: &[Herramienta], guardia: &Guardia, n: usize) -> Paso {
    olvidar_imagenes_viejas(conversacion);
    let herramientas = catalogo::visibles(todas, &conversacion.familias);
    let modo = match guardia.margen(n) {
        Margen::Pleno => Modo::Libre,
        Margen::SoloLectura => Modo::SoloLeer(catalogo::de_lectura(&herramientas, &guardia.bloqueadas)),
        Margen::Cerrar => Modo::Ninguna,
    };
    Paso {
        mensajes: conversacion.mensajes.clone(),
        herramientas,
        modo,
    }
}

/// What the harness tells the AI, marked as not the person's words. It
/// travels inside a tool's result, never as a message of the person's:
/// a Gemini turn starts at the person's last words, and a note there would
/// start a new one.
pub fn nota_del_arnes(texto: &str) -> String {
    format!("[Nota de Komunta, no escrita por la persona] {texto}")
}

/// What the AI is told about the step that follows, as the budget left
/// has it; None when it may go on as before.
pub fn aviso_de_margen(margen: Margen) -> Option<&'static str> {
    match margen {
        Margen::Pleno => None,
        Margen::SoloLectura => Some("Quedan pocos pasos: solo puedes leer y comprobar. Cierra lo que tengas abierto."),
        Margen::Cerrar => Some("Se acaba el turno: no quedan herramientas. Di a la persona qué has hecho y qué falta."),
    }
}

/// Once there are more than [`IMAGENES`] pictures, every one but the last
/// [`QUEDAN`] becomes the line that says what it was: a frame costs about a
/// thousand tokens at every step.
fn olvidar_imagenes_viejas(conversacion: &mut Conversacion) {
    let con_imagen = |parte: &Parte| matches!(parte, Parte::Resultado { imagen: Some(_), .. });
    let total = conversacion
        .mensajes
        .iter()
        .flat_map(|m| &m.partes)
        .filter(|p| con_imagen(p))
        .count();
    if total <= IMAGENES {
        return;
    }
    let mut sobran = total - QUEDAN;
    for parte in conversacion.mensajes.iter_mut().flat_map(|m| &mut m.partes) {
        if sobran == 0 {
            return;
        }
        if let Parte::Resultado { imagen, valor, .. } = parte
            && let Some(vista) = imagen.take()
        {
            valor["fotograma"] = serde_json::json!(format!("visto antes: {}", vista.nota));
            sobran -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::conversacion::Rol;
    use crate::guardia::Limites;
    use crate::herramienta::{Clase, Imagen, NUCLEO};

    fn fotograma(n: u8) -> Parte {
        Parte::Resultado {
            id: format!("c{n}"),
            nombre: "ver_fotograma".into(),
            valor: json!({"ok": true}),
            error: false,
            imagen: Some(Imagen {
                mime: "image/jpeg".into(),
                datos: vec![n],
                nota: format!("t={n} s"),
            }),
        }
    }

    fn con_fotogramas(n: u8) -> Conversacion {
        let mut conversacion = Conversacion::default();
        for i in 0..n {
            conversacion.mensajes.push(Mensaje {
                rol: Rol::Usuario,
                partes: vec![fotograma(i)],
                crudo: None,
            });
        }
        conversacion
    }

    fn imagenes(conversacion: &Conversacion) -> Vec<bool> {
        conversacion
            .mensajes
            .iter()
            .flat_map(|m| &m.partes)
            .map(|p| matches!(p, Parte::Resultado { imagen: Some(_), .. }))
            .collect()
    }

    #[test]
    fn old_frames_are_dropped_in_a_batch_not_one_a_step() {
        let guardia = Guardia::new(Limites::default());
        let mut conversacion = con_fotogramas(4);
        preparar(&mut conversacion, &[], &guardia, 0);
        assert_eq!(imagenes(&conversacion), [true; 4], "four fit");

        let mut conversacion = con_fotogramas(5);
        preparar(&mut conversacion, &[], &guardia, 0);
        assert_eq!(imagenes(&conversacion), [false, false, false, false, true]);
        match &conversacion.mensajes[0].partes[0] {
            Parte::Resultado { valor, .. } => assert_eq!(valor["fotograma"], "visto antes: t=0 s"),
            otra => panic!("{otra:?}"),
        }
    }

    #[test]
    fn the_tools_stay_declared_and_the_mode_narrows_near_the_end() {
        let mut conversacion = Conversacion::default();
        let todas = [
            Herramienta::nueva("timeline_read", "", json!({}), Clase::Lectura, NUCLEO),
            Herramienta::nueva("clips_split", "", json!({}), Clase::Edicion, NUCLEO),
        ];
        let guardia = Guardia::new(Limites {
            pasos: 5,
            tiempo: std::time::Duration::from_secs(300),
        });
        let primero = preparar(&mut conversacion, &todas, &guardia, 0);
        assert_eq!((primero.herramientas.len(), primero.modo), (2, Modo::Libre));

        let casi = preparar(&mut conversacion, &todas, &guardia, 2);
        assert_eq!(casi.herramientas.len(), 2);
        assert_eq!(casi.modo, Modo::SoloLeer(vec!["timeline_read".into()]));

        let ultimo = preparar(&mut conversacion, &todas, &guardia, 4);
        assert_eq!((ultimo.herramientas.len(), ultimo.modo), (2, Modo::Ninguna));
    }
}
