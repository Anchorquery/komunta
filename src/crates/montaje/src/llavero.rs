//! Where the AI providers' keys are kept: the system's own credential
//! store (Windows' Credential Manager, the macOS keychain), never a file
//! in the clear. A run reads them from there ([`claves`]); the old
//! engine's `.env` is still looked in, until its keys are brought over.

use std::path::Path;

use crate::proveedores::Proveedor;

/// The credential store's service name for Komunta's keys.
const SERVICIO: &str = "Komunta";

/// Something that keeps keys by provider.
pub trait Llavero {
    /// The key kept for `proveedor`, if any.
    fn leer(&self, proveedor: Proveedor) -> Option<String>;
    /// Keeps `clave` for `proveedor`, replacing any before.
    fn guardar(&self, proveedor: Proveedor, clave: &str) -> Result<(), String>;
    /// Forgets the key for `proveedor`; none kept is fine.
    fn borrar(&self, proveedor: Proveedor) -> Result<(), String>;
}

/// The system's credential store.
#[derive(Clone, Copy, Debug, Default)]
pub struct LlaveroSistema;

impl LlaveroSistema {
    fn entrada(proveedor: Proveedor) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICIO, proveedor.id()).map_err(|error| error.to_string())
    }
}

impl Llavero for LlaveroSistema {
    fn leer(&self, proveedor: Proveedor) -> Option<String> {
        Self::entrada(proveedor).ok()?.get_password().ok()
    }

    fn guardar(&self, proveedor: Proveedor, clave: &str) -> Result<(), String> {
        Self::entrada(proveedor)?
            .set_password(clave)
            .map_err(|error| format!("no se pudo guardar la clave: {error}"))
    }

    fn borrar(&self, proveedor: Proveedor) -> Result<(), String> {
        match Self::entrada(proveedor)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("no se pudo borrar la clave: {error}")),
        }
    }
}

/// A key as typed or pasted, cleaned, or why it cannot be one: keys are
/// long runs of letters, digits, `-`, `_` and `.`.
pub fn limpiar(clave: &str) -> Result<String, String> {
    let clave = clave.trim();
    let valida = (16..=300).contains(&clave.len())
        && clave
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valida {
        Ok(clave.to_owned())
    } else {
        Err("Eso no parece una clave: se esperan letras, números, - y _, sin espacios.".to_owned())
    }
}

/// How a key is shown once kept: its last four characters, never more.
pub fn final_visible(clave: &str) -> String {
    let caracteres: Vec<char> = clave.chars().collect();
    if caracteres.len() < 8 {
        return String::new();
    }
    caracteres[caracteres.len() - 4..].iter().collect()
}

/// The key for `proveedor`: the one kept, else the one in the engine's
/// `.env` at `env_motor`, else the system's environment; for Google,
/// `GOOGLE_API_KEY` after `GEMINI_API_KEY`.
pub fn clave(proveedor: Proveedor, llavero: &impl Llavero, env_motor: &Path) -> Option<String> {
    let buscar = |variable: &str| en_env(env_motor, variable).or_else(|| std::env::var(variable).ok());
    let respaldo = || (proveedor == Proveedor::Google).then(|| buscar("GOOGLE_API_KEY")).flatten();
    llavero
        .leer(proveedor)
        .or_else(|| buscar(proveedor.variable()))
        .or_else(respaldo)
        .filter(|clave| !clave.trim().is_empty())
}

/// Every provider's [`clave`], by its id: "anthropic", "openai", "google".
pub fn claves(llavero: &impl Llavero, env_motor: &Path) -> std::collections::HashMap<String, String> {
    Proveedor::TODOS
        .into_iter()
        .filter_map(|proveedor| Some((proveedor.id().to_owned(), clave(proveedor, llavero, env_motor)?)))
        .collect()
}

/// `variable`'s value in the `.env` file at `ruta`, read the way the engine
/// reads it; None when it is not there or empty.
pub fn en_env(ruta: &Path, variable: &str) -> Option<String> {
    let texto = std::fs::read_to_string(ruta).ok()?;
    texto.lines().find_map(|linea| {
        let (clave, valor) = linea.trim().split_once('=')?;
        (clave.trim() == variable)
            .then(|| valor.trim().trim_matches(['"', '\'']).to_owned())
            .filter(|v| !v.is_empty())
    })
}

#[cfg(test)]
pub(crate) mod prueba {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// Keys in memory, for tests.
    #[derive(Default)]
    pub struct LlaveroEnMemoria(pub RefCell<HashMap<Proveedor, String>>);

    impl Llavero for LlaveroEnMemoria {
        fn leer(&self, proveedor: Proveedor) -> Option<String> {
            self.0.borrow().get(&proveedor).cloned()
        }
        fn guardar(&self, proveedor: Proveedor, clave: &str) -> Result<(), String> {
            self.0.borrow_mut().insert(proveedor, clave.to_owned());
            Ok(())
        }
        fn borrar(&self, proveedor: Proveedor) -> Result<(), String> {
            self.0.borrow_mut().remove(&proveedor);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prueba::LlaveroEnMemoria;
    use super::*;

    #[test]
    fn a_pasted_key_is_trimmed_and_checked() {
        assert_eq!(
            limpiar("  sk-ant-api03-abcdefghijklmnop \n").unwrap(),
            "sk-ant-api03-abcdefghijklmnop"
        );
        assert!(limpiar("corta").is_err());
        assert!(limpiar("sk-ant api03 with spaces inside").is_err());
    }

    #[test]
    fn only_the_last_four_are_ever_shown() {
        assert_eq!(final_visible("sk-ant-api03-abcdWXYZ"), "WXYZ");
        assert_eq!(final_visible("short"), "");
    }

    #[test]
    fn a_key_kept_comes_before_the_one_in_the_engine_env() {
        let env = std::env::temp_dir().join(format!("komunta-llavero-{}.env", std::process::id()));
        std::fs::write(&env, "# claves
OPENAI_API_KEY = \"sk-del-env\"
GOOGLE_API_KEY='AIza-del-env'
").unwrap();
        let llavero = LlaveroEnMemoria::default();
        llavero.guardar(Proveedor::OpenAi, "sk-guardada").unwrap();
        assert_eq!(clave(Proveedor::OpenAi, &llavero, &env).as_deref(), Some("sk-guardada"));
        llavero.borrar(Proveedor::OpenAi).unwrap();
        assert_eq!(clave(Proveedor::OpenAi, &llavero, &env).as_deref(), Some("sk-del-env"));
        assert_eq!(en_env(&env, "GOOGLE_API_KEY").as_deref(), Some("AIza-del-env"));
        assert_eq!(en_env(&env, "ANTHROPIC_API_KEY"), None);
        let _ = std::fs::remove_file(&env);
    }
}
