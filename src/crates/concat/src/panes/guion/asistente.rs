// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The script as the in-window agent reads and edits it: the pane's own
//! plan in memory, so an edit shows in the pane at once and stays unapplied
//! until the person or the agent applies it, the same as one made by hand.
//! The tools that call this are [`crate::panes::asistente_guion`]'s.

use std::path::PathBuf;

use montaje::guion::{Bloque, Tipo};
use montaje::versiones;
use serde_json::{Value, json};

use super::GuionPane;

/// The words of a take shown when the whole script is read, before "…".
const PALABRAS_RESUMEN: usize = 12;

/// One edit the agent makes to the script.
#[derive(Clone, Debug, PartialEq)]
pub enum Operacion {
    /// Words `desde..=hasta` of take `toma` cut out, or put back.
    Palabras {
        toma: usize,
        desde: usize,
        hasta: usize,
        quitar: bool,
    },
    QuitarToma(usize),
    /// A take one place earlier (-1) or later (1).
    MoverToma { toma: usize, hacia: i32 },
    CambiarSubtitulo { k: usize, texto: String },
    QuitarSubtitulo(usize),
    QuitarRecurso(usize),
    /// Every edit not applied yet thrown away.
    Descartar,
}

impl GuionPane {
    /// The script for the agent: every take in short, or take `toma` with
    /// all its words.
    pub(crate) fn para_asistente(&self, toma: Option<usize>) -> Result<Value, String> {
        self.plan.as_ref().ok_or_else(sin_guion)?;
        let tomas: Vec<Value> = match toma {
            Some(n) => {
                let bloque = self.bloque(n)?;
                vec![bloque_json(bloque, usize::MAX)]
            }
            None => self
                .guion
                .bloques
                .iter()
                .map(|b| bloque_json(b, PALABRAS_RESUMEN))
                .collect(),
        };
        let avisos: Vec<Value> = self
            .avisos
            .iter()
            .map(|a| json!({"texto": a.texto, "como_arreglarlo": a.instruccion}))
            .collect();
        Ok(json!({
            "tomas": tomas,
            "duracion": redondo(self.guion.duracion()),
            "avisos": avisos,
            "sin_aplicar": self.dirty,
        }))
    }

    /// Applies `operacion` to the plan in memory, after checking that what
    /// it names is there: the plan itself would quietly do nothing.
    pub(crate) fn editar_para_asistente(&mut self, operacion: Operacion) -> Result<Value, String> {
        if self.busy {
            return Err("el guion se está aplicando al vídeo: espera a que termine".into());
        }
        self.comprobar(&operacion)?;
        if operacion == Operacion::Descartar {
            let proyecto = self.proyecto.clone().ok_or_else(sin_guion)?;
            self.load(&proyecto);
            return Ok(self.resumen());
        }
        let plan = self.plan.as_mut().ok_or_else(sin_guion)?;
        match operacion {
            Operacion::Palabras {
                toma,
                desde,
                hasta,
                quitar,
            } => plan.marcar_palabras(toma, desde..=hasta, quitar),
            Operacion::QuitarToma(toma) => plan.quitar_toma(toma),
            Operacion::MoverToma { toma, hacia } => plan.mover_toma(toma, hacia),
            Operacion::CambiarSubtitulo { k, texto } => plan.cambiar_subtitulo(k, &texto),
            Operacion::QuitarSubtitulo(k) => plan.quitar_subtitulo(k),
            Operacion::QuitarRecurso(k) => plan.quitar_recurso(k),
            Operacion::Descartar => {}
        }
        self.status.clear();
        self.rebuild();
        Ok(self.resumen())
    }

    /// "Aplicar al vídeo" asked for once the agent's turn is over: the
    /// project is opened again on the way, which a turn under way cannot
    /// live through.
    pub(crate) fn pedir_aplicar(&mut self) -> Result<Value, String> {
        self.plan.as_ref().ok_or_else(sin_guion)?;
        if self.busy {
            return Err("el guion ya se está aplicando al vídeo".into());
        }
        self.aplicar_al_acabar = true;
        Ok(json!({
            "pendiente": true,
            "nota": "Se aplicará al vídeo en cuanto acabe este turno: no hagas más cambios en la línea de tiempo \
                     y di a la persona que aparecerán líneas de tiempo nuevas."
        }))
    }

    /// Whether the agent asked for the script to be applied; asked once.
    pub(crate) fn tomar_aplicar(&mut self) -> bool {
        std::mem::take(&mut self.aplicar_al_acabar)
    }

    /// The plan's versions, newest last.
    pub(crate) fn versiones_para_asistente(&self) -> Result<Value, String> {
        let trabajo = self.trabajo().ok_or_else(sin_guion)?;
        let versiones: Vec<Value> = versiones::versiones(&trabajo)
            .into_iter()
            .map(|v| json!({"n": v.n, "origen": v.origen, "nota": v.nota, "fecha": v.fecha}))
            .collect();
        Ok(json!({"versiones": versiones, "sin_aplicar": self.dirty}))
    }

    /// Brings version `n` of the plan back, the plan on disk kept as a
    /// version first so this can be gone back on too; edits not applied
    /// are thrown away.
    pub(crate) fn volver_para_asistente(&mut self, n: u32) -> Result<Value, String> {
        let (Some(trabajo), Some(proyecto)) = (self.trabajo(), self.proyecto.clone()) else {
            return Err(sin_guion());
        };
        if self.busy {
            return Err("el guion se está aplicando al vídeo: espera a que termine".into());
        }
        let nota = format!("Antes de volver a la versión {n}");
        versiones::guardar(&trabajo, "komunta", Some(&nota), &[]).map_err(|e| e.to_string())?;
        versiones::restaurar(&trabajo, n).map_err(|e| e.to_string())?;
        self.load(&proyecto);
        Ok(self.resumen())
    }

    /// The job folder of the montage, its plan and its versions.
    pub(crate) fn trabajo(&self) -> Option<PathBuf> {
        self.plan.as_ref()?;
        self.encargo.as_ref().map(|e| e.trabajo.clone())
    }

    /// The take `n` of the script, or why there is none.
    fn bloque(&self, n: usize) -> Result<&Bloque, String> {
        let total = self.guion.bloques.len();
        self.guion
            .bloques
            .get(n)
            .ok_or_else(|| format!("no hay toma {n}: el guion tiene {total}, de 0 a {}", total.saturating_sub(1)))
    }

    fn comprobar(&self, operacion: &Operacion) -> Result<(), String> {
        let plan = self.plan.as_ref().ok_or_else(sin_guion)?;
        let cuantos = |clave: &str| plan.0[clave].as_array().map_or(0, Vec::len);
        match operacion {
            Operacion::Palabras {
                toma, desde, hasta, ..
            } => {
                let bloque = self.bloque(*toma)?;
                let (Some(primera), Some(ultima)) = (bloque.palabras.first(), bloque.palabras.last()) else {
                    return Err(format!("la toma {toma} no tiene palabras: es un plano sin voz"));
                };
                if desde > hasta || *desde < primera.i || *hasta > ultima.i {
                    return Err(format!(
                        "las palabras de la toma {toma} van de la {} a la {}: lee guion_leer con esa toma",
                        primera.i, ultima.i
                    ));
                }
            }
            Operacion::QuitarToma(toma) => {
                self.bloque(*toma)?;
            }
            Operacion::MoverToma { toma, hacia } => {
                self.bloque(*toma)?;
                let destino = toma.checked_add_signed(*hacia as isize);
                if !matches!(hacia, -1 | 1) || destino.is_none_or(|d| d >= self.guion.bloques.len()) {
                    return Err(format!("la toma {toma} no se puede mover {hacia}: va de una en una, -1 o 1, sin salirse"));
                }
            }
            Operacion::CambiarSubtitulo { k, .. } | Operacion::QuitarSubtitulo(k) if *k >= cuantos("subtitulos") => {
                return Err(format!("no hay subtítulo {k}: hay {}", cuantos("subtitulos")));
            }
            Operacion::QuitarRecurso(k) if *k >= cuantos("recursos") => {
                return Err(format!("no hay recurso {k}: hay {}", cuantos("recursos")));
            }
            _ => {}
        }
        Ok(())
    }

    /// The script after an edit, in a line or two.
    fn resumen(&self) -> Value {
        json!({
            "tomas": self.guion.bloques.len(),
            "duracion": redondo(self.guion.duracion()),
            "avisos": self.avisos.iter().map(|a| a.texto.as_str()).collect::<Vec<_>>(),
            "sin_aplicar": self.dirty,
        })
    }
}

fn sin_guion() -> String {
    "el proyecto abierto no tiene guion de montaje: no se hizo con IA o falta su trabajo".into()
}

/// A take for the agent, with at most `palabras` of its words: `i:texto`,
/// `~i:texto~` when it is cut out, a `°` after a filler word.
fn bloque_json(bloque: &Bloque, palabras: usize) -> Value {
    let texto: Vec<String> = bloque
        .palabras
        .iter()
        .take(palabras)
        .map(|p| {
            let marca = if p.coletilla { "°" } else { "" };
            if p.quitada {
                format!("~{}:{}{marca}~", p.i, p.txt)
            } else {
                format!("{}:{}{marca}", p.i, p.txt)
            }
        })
        .collect();
    let mut texto = texto.join(" ");
    if bloque.palabras.len() > palabras {
        texto.push_str(" …");
    }
    let tipo = match bloque.tipo {
        Tipo::Voz => "voz",
        Tipo::SoloAudio => "solo_audio",
        Tipo::Plano => "plano",
    };
    json!({
        "toma": bloque.toma,
        "clip": bloque.clip,
        "tipo": tipo,
        "duracion": redondo(bloque.duracion),
        "palabras": texto,
        "quitadas": bloque.palabras.iter().filter(|p| p.quitada).count(),
        "subtitulos": bloque.subtitulos.iter()
            .map(|s| json!({"k": s.indice, "texto": s.texto, "estilo": s.estilo}))
            .collect::<Vec<_>>(),
        "recursos": bloque.recursos.iter()
            .map(|r| json!({"k": r.indice, "clip": r.clip, "efecto": r.efecto, "garabato": r.garabato}))
            .collect::<Vec<_>>(),
    })
}

fn redondo(segundos: f64) -> f64 {
    (segundos * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use montaje::guion::PalabraGuion;

    use super::*;

    fn bloque() -> Bloque {
        let palabra = |i: usize, txt: &str, quitada: bool, coletilla: bool| PalabraGuion {
            i,
            txt: txt.into(),
            quitada,
            coletilla,
        };
        Bloque {
            toma: 2,
            clip: "a".into(),
            tipo: Tipo::Voz,
            palabras: vec![
                palabra(10, "Kaixo", false, false),
                palabra(11, "eh", true, true),
                palabra(12, "lagunak", false, false),
            ],
            duracion: 1.234,
            subtitulos: Vec::new(),
            recursos: Vec::new(),
        }
    }

    #[test]
    fn a_take_reads_as_its_words_with_the_cut_ones_struck() {
        let entera = bloque_json(&bloque(), usize::MAX);
        assert_eq!(entera["palabras"], "10:Kaixo ~11:eh°~ 12:lagunak");
        assert_eq!(entera["quitadas"], 1);
        assert_eq!(entera["duracion"], 1.23);
        assert_eq!(bloque_json(&bloque(), 2)["palabras"], "10:Kaixo ~11:eh°~ …");
    }
}
