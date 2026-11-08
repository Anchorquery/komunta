//! The engine's settings for one run, as its `config.cargar` makes them:
//! `config.yaml`, then `ajustes.json` over it (what its settings page
//! changed), then the team's choices for the run over both (the dotted
//! `"ia.modo"` keys of `MOTOR_AJUSTES`).

use serde_json::{Map, Value};

/// `encima` over `base`, object into object; anything else replaces.
pub fn fusionar(base: &mut Value, encima: Value) {
    let Value::Object(encima) = encima else {
        return;
    };
    if !base.is_object() {
        *base = Value::Object(Map::new());
    }
    let base = base.as_object_mut().expect("just made an object");
    for (clave, valor) in encima {
        match (base.get_mut(&clave), valor) {
            (Some(destino @ Value::Object(_)), valor @ Value::Object(_)) => fusionar(destino, valor),
            (_, valor) => {
                base.insert(clave, valor);
            }
        }
    }
}

/// Sets `valor` at the dotted `ruta` ("ia.google.modelo"), making the
/// objects on the way.
pub fn poner(destino: &mut Value, ruta: &str, valor: Value) {
    let mut nodo = destino;
    let partes: Vec<&str> = ruta.split('.').collect();
    let (ultima, camino) = partes.split_last().expect("split gives one part at least");
    for parte in camino {
        if !nodo[*parte].is_object() {
            nodo[*parte] = Value::Object(Map::new());
        }
        nodo = &mut nodo[*parte];
    }
    nodo[*ultima] = valor;
}

/// Each dotted setting of `cambios` - an object like `MOTOR_AJUSTES` - put
/// into `destino`.
pub fn aplicar(destino: &mut Value, cambios: &Value) {
    for (ruta, valor) in cambios.as_object().into_iter().flatten() {
        poner(destino, ruta, valor.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_is_changed_lands_over_the_defaults() {
        let mut config = json!({"ia": {"modo": "claude_code", "google": {"modelo": "a", "respaldo": ["b"]}}});
        fusionar(&mut config, json!({"ia": {"modo": "anthropic", "google": {"modelo": "c"}}}));
        assert_eq!(config, json!({"ia": {"modo": "anthropic", "google": {"modelo": "c", "respaldo": ["b"]}}}));
        aplicar(&mut config, &json!({"ia.anthropic.esfuerzo": "low", "descripcion.modo": "ninguno"}));
        assert_eq!(config["ia"]["anthropic"]["esfuerzo"], "low");
        assert_eq!(config["descripcion"]["modo"], "ninguno");
    }
}
