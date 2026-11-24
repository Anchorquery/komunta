// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The montage as the in-window agent sees and starts it: the Montaje
//! pane's own sheet, so a montage the agent asks for is the one the person
//! would have asked for with the same choices. The tools that call this are
//! [`crate::panes::asistente_montaje`]'s.

use montaje::Encargo;
use serde_json::{Value, json};

use super::{MontajePane, open_project};
use crate::studio::Studio;

/// What the agent changes on the sheet before a montage; what it leaves
/// out stays as the project's last montage had it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cambios {
    /// The folder with the material.
    pub material: Option<String>,
    /// "¿Qué quieres?".
    pub peticion: Option<String>,
    /// The kind of video, by id; "automatica" lets the AI choose.
    pub receta: Option<String>,
    /// The brand kit, by id; empty for none.
    pub marca: Option<String>,
    /// "es", "eu" or "auto".
    pub idioma: Option<String>,
}

impl MontajePane {
    /// Whether the open project is a montage and how it was asked for, and
    /// the run under way, if any.
    pub(crate) fn estado_para_asistente(&self, studio: &Studio) -> Value {
        let encargo = open_project(studio).and_then(|p| Encargo::leer(&p));
        let hecho = encargo.map(|e| {
            json!({
                "receta": e.receta.id,
                "pedida": e.pedida,
                "peticion": e.peticion,
                "material": e.material.display().to_string(),
                "marca": e.marca,
                "formatos": e.formatos,
                "veces_montado": e.version,
            })
        });
        let corrida = self.busy.then(|| {
            json!({"fase": self.phase, "progreso": self.progress, "mensaje": self.message})
        });
        json!({
            "montaje": hecho,
            "en_marcha": corrida,
            "ultimo_error": (!self.error.is_empty()).then_some(&self.error),
        })
    }

    /// The kinds of video and the brand kits there are to choose from.
    pub(crate) fn opciones_para_asistente(&mut self, studio: &Studio) -> Value {
        self.refresh(studio);
        let recetas: Vec<Value> = self
            .recipes
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "nombre": r.nombre,
                    "descripcion": r.descripcion,
                    "formato": r.formato,
                    "duracion": r.duracion,
                })
            })
            .collect();
        json!({"recetas": recetas, "marcas": self.brands})
    }

    /// A montage of the open project asked for with `cambios`, to start once
    /// the agent's turn is over: laying it out opens the project again,
    /// which a turn under way cannot live through. The sheet is checked
    /// now, so a missing folder or model is told to the agent.
    pub(crate) fn pedir_montaje(&mut self, studio: &Studio, cambios: Cambios) -> Result<Value, String> {
        if self.busy {
            return Err("ya hay un montaje en marcha: mira montaje_estado".into());
        }
        if open_project(studio).is_none() {
            return Err("no hay ningún proyecto abierto".into());
        }
        self.open_sheet(studio);
        self.open = false;
        let hecho = self.cambiar(cambios);
        hecho.and_then(|()| self.ready(studio))?;
        self.correr_al_acabar = true;
        Ok(json!({
            "pendiente": true,
            "receta": self.recipes.get(self.recipe).map(|r| r.nombre.clone()),
            "peticion": self.request,
            "material": self.material,
            "marca": self.brand.checked_sub(1).and_then(|i| self.brands.get(i)),
            "nota": "El montaje empieza en cuanto acabe este turno y tarda unos minutos; \
                     el panel Montaje enseña cómo va. Dilo a la persona.",
        }))
    }

    fn cambiar(&mut self, cambios: Cambios) -> Result<(), String> {
        if let Some(material) = cambios.material {
            self.set_material(material);
        }
        if let Some(peticion) = cambios.peticion {
            self.request = peticion;
        }
        if let Some(id) = cambios.receta {
            let ids: Vec<&str> = self.recipes.iter().map(|r| r.id.as_str()).collect();
            self.recipe = ids
                .iter()
                .position(|r| *r == id)
                .ok_or_else(|| format!("no hay receta «{id}»: {}", ids.join(", ")))?;
            self.from_recipe();
        }
        if let Some(marca) = cambios.marca {
            self.brand = if marca.is_empty() {
                0
            } else {
                1 + self
                    .brands
                    .iter()
                    .position(|b| *b == marca)
                    .ok_or_else(|| format!("no hay kit de marca «{marca}»: {}", self.brands.join(", ")))?
            };
        }
        if let Some(idioma) = cambios.idioma {
            self.language = match idioma.as_str() {
                "es" => 1,
                "eu" => 2,
                "auto" | "" => 0,
                otro => return Err(format!("«{otro}» no es un idioma del montaje: es, eu o auto")),
            };
        }
        Ok(())
    }

    /// Whether the agent asked for a montage; asked once.
    pub(crate) fn tomar_correr(&mut self) -> bool {
        std::mem::take(&mut self.correr_al_acabar)
    }

    /// Stops the montage under way.
    pub(crate) fn cancelar_para_asistente(&mut self) -> Result<Value, String> {
        if !self.busy {
            return Err("no hay ningún montaje en marcha".into());
        }
        self.parada.cancelar();
        Ok(json!({"cancelado": true}))
    }
}
