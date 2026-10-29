//! Settings › Montaje e IA, the AI half: who plans and who takes over,
//! each provider's key - kept in the system's credential store, proved
//! with a call to the provider - and model, who describes the clips, and
//! the whisper models the analysis hears Spanish and Basque takes with.
//!
//! The engine's folder, Python and recipes stay with the montage pane;
//! this is the part that grew past a couple of fields.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use montaje::ia::{self, Ia};
use montaje::llavero::{self, Llavero, LlaveroSistema};
use montaje::proveedores::Proveedor;
use slint::{ModelRc, SharedString, VecModel};

use crate::i18n::{t, t_or, tf};
use crate::panes::Msg;
use crate::panes::montaje::MontajeMsg;
use crate::panes::settings::{SettingsMsg, model_key};
use crate::studio::Studio;
use crate::ui::{MontajeSettingsData, ProveedorIaData};

/// Settings' Transcriber page, where whisper models are downloaded.
const TRANSCRIBER_PAGE: i32 = 2;

/// The planner when it fails: by default the engine's, these.
const RESPALDO_DEL_MOTOR: [&str; 2] = ["anthropic", "openai"];

/// Claude's effort when none is chosen: the engine's.
const ESFUERZO_DEL_MOTOR: &str = "high";

/// Where a provider's key comes from, when it has one.
#[derive(Clone, Debug, PartialEq)]
enum Origen {
    /// Komunta's own, in the credential store.
    Llavero(String),
    /// The engine's `.env`, where the old web settings put it.
    EnvMotor(String),
    /// The system's environment.
    Sistema(String),
}

impl Origen {
    fn clave(&self) -> &str {
        match self {
            Origen::Llavero(clave) | Origen::EnvMotor(clave) | Origen::Sistema(clave) => clave,
        }
    }
}

/// A provider's page state that is not a setting.
#[derive(Default)]
struct Estado {
    /// The key being typed, not yet kept.
    borrador: String,
    probando: bool,
    /// The last proof: its models, or why the key failed.
    prueba: Option<Result<Vec<String>, String>>,
}

/// The page's state between publishes.
#[derive(Default)]
pub struct AjustesIa {
    estados: HashMap<Proveedor, Estado>,
    /// Where each provider's key is, read once and again after a change:
    /// the credential store is not read on every publish.
    origenes: RefCell<Option<HashMap<Proveedor, Origen>>>,
}

impl AjustesIa {
    /// A field typed into: a provider's key, as `key:<provider>`.
    pub fn editado(&mut self, campo: &str, texto: String) {
        if let Some(proveedor) = proveedor_de(campo, "key") {
            self.estado(proveedor).borrador = texto;
        }
    }

    /// A choice made, by field and index into the options published.
    pub fn elegido(&mut self, campo: &str, indice: i32, studio: &mut Studio) {
        let indice = indice.max(0) as usize;
        // The planner's and the fallbacks' lists open with "the engine's" or "none".
        let de_lista = |lista: &[&str]| {
            indice
                .checked_sub(1)
                .and_then(|i| lista.get(i))
                .map(|c| (*c).to_owned())
        };
        let whispers = whisper_ids(studio);
        let modelos =
            proveedor_de(campo, "model").map(|p| (p, self.modelos(p, &studio.prefs.montaje.ia)));
        let elegida = &mut studio.prefs.montaje.ia;
        match campo {
            "brain" => elegida.cerebro = de_lista(&ia::CEREBROS),
            "fallback-1" | "fallback-2" => {
                let actual = respaldo_actual(elegida);
                let mut dos = [actual.first().cloned(), actual.get(1).cloned()];
                dos[usize::from(campo == "fallback-2")] = de_lista(&respaldos());
                let mut respaldo: Vec<String> = dos.into_iter().flatten().collect();
                respaldo.dedup();
                elegida.respaldo = Some(respaldo);
            }
            "effort" => elegida.esfuerzo = ia::ESFUERZOS.get(indice).map(|e| (*e).to_owned()),
            "describer" => {
                elegida.descripcion = ia::DESCRIPCIONES.get(indice).map(|d| (*d).to_owned())
            }
            "whisper-es" => elegida.transcripcion_es = whispers.get(indice).cloned(),
            "whisper-eu" => elegida.transcripcion_eu = whispers.get(indice).cloned(),
            _ => {
                let Some((proveedor, modelos)) = modelos else {
                    return;
                };
                if let Some(modelo) = modelos.get(indice) {
                    elegida
                        .modelos
                        .insert(proveedor.id().to_owned(), modelo.clone());
                }
            }
        }
        studio.prefs.save(&studio.host.dirs);
    }

    /// A button: `save-key:`, `test:`, `import-key:` or `forget-key:` and a
    /// provider, or `open-transcriber`.
    pub fn accion(&mut self, nombre: &str, studio: &mut Studio) {
        if nombre == "open-transcriber" {
            studio.handle(Msg::Settings(SettingsMsg::PageChanged(TRANSCRIBER_PAGE)));
            return;
        }
        let Some((verbo, proveedor)) = nombre.split_once(':') else {
            return;
        };
        let Some(proveedor) = Proveedor::desde_id(proveedor) else {
            return;
        };
        let resultado = match verbo {
            "save-key" => self.guardar(proveedor, studio),
            "import-key" => self.importar(proveedor, studio),
            "forget-key" => LlaveroSistema
                .borrar(proveedor)
                .map(|()| self.estado(proveedor).prueba = None),
            "test" => {
                self.probar(proveedor, studio);
                Ok(())
            }
            _ => Ok(()),
        };
        self.origenes.replace(None);
        if let Err(error) = resultado {
            self.estado(proveedor).prueba = Some(Err(error));
        }
    }

    /// A proof came back.
    pub fn probado(&mut self, proveedor: Proveedor, resultado: Result<Vec<String>, String>) {
        let estado = self.estado(proveedor);
        estado.probando = false;
        estado.prueba = Some(resultado);
    }

    /// Keeps the typed key and proves it.
    fn guardar(&mut self, proveedor: Proveedor, studio: &mut Studio) -> Result<(), String> {
        let clave = llavero::limpiar(&self.estado(proveedor).borrador)?;
        LlaveroSistema.guardar(proveedor, &clave)?;
        self.estado(proveedor).borrador.clear();
        self.origenes.replace(None);
        self.probar(proveedor, studio);
        Ok(())
    }

    /// Moves the key the engine's `.env` holds into the credential store.
    fn importar(&mut self, proveedor: Proveedor, studio: &mut Studio) -> Result<(), String> {
        let carpeta = crate::panes::montaje::motor(studio).carpeta;
        let clave = clave_en_env(&carpeta.join(".env"), proveedor.variable())
            .ok_or_else(|| t("montaje.keyNotInEnv"))?;
        LlaveroSistema.guardar(proveedor, &llavero::limpiar(&clave)?)?;
        self.origenes.replace(None);
        self.probar(proveedor, studio);
        Ok(())
    }

    /// Asks the provider for its models with whichever key it has, on a
    /// worker; the answer comes back as [`MontajeMsg::Tested`].
    fn probar(&mut self, proveedor: Proveedor, studio: &Studio) {
        let Some(clave) = self
            .origen(proveedor, studio)
            .map(|origen| origen.clave().to_owned())
        else {
            self.estado(proveedor).prueba = Some(Err(t("montaje.noKey")));
            return;
        };
        self.estado(proveedor).probando = true;
        crate::host::spawn(
            move || proveedor.probar(&clave),
            move |studio, _, _, resultado| {
                studio.handle(Msg::Montaje(MontajeMsg::Tested(proveedor, resultado)));
            },
        );
    }

    fn estado(&mut self, proveedor: Proveedor) -> &mut Estado {
        self.estados.entry(proveedor).or_default()
    }

    /// Where `proveedor`'s key is: the credential store first, as the
    /// engine sees it, then its `.env`, then the system.
    fn origen(&self, proveedor: Proveedor, studio: &Studio) -> Option<Origen> {
        let mut origenes = self.origenes.borrow_mut();
        let origenes = origenes.get_or_insert_with(|| leer_origenes(studio));
        origenes.get(&proveedor).cloned()
    }

    /// The models to offer for `proveedor`: its own list once proved, the
    /// recommended ones before; the chosen one always among them.
    fn modelos(&self, proveedor: Proveedor, ia: &Ia) -> Vec<String> {
        let probados = match self.estados.get(&proveedor).and_then(|e| e.prueba.as_ref()) {
            Some(Ok(modelos)) if !modelos.is_empty() => modelos.clone(),
            _ => proveedor
                .recomendados()
                .iter()
                .map(|r| r.id.to_owned())
                .collect(),
        };
        let elegido = ia.modelo(proveedor).to_owned();
        if probados.contains(&elegido) {
            probados
        } else {
            std::iter::once(elegido).chain(probados).collect()
        }
    }

    /// The AI half of the page as the window draws it, into `datos`.
    pub fn publicar(&self, studio: &Studio, datos: &mut MontajeSettingsData) {
        let prefs = &studio.prefs.montaje;
        let ia = &prefs.ia;
        let cerebro = ia.cerebro.as_deref().unwrap_or("");
        datos.brain = ia::CEREBROS
            .iter()
            .position(|c| *c == cerebro)
            .map_or(0, |i| i as i32 + 1);
        datos.brains = strings(
            std::iter::once(t("montaje.brainEngineChoice"))
                .chain(ia::CEREBROS.iter().map(|c| nombre_cerebro(c))),
        );
        let respaldo = respaldo_actual(ia);
        let posicion = |n: usize| {
            respaldo
                .get(n)
                .and_then(|r| respaldos().iter().position(|c| c == r))
                .map_or(0, |i| i as i32 + 1)
        };
        datos.fallback_1 = posicion(0);
        datos.fallback_2 = posicion(1);
        datos.fallbacks = strings(
            std::iter::once(t("montaje.fallbackNone"))
                .chain(respaldos().iter().map(|c| nombre_cerebro(c))),
        );
        let esfuerzo = ia.esfuerzo.as_deref().unwrap_or(ESFUERZO_DEL_MOTOR);
        datos.effort = ia::ESFUERZOS
            .iter()
            .position(|e| *e == esfuerzo)
            .unwrap_or(2) as i32;
        datos.efforts = strings(
            ia::ESFUERZOS
                .iter()
                .map(|e| t(&format!("montaje.effort.{e}"))),
        );
        let describe = ia.descripcion.as_deref().unwrap_or("auto");
        datos.describer = ia::DESCRIPCIONES
            .iter()
            .position(|d| *d == describe)
            .unwrap_or(0) as i32;
        datos.describers = strings(
            ia::DESCRIPCIONES
                .iter()
                .map(|d| t(&format!("montaje.describer.{d}"))),
        );
        datos.providers = ModelRc::new(VecModel::from(
            Proveedor::TODOS
                .iter()
                .map(|p| self.proveedor(*p, studio))
                .collect::<Vec<_>>(),
        ));
        self.publicar_whisper(studio, datos);
    }

    fn proveedor(&self, proveedor: Proveedor, studio: &Studio) -> ProveedorIaData {
        let estado = self.estados.get(&proveedor);
        let origen = self.origen(proveedor, studio);
        let modelos = self.modelos(proveedor, &studio.prefs.montaje.ia);
        let elegido = studio.prefs.montaje.ia.modelo(proveedor);
        let (state, status) = match (
            estado.is_some_and(|e| e.probando),
            estado.and_then(|e| e.prueba.as_ref()),
        ) {
            (true, _) => (1, t("montaje.testing")),
            (false, Some(Ok(modelos))) => (2, tf("montaje.keyWorks", &[&modelos.len()])),
            (false, Some(Err(error))) => (3, error.clone()),
            (false, None) => (0, describir_origen(origen.as_ref())),
        };
        let nota = proveedor
            .recomendados()
            .iter()
            .find(|r| r.id == elegido)
            .map(|r| r.nota)
            .unwrap_or_default();
        ProveedorIaData {
            id: proveedor.id().into(),
            name: proveedor.nombre().into(),
            kept: matches!(origen, Some(Origen::Llavero(_))),
            in_engine_env: matches!(origen, Some(Origen::EnvMotor(_))),
            has_key: origen.is_some(),
            draft: estado
                .map(|e| e.borrador.as_str())
                .unwrap_or_default()
                .into(),
            state,
            status: status.into(),
            model: modelos.iter().position(|m| m == elegido).unwrap_or(0) as i32,
            models: strings(modelos.iter().cloned()),
            note: nota.into(),
        }
    }

    fn publicar_whisper(&self, studio: &Studio, datos: &mut MontajeSettingsData) {
        let modelos = whispers(studio);
        let elegida = &studio.prefs.montaje.ia;
        let posicion = |id: Option<&String>, defecto: &str| {
            let id = id.map_or(defecto, String::as_str);
            modelos.iter().position(|m| m.id == id).unwrap_or(0)
        };
        let es = posicion(elegida.transcripcion_es.as_ref(), ia::WHISPER_ES);
        let eu = posicion(elegida.transcripcion_eu.as_ref(), ia::WHISPER_EU);
        datos.whisper_es = es as i32;
        datos.whisper_eu = eu as i32;
        datos.whispers = strings(modelos.iter().map(|m| {
            let nombre = t_or(&model_key("transcriber.model", &m.id), &m.label);
            if m.downloaded {
                nombre
            } else {
                tf("montaje.notDownloaded", &[&nombre])
            }
        }));
        let faltan = [es, eu]
            .iter()
            .any(|&i| modelos.get(i).is_some_and(|m| !m.downloaded));
        datos.whisper_missing = faltan;
    }
}

/// The whisper models the page offers, by id, in the order it lists them:
/// those that hear more than English and the engine knows too.
fn whisper_ids(studio: &Studio) -> Vec<String> {
    whispers(studio).into_iter().map(|m| m.id).collect()
}

fn whispers(studio: &Studio) -> Vec<concat_speech::transcribe::ModelStatus> {
    concat_speech::Transcriber::status(&studio.host.dirs)
        .map(|estado| {
            estado
                .models
                .into_iter()
                .filter(|m| !m.english_only && ia::whisper_del_motor(&m.id).is_some())
                .collect()
        })
        .unwrap_or_default()
}

/// Who may take over from the planner: every AI but the manual one.
fn respaldos() -> Vec<&'static str> {
    ia::CEREBROS
        .iter()
        .copied()
        .filter(|c| *c != "manual")
        .collect()
}

/// The fallbacks in force: chosen, or the engine's.
fn respaldo_actual(ia: &Ia) -> Vec<String> {
    ia.respaldo
        .clone()
        .unwrap_or_else(|| RESPALDO_DEL_MOTOR.iter().map(|r| (*r).to_owned()).collect())
}

/// `campo` as `<prefijo>:<provider>`.
fn proveedor_de(campo: &str, prefijo: &str) -> Option<Proveedor> {
    let (antes, id) = campo.split_once(':')?;
    (antes == prefijo)
        .then(|| Proveedor::desde_id(id))
        .flatten()
}

/// Where every provider's key is now.
fn leer_origenes(studio: &Studio) -> HashMap<Proveedor, Origen> {
    let env_motor = crate::panes::montaje::motor(studio).carpeta.join(".env");
    Proveedor::TODOS
        .into_iter()
        .filter_map(|proveedor| {
            let origen = LlaveroSistema
                .leer(proveedor)
                .map(Origen::Llavero)
                .or_else(|| clave_en_env(&env_motor, proveedor.variable()).map(Origen::EnvMotor))
                .or_else(|| {
                    std::env::var(proveedor.variable())
                        .ok()
                        .filter(|v| !v.is_empty())
                        .map(Origen::Sistema)
                })?;
            Some((proveedor, origen))
        })
        .collect()
}

/// `variable`'s value in a `.env` file, read the way the engine reads it.
fn clave_en_env(ruta: &Path, variable: &str) -> Option<String> {
    let texto = std::fs::read_to_string(ruta).ok()?;
    texto.lines().find_map(|linea| {
        let (clave, valor) = linea.trim().split_once('=')?;
        (clave.trim() == variable)
            .then(|| valor.trim().trim_matches(['"', '\'']).to_owned())
            .filter(|v| !v.is_empty())
    })
}

fn describir_origen(origen: Option<&Origen>) -> String {
    match origen {
        Some(Origen::Llavero(clave)) => tf("montaje.keyKept", &[&llavero::final_visible(clave)]),
        Some(Origen::EnvMotor(clave)) => tf("montaje.keyInEnv", &[&llavero::final_visible(clave)]),
        Some(Origen::Sistema(clave)) => {
            tf("montaje.keyInSystem", &[&llavero::final_visible(clave)])
        }
        None => t("montaje.noKey"),
    }
}

/// An AI as the lists name it.
pub(crate) fn nombre_cerebro(id: &str) -> String {
    t(&format!("montaje.brainName.{}", id.replace('_', "")))
}

fn strings(items: impl Iterator<Item = impl Into<SharedString>>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items.map(Into::into).collect::<Vec<SharedString>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_name_their_provider() {
        assert_eq!(
            proveedor_de("key:anthropic", "key"),
            Some(Proveedor::Anthropic)
        );
        assert_eq!(
            proveedor_de("model:google", "model"),
            Some(Proveedor::Google)
        );
        assert_eq!(proveedor_de("model:google", "key"), None);
        assert_eq!(proveedor_de("key:nadie", "key"), None);
    }

    #[test]
    fn a_key_is_read_from_the_engines_env_as_the_engine_reads_it() {
        let carpeta = std::env::temp_dir().join(format!("komunta-env-{}", std::process::id()));
        std::fs::create_dir_all(&carpeta).unwrap();
        let ruta = carpeta.join(".env");
        std::fs::write(
            &ruta,
            "# claves\nOPENAI_API_KEY=\n ANTHROPIC_API_KEY = \"sk-ant-1234567890abcd\"\n",
        )
        .unwrap();
        assert_eq!(
            clave_en_env(&ruta, "ANTHROPIC_API_KEY").as_deref(),
            Some("sk-ant-1234567890abcd")
        );
        assert_eq!(
            clave_en_env(&ruta, "OPENAI_API_KEY"),
            None,
            "an empty value is no key"
        );
        assert_eq!(clave_en_env(&ruta, "GEMINI_API_KEY"), None);
        let _ = std::fs::remove_dir_all(&carpeta);
    }

    #[test]
    fn the_planner_never_falls_back_to_manual() {
        assert!(!respaldos().contains(&"manual"));
        assert_eq!(respaldo_actual(&Ia::default()), ["anthropic", "openai"]);
    }
}
