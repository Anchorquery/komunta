// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! One montage run, on a worker: the material listed and analysed
//! ([`montaje_analisis`]), described ([`montaje_descripcion`]), the plan
//! asked for ([`montaje_plan`]) and laid out by the `montaje` crate as the
//! timelines to graft into the project.

use std::path::PathBuf;

use concat_host::AppDirs;
use concat_project::Project;
use montaje::avance::{Avance, Parada};
use montaje::ia::Ia;
use montaje::llavero::{self, LlaveroSistema};
use montaje::{Datos, Encargo, Pedido};

use crate::i18n::tf;
use crate::panes::{montaje_analisis, montaje_descripcion, montaje_plan};

/// Everything a run needs from the app, taken on the UI thread.
pub(crate) struct Corrida {
    pub datos: Datos,
    /// The old engine's `.env`, for keys not brought over yet.
    pub env_viejo: PathBuf,
    pub dirs: AppDirs,
    pub ia: Ia,
    pub parada: Parada,
    /// The project folder the montage is for.
    pub proyecto: PathBuf,
    /// The project's montages so far: a redo is the next one.
    pub version: u32,
    /// Frames, the main one first.
    pub formatos: Vec<String>,
}

impl Corrida {
    /// The whole run for `pedido`, telling `avisar` how it goes; `listo`
    /// is told when the plan is in and is being laid out. A failure is
    /// said in a sentence; a stop is an empty one.
    pub fn ejecutar(
        &self,
        pedido: &Pedido,
        avisar: impl FnMut(Avance) + Copy,
        listo: impl FnOnce(),
    ) -> Result<(Encargo, Project), String> {
        self.analizar(pedido, avisar)?;
        let claves = llavero::claves(&LlaveroSistema, &self.env_viejo);
        let trabajo = montaje_plan::planificar(&self.datos, claves, pedido, &self.ia, &self.parada, avisar)?;
        listo();
        let encargo = self.encargo(pedido, trabajo);
        let nuevo = encargo
            .preparar(&self.proyecto)
            .map_err(|error| error.to_string())?;
        Ok((encargo, nuevo))
    }

    /// The material listed, its clips analysed and described.
    fn analizar(&self, pedido: &Pedido, mut avisar: impl FnMut(Avance) + Copy) -> Result<(), String> {
        let trabajo = montaje_analisis::Trabajo {
            carpeta: self.datos.trabajos().join(&pedido.id),
            idioma: pedido.idioma.clone(),
            cache: self.datos.cache(),
            musica_kit: pedido.marca.as_deref().and_then(|marca| self.datos.musica(marca)),
        };
        montaje_analisis::inventariar(&pedido.material, pedido.guion.as_deref(), &trabajo.carpeta, &mut avisar)
            .map_err(|error| tf("montaje.listingFailed", &[&error.to_string()]))?;
        self.seguir()?;
        montaje_analisis::analizar(&self.dirs, &self.ia, &trabajo, &self.parada, avisar)?;
        self.seguir()?;
        montaje_descripcion::describir(&self.datos, &self.env_viejo, &self.ia, &trabajo.carpeta, avisar);
        self.seguir()
    }

    /// An empty error once the run is stopped.
    fn seguir(&self) -> Result<(), String> {
        if self.parada.cancelada() {
            Err(String::new())
        } else {
            Ok(())
        }
    }

    /// The project's link to the job planned in `trabajo`.
    fn encargo(&self, pedido: &Pedido, trabajo: PathBuf) -> Encargo {
        Encargo {
            receta: pedido.receta_final(&trabajo),
            trabajo,
            datos: self.datos.carpeta.clone(),
            marca: pedido.marca.clone(),
            formatos: self.formatos.clone(),
            version: self.version,
            material: pedido.material.clone(),
            pedida: pedido.receta.id.clone(),
            peticion: pedido.peticion.clone(),
        }
    }
}
