//! What the team asked a montage for, and the name of the job that does it.

use std::path::{Path, PathBuf};

use crate::receta::Receta;

/// What the team asked for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pedido {
    /// The folder of footage, photos, audio and documents.
    pub material: PathBuf,
    /// The job's name in `trabajos/`.
    pub id: String,
    /// What kind of video it is.
    pub receta: Receta,
    /// "¿Qué quieres?", in the team's words; may be empty.
    pub peticion: String,
    /// A script that is not in the material folder.
    pub guion: Option<PathBuf>,
    /// "es" or "eu"; None lets the transcription tell.
    pub idioma: Option<String>,
    /// Brand kit id.
    pub marca: Option<String>,
    /// The planner ("claude_code", "anthropic", ...); None is the one the
    /// settings name.
    pub cerebro: Option<String>,
    /// For an automatic recipe, the ones the AI chooses among.
    pub candidatas: Vec<Receta>,
}

impl Pedido {
    /// The recipe to lay the job's plan out with: the one asked for, or,
    /// for an automatic request, the one the AI wrote in the plan (the
    /// first candidate when it named none), in the frames asked for.
    pub fn receta_final(&self, trabajo: &Path) -> Receta {
        if !self.receta.es_automatica() {
            return self.receta.clone();
        }
        let plantilla = crate::PlanCrudo::leer(trabajo)
            .and_then(|plan| plan.plan())
            .map(|plan| plan.plantilla)
            .unwrap_or_default();
        let mut receta = crate::receta::elegida(&self.candidatas, &plantilla)
            .or_else(|| self.candidatas.iter().find(|r| !r.es_automatica()))
            .cloned()
            .unwrap_or_default();
        receta.formato = self.receta.formato.clone();
        receta.formatos_extra = self.receta.formatos_extra.clone();
        receta
    }

    /// The rules the AI gets: the recipe's, then the request.
    pub fn reglas(&self) -> String {
        let mut reglas = if self.receta.es_automatica() {
            crate::receta::reglas_eleccion(&self.candidatas)
        } else {
            self.receta.reglas()
        };
        if !self.peticion.trim().is_empty() {
            reglas.push_str("\n\n## Lo que pide el equipo para este vídeo\n");
            reglas.push_str(self.peticion.trim());
            reglas.push_str("\nSi esto contradice las reglas anteriores, manda esto.");
        }
        reglas
    }
}

/// A job id from a name: letters, digits, `_` and `-`, with the date in
/// front so two jobs of the same name do not meet.
pub fn id_trabajo(nombre: &str, fecha: &str) -> String {
    let limpio: String = nombre
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let limpio = limpio.trim_matches('_');
    let limpio = if limpio.is_empty() { "montaje" } else { limpio };
    format!("{fecha}_{limpio}")
}

/// Now, in UTC, as `AAAAMMDD_HHMMSS`: the date in front of a job id.
pub fn ahora() -> String {
    chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_follows_the_recipe() {
        let pedido = Pedido {
            receta: crate::receta::de_serie().remove(0),
            peticion: "Que dure 30 segundos".into(),
            ..Pedido::default()
        };
        let reglas = pedido.reglas();
        assert!(reglas.ends_with("Si esto contradice las reglas anteriores, manda esto."));
        assert!(reglas.contains("Que dure 30 segundos"));
    }

    #[test]
    fn job_ids_are_safe_folder_names() {
        assert_eq!(id_trabajo("Andoain: acto/2", "20261028"), "20261028_Andoain__acto_2");
        assert_eq!(id_trabajo("  ", "20261028"), "20261028_montaje");
    }

    #[test]
    fn now_is_a_date_and_a_time() {
        let ahora = ahora();
        assert_eq!(ahora.len(), 15, "{ahora}");
        assert_eq!(&ahora[8..9], "_");
        assert!(ahora.replace('_', "").chars().all(|c| c.is_ascii_digit()));
    }
}
