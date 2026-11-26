//! A montage's material described, after its analysis and before the
//! plan: Gemini watching a contact video, or Claude or a command-line AI
//! (Codex, Gemini CLI, Claude Code: the sign-in, no key) looking at strips
//! of frames, as Settings and the montage's settings choose (see
//! `montaje::descripcion`), with the keys kept in the system's credential
//! store. What is described is written into the job's `analisis.json` and
//! the clip cache.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use analisis::cache::Cache;
use analisis::descripcion::cuota::Cuota;
use analisis::descripcion::describir::{self, Encargo, Proveedor};
use montaje::descripcion::{Descriptor, descriptores, mirador};
use montaje::plan::Entorno;
use montaje::plan::clis::MiradorCli;
use montaje::ia::Ia;
use montaje::llavero::LlaveroSistema;
use montaje::Datos;
use montaje::avance::Avance;

/// Describes what the job in `trabajo` lacks, telling `avisar` what it
/// does; `env_viejo` is the old engine's `.env`, for keys not brought over
/// yet, and `parada` stops a command-line AI looking. Nothing here stops
/// the montage: the planner does without what is not described, and a
/// failure is logged.
pub(crate) fn describir(
    datos: &Datos,
    env_viejo: &Path,
    ia: &Ia,
    trabajo: &Path,
    parada: Arc<AtomicBool>,
    mut avisar: impl FnMut(Avance),
) {
    let ajustes = datos.ajustes(ia);
    let cli = mirador(ia, &ajustes).and_then(|modo| {
        let entorno = Entorno {
            config: ajustes.clone(),
            parada,
            ..Entorno::default()
        };
        MiradorCli::new(modo, trabajo, &entorno)
    });
    let proveedores: Vec<Proveedor> = match cli {
        Some(cli) => vec![Proveedor::Cli(Arc::new(cli))],
        None => descriptores(ia, &ajustes, &LlaveroSistema, env_viejo)
            .into_iter()
            .map(proveedor)
            .collect(),
    };
    if proveedores.is_empty() {
        log::info!("montaje: the material is not described: no provider with a key, or turned off");
        return;
    }
    // Only kept into: a description is added to the entry under the key
    // the clip's analysis gave it, whatever these settings.
    let cache = Cache::new(datos.cache(), serde_json::Value::Null);
    let encargo = Encargo {
        trabajo,
        proveedores,
        cache: Some(&cache),
        cuota: Cuota::en(datos.carpeta_cache()),
    };
    let resultado = describir::describir(&encargo, |aviso| {
        avisar(Avance {
            fase: "analisis".into(),
            fraccion: 1.0,
            mensaje: aviso.to_owned(),
        });
    });
    match resultado {
        Ok(resumen) => log::info!(
            "montaje: {} described by {:?} in {} requests, {} not{}",
            resumen.descritos,
            resumen.proveedores,
            resumen.peticiones,
            resumen.fallos,
            resumen.error.map(|e| format!(": {e}")).unwrap_or_default()
        ),
        Err(error) => log::warn!("montaje: the description stopped: {error}"),
    }
}

fn proveedor(descriptor: Descriptor) -> Proveedor {
    match descriptor.proveedor {
        montaje::Proveedor::Google => Proveedor::Google {
            clave: descriptor.clave,
            modelos: descriptor.modelos,
            limite_diario: descriptor.limite_diario,
        },
        _ => Proveedor::Anthropic {
            clave: descriptor.clave,
            modelo: descriptor.modelos.into_iter().next().unwrap_or_default(),
        },
    }
}
