// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! A montage's plan asked for, once its material is analysed and
//! described: the planner Settings and the montage's settings choose, the
//! keys from the system's credential store (see `montaje::plan`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use analisis::trabajo::{self, Archivo};
use montaje::avance::{Avance, Parada};
use montaje::ia::Ia;
use montaje::plan::{self, Encargo, Entorno, Resultado};
use montaje::{Datos, Pedido, estado};
use serde_json::{Map, json};

use crate::i18n::{t, tf};

/// Plans the job of `pedido` with the providers' `claves`, telling
/// `avisar` how it goes. Returns its folder once `ia/plan.json` is there.
/// A failure is said in a sentence; a cancelled run, empty.
pub(crate) fn planificar(
    datos: &Datos,
    claves: HashMap<String, String>,
    pedido: &Pedido,
    ia: &Ia,
    parada: &Parada,
    avisar: impl FnMut(Avance),
) -> Result<PathBuf, String> {
    let carpeta = datos.trabajos().join(&pedido.id);
    analizado(&carpeta)?;
    anotar(&carpeta, pedido).map_err(|e| e.to_string())?;
    let reglas = pedido.reglas();
    let encargo = Encargo {
        trabajo: &carpeta,
        plantilla: &pedido.receta.id,
        reglas: &reglas,
        entorno: Entorno {
            config: datos.ajustes(ia),
            claves,
            cuota: datos.carpeta_cache(),
            parada: parada.bandera(),
        },
    };
    match plan::preparar(&encargo, avisar) {
        Ok(Resultado::Plan { cerebro, .. }) => {
            log::info!("montaje: planned by {cerebro}");
            Ok(carpeta)
        }
        Ok(Resultado::Esperando(pedido)) => Err(tf("montaje.planWaiting", &[&pedido.display()])),
        Err(_) if parada.cancelada() => Err(String::new()),
        Err(fallo) => {
            log::warn!("montaje: no plan: {fallo}");
            Err(fallo.to_string())
        }
    }
}

/// Some video the job lists is in its analysis: the plan is made with the
/// ones that are, and the rest are logged.
fn analizado(carpeta: &Path) -> Result<(), String> {
    let (Ok(videos), Ok(archivo)) = (trabajo::videos(carpeta), Archivo::abrir(carpeta)) else {
        return Err(t("montaje.notAnalysed"));
    };
    let pendientes = archivo.pendientes(&videos);
    if videos.is_empty() || pendientes.len() == videos.len() {
        return Err(t("montaje.notAnalysed"));
    }
    for video in pendientes {
        log::warn!("montaje: {} was not analysed and is left out of the plan", video.id);
    }
    Ok(())
}

/// What was asked for the job, in its `estado.json`: the template,
/// language and material, the brand kit and planner.
fn anotar(carpeta: &Path, pedido: &Pedido) -> std::io::Result<()> {
    let mut extra = Map::new();
    extra.insert("plantilla".into(), json!(pedido.receta.id));
    extra.insert("idioma".into(), json!(pedido.idioma));
    extra.insert("entrada".into(), json!(pedido.material.display().to_string()));
    if let Some(marca) = &pedido.marca {
        extra.insert("marca".into(), json!(marca));
    }
    if let Some(cerebro) = &pedido.cerebro {
        extra.insert("cerebro".into(), json!(cerebro));
    }
    estado::guardar(carpeta, "analizando", extra)
}
