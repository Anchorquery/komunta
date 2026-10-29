//! Where the AI providers' keys are kept: the system's own credential
//! store (Windows' Credential Manager, the macOS keychain), never a file
//! in the clear. Each run hands them to the engine in its environment.

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

/// The environment a run of the engine gets: each kept key under the
/// variable the engine reads.
pub fn entorno(llavero: &impl Llavero) -> Vec<(&'static str, String)> {
    Proveedor::TODOS
        .into_iter()
        .filter_map(|proveedor| Some((proveedor.variable(), llavero.leer(proveedor)?)))
        .collect()
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
    fn the_engine_gets_each_kept_key_under_its_variable() {
        let llavero = LlaveroEnMemoria::default();
        llavero
            .guardar(Proveedor::Google, "AIzaSyExampleExample")
            .unwrap();
        assert_eq!(
            entorno(&llavero),
            [("GEMINI_API_KEY", "AIzaSyExampleExample".to_owned())]
        );
        llavero.borrar(Proveedor::Google).unwrap();
        assert!(entorno(&llavero).is_empty());
    }
}
