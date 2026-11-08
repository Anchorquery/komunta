//! JSON values written as the Python engine wrote them in the texts the AI
//! reads and the errors the team sees: `None`, `True`, `12`, `2.5`,
//! `[5, 6]`. The planner's prompt and the plan's checks are held to the
//! engine's, character for character.

use serde_json::Value;

/// `str(valor)`: a string as it is, everything else as [`repr`].
pub fn texto(valor: &Value) -> String {
    match valor {
        Value::String(texto) => texto.clone(),
        otro => repr(otro),
    }
}

/// `repr(valor)`.
pub fn repr(valor: &Value) -> String {
    match valor {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(numero) => match numero.as_f64() {
            Some(real) if numero.is_f64() => flotante(real),
            _ => numero.to_string(),
        },
        Value::String(texto) => cadena(texto),
        Value::Array(lista) => format!("[{}]", lista.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Value::Object(mapa) => {
            let pares: Vec<String> = mapa.iter().map(|(k, v)| format!("{}: {}", cadena(k), repr(v))).collect();
            format!("{{{}}}", pares.join(", "))
        }
    }
}

/// A float as Python prints it: `12.0`, `2.5`.
pub fn flotante(valor: f64) -> String {
    analisis::descripcion::numero(valor)
}

/// A number from the JSON as Python prints it: an integer stays one.
pub fn numero(valor: &Value) -> String {
    match valor {
        Value::Number(_) => repr(valor),
        otro => texto(otro),
    }
}

/// Python's truthiness: null, false, 0, "" and empty lists and objects are
/// false.
pub fn lleno(valor: &Value) -> bool {
    match valor {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// A string in quotes, as `repr` writes it: single ones, unless it holds
/// one and no double.
fn cadena(texto: &str) -> String {
    let comilla = if texto.contains('\'') && !texto.contains('"') { '"' } else { '\'' };
    let mut salida = String::from(comilla);
    for c in texto.chars() {
        match c {
            '\\' => salida.push_str("\\\\"),
            '\n' => salida.push_str("\\n"),
            '\t' => salida.push_str("\\t"),
            '\r' => salida.push_str("\\r"),
            c if c == comilla => {
                salida.push('\\');
                salida.push(c);
            }
            c => salida.push(c),
        }
    }
    salida.push(comilla);
    salida
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_print_as_python_prints_them() {
        assert_eq!(repr(&json!(null)), "None");
        assert_eq!(repr(&json!(true)), "True");
        assert_eq!(repr(&json!(12)), "12");
        assert_eq!(repr(&json!(12.0)), "12.0");
        assert_eq!(repr(&json!(9.365)), "9.365");
        assert_eq!(repr(&json!([5, "a", [1.5]])), "[5, 'a', [1.5]]");
        assert_eq!(repr(&json!("it's")), "\"it's\"");
        assert_eq!(texto(&json!("IMG_1")), "IMG_1");
        assert_eq!(texto(&json!(null)), "None");
    }

    #[test]
    fn truthiness_is_python_s() {
        for vacio in [json!(null), json!(0), json!(0.0), json!(""), json!([]), json!({}), json!(false)] {
            assert!(!lleno(&vacio), "{vacio}");
        }
        assert!(lleno(&json!({"a": 1})));
    }
}
