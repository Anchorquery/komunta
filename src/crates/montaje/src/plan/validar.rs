//! The plan checked before anything is built from it, as the engine's
//! `motor/validar.py` checks it: a broken plan fails here with a sentence
//! the AI can correct, not halfway through the montage. First the types of
//! each field (an AI writes "12" for 12), then what each one points at.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::python::{lleno, repr, texto};

const ESTILOS: [&str; 3] = ["clave", "enfasis", "normal"];
const POSICIONES: [&str; 4] = ["auto", "arriba", "centro", "abajo"];
const GARABATOS: [&str; 3] = ["corazon", "circulo", "subrayado"];
const AUDIO_PLANO: [&str; 2] = ["silencio", "ambiente"];

/// What is wrong with `plan` for the job's `analisis` and `inventario`;
/// nothing when it can be built.
pub fn validar(plan: &Value, analisis: &Value, inventario: &Value) -> Vec<String> {
    let errores = tipos(plan);
    if !errores.is_empty() {
        return errores;
    }
    let material = Material::de(analisis, inventario);
    let tomas = lista(plan, "tomas");
    if tomas.is_empty() {
        return vec!["El plan no tiene tomas.".into()];
    }
    let mut errores = Vec::new();
    let palabras: Vec<usize> = tomas
        .iter()
        .enumerate()
        .map(|(k, toma)| revisar_toma(k, toma, &material, &mut errores))
        .collect();
    let anclas = Anclas { palabras: &palabras };
    let subtitulos = lista(plan, "subtitulos");
    for (n, subtitulo) in subtitulos.iter().enumerate() {
        revisar_subtitulo(n, subtitulo, &anclas, &mut errores);
    }
    for (n, recurso) in lista(plan, "recursos").iter().enumerate() {
        revisar_recurso(n, recurso, subtitulos.len(), &material, &anclas, &mut errores);
    }
    revisar_musica(&plan["musica"], &material, &mut errores);
    errores
}

/// The ids a plan can use.
struct Material {
    clips: Map<String, Value>,
    imagenes: HashSet<String>,
    /// The music, sorted, as the error lists it.
    audios: Vec<String>,
}

impl Material {
    fn de(analisis: &Value, inventario: &Value) -> Self {
        let por_id = |valor: &Value| -> Vec<(String, Value)> {
            valor
                .as_array()
                .into_iter()
                .flatten()
                .map(|e| (texto(&e["id"]), e.clone()))
                .collect()
        };
        let mut audios: Vec<String> = por_id(&analisis["audios"]).into_iter().map(|(id, _)| id).collect();
        audios.sort();
        audios.dedup();
        Material {
            clips: por_id(&analisis["clips"]).into_iter().collect(),
            imagenes: por_id(&inventario["imagenes"]).into_iter().map(|(id, _)| id).collect(),
            audios,
        }
    }

    fn clip(&self, id: &Value) -> Option<&Value> {
        id.as_str().and_then(|id| self.clips.get(id))
    }

    fn existe(&self, id: &Value) -> bool {
        id.as_str()
            .is_some_and(|id| self.clips.contains_key(id) || self.imagenes.contains(id))
    }
}

/// The takes' word counts, to check what is anchored to them.
struct Anclas<'a> {
    palabras: &'a [usize],
}

impl Anclas<'_> {
    fn toma(&self, k: &Value) -> Option<usize> {
        k.as_u64()
            .filter(|_| es_entero(k))
            .map(|k| k as usize)
            .filter(|k| *k < self.palabras.len())
    }

    fn palabra(&self, k: &Value, i: &Value) -> bool {
        let Some(k) = self.toma(k) else {
            return false;
        };
        es_entero(i) && i.as_u64().is_some_and(|i| (i as usize) < self.palabras[k])
    }
}

fn es_entero(valor: &Value) -> bool {
    valor.is_i64() || valor.is_u64()
}

fn es_numero(valor: &Value) -> bool {
    valor.is_number()
}

fn real(valor: &Value) -> f64 {
    valor.as_f64().unwrap_or(f64::NAN)
}

fn tiene(objeto: &Value, clave: &str) -> bool {
    objeto.get(clave).is_some()
}

/// `objeto[clave]`, or `defecto` when it is not there (`dict.get`).
fn o<'a>(objeto: &'a Value, clave: &str, defecto: &'a Value) -> &'a Value {
    objeto.get(clave).unwrap_or(defecto)
}

fn lista<'a>(plan: &'a Value, clave: &str) -> &'a [Value] {
    plan[clave].as_array().map_or(&[], Vec::as_slice)
}

fn dentro(valor: &Value, opciones: &[&str]) -> bool {
    valor.as_str().is_some_and(|v| opciones.contains(&v))
}

/// The types of every field, before their values are looked at.
fn tipos(plan: &Value) -> Vec<String> {
    if !plan.is_object() {
        return vec!["El plan debe ser un objeto JSON.".into()];
    }
    let mut errores = Vec::new();
    for clave in ["tomas", "subtitulos", "recursos"] {
        let valor = o(plan, clave, &Value::Null);
        let bien = match plan.get(clave) {
            None => true,
            Some(_) => valor.as_array().is_some_and(|l| l.iter().all(Value::is_object)),
        };
        if !bien {
            errores.push(format!("«{clave}» debe ser una lista de objetos."));
        }
    }
    if !errores.is_empty() {
        return errores;
    }
    for (k, toma) in lista(plan, "tomas").iter().enumerate() {
        tipos_toma(k, toma, &mut errores);
    }
    for (n, subtitulo) in lista(plan, "subtitulos").iter().enumerate() {
        tipos_subtitulo(n, subtitulo, &mut errores);
    }
    for (n, recurso) in lista(plan, "recursos").iter().enumerate() {
        tipos_recurso(n, recurso, &mut errores);
    }
    tipos_musica(&plan["musica"], &mut errores);
    errores
}

fn enteros(objeto: &Value, donde: &str, claves: &[&str], que: &str, errores: &mut Vec<String>) {
    for clave in claves {
        if objeto.get(*clave).is_some_and(|v| !es_entero(v)) {
            errores.push(format!("{donde}.{clave} debe ser {que}."));
        }
    }
}

fn numeros(objeto: &Value, donde: &str, claves: &[&str], positivos: bool, errores: &mut Vec<String>) {
    for clave in claves {
        let mal = objeto
            .get(*clave)
            .is_some_and(|v| !es_numero(v) || (positivos && real(v) < 0.0));
        if mal {
            let que = if positivos { "un número positivo" } else { "un número" };
            errores.push(format!("{donde}.{clave} debe ser {que}."));
        }
    }
}

fn tipos_toma(k: usize, toma: &Value, errores: &mut Vec<String>) {
    let donde = format!("tomas[{k}]");
    enteros(toma, &donde, &["desde", "hasta"], "un número entero (índice de palabra)", errores);
    numeros(toma, &donde, &["desde_seg", "dur"], false, errores);
    let quitar = &toma["quitar"];
    let par = |x: &Value| x.as_array().is_some_and(|p| p.len() == 2 && p.iter().all(es_entero));
    let bien = !lleno(quitar) || quitar.as_array().is_some_and(|q| q.iter().all(par));
    if !bien {
        errores.push(format!("{donde}.quitar debe ser una lista de pares [desde, hasta] enteros."));
    }
}

fn tipos_subtitulo(n: usize, subtitulo: &Value, errores: &mut Vec<String>) {
    let donde = format!("subtitulos[{n}]");
    enteros(subtitulo, &donde, &["toma", "desde", "hasta"], "un número entero", errores);
    numeros(subtitulo, &donde, &["seg", "dur"], false, errores);
    if subtitulo.get("texto").is_some_and(|t| !t.is_string()) {
        errores.push(format!("{donde}.texto debe ser texto."));
    }
}

fn tipos_recurso(n: usize, recurso: &Value, errores: &mut Vec<String>) {
    let donde = format!("recursos[{n}]");
    enteros(recurso, &donde, &["subtitulo", "toma", "palabra"], "un número entero", errores);
    numeros(recurso, &donde, &["dur", "desde_seg", "seg"], true, errores);
    let posicion = &recurso["garabato_pos"];
    let en_cuadro = |x: &Value| es_numero(x) && (0.0..=1.0).contains(&real(x));
    let bien = posicion.is_null() || posicion.as_array().is_some_and(|p| p.len() == 2 && p.iter().all(en_cuadro));
    if !bien {
        errores.push(format!("{donde}.garabato_pos debe ser [x, y] entre 0 y 1."));
    }
}

fn tipos_musica(musica: &Value, errores: &mut Vec<String>) {
    if !musica.is_null() && !musica.is_object() {
        errores.push("musica debe ser un objeto o null.".into());
    } else if lleno(musica) {
        numeros(musica, "musica", &["volumen", "desde_seg"], true, errores);
    }
}

/// Checks take `k`; returns how many words it can be anchored to.
fn revisar_toma(k: usize, toma: &Value, material: &Material, errores: &mut Vec<String>) -> usize {
    if tiene(toma, "desde_seg") && !tiene(toma, "desde") {
        revisar_plano(k, toma, material, errores);
        return 0;
    }
    let Some(clip) = material.clip(&toma["clip"]) else {
        errores.push(format!("tomas[{k}]: el clip «{}» no existe.", texto(&toma["clip"])));
        return 0;
    };
    let n = clip["transcripcion"]["palabras"].as_array().map_or(0, Vec::len);
    if n == 0 {
        errores.push(format!(
            "tomas[{k}]: el clip {} no tiene transcripción (¿es una toma con voz?).",
            texto(&toma["clip"])
        ));
        return 0;
    }
    let menos_uno = Value::from(-1);
    let (desde, hasta) = (real(o(toma, "desde", &menos_uno)), real(o(toma, "hasta", &menos_uno)));
    if !(0.0 <= desde && desde <= hasta && hasta < n as f64) {
        errores.push(format!(
            "tomas[{k}] ({}): desde/hasta = {}/{} fuera de 0..{}.",
            texto(&toma["clip"]),
            texto(&toma["desde"]),
            texto(&toma["hasta"]),
            n - 1
        ));
    }
    let cero = Value::from(0);
    let inicio = real(o(toma, "desde", &cero));
    for quitar in toma["quitar"].as_array().into_iter().flatten() {
        let par = quitar.as_array().map_or(&[][..], Vec::as_slice);
        let bien = par.len() == 2 && inicio <= real(&par[0]) && real(&par[0]) <= real(&par[1]) && real(&par[1]) <= hasta;
        if !bien {
            errores.push(format!("tomas[{k}]: quitar {} fuera del rango de la toma.", repr(quitar)));
        }
    }
    n
}

/// A shot with no words, by seconds.
fn revisar_plano(k: usize, toma: &Value, material: &Material, errores: &mut Vec<String>) {
    let clip = &toma["clip"];
    let desde = &toma["desde_seg"];
    let dos = Value::from(2.0);
    let dur = o(toma, "dur", &dos);
    if !material.existe(clip) {
        errores.push(format!("tomas[{k}]: el clip «{}» no existe.", texto(clip)));
    } else if !es_numero(desde) || real(desde) < 0.0 {
        errores.push(format!("tomas[{k}]: desde_seg no válido."));
    } else if !es_numero(dur) || !(0.5..=30.0).contains(&real(dur)) {
        errores.push(format!("tomas[{k}]: dur debe estar entre 0,5 y 30 s."));
    } else if let Some(datos) = material.clip(clip)
        && real(desde) >= real(&datos["duracion"])
    {
        errores.push(format!("tomas[{k}]: desde_seg {} pasa del final del clip.", texto(desde)));
    }
    let silencio = Value::from("silencio");
    if !dentro(o(toma, "audio", &silencio), &AUDIO_PLANO) {
        errores.push(format!(
            "tomas[{k}]: audio «{}» no existe (silencio | ambiente).",
            texto(&toma["audio"])
        ));
    }
}

fn revisar_subtitulo(n: usize, subtitulo: &Value, anclas: &Anclas, errores: &mut Vec<String>) {
    let vacio = Value::from("");
    if o(subtitulo, "texto", &vacio).as_str().unwrap_or("").trim().is_empty() {
        errores.push(format!("subtitulos[{n}]: sin texto."));
    }
    let toma = &subtitulo["toma"];
    let sobre_un_plano = !tiene(subtitulo, "desde") && anclas.toma(toma).is_some_and(|k| anclas.palabras[k] == 0);
    let (desde, hasta) = (&subtitulo["desde"], &subtitulo["hasta"]);
    let anclado = anclas.palabra(toma, desde) && anclas.palabra(toma, hasta) && real(desde) <= real(hasta);
    if !sobre_un_plano && !anclado {
        errores.push(format!(
            "subtitulos[{n}] «{}»: toma/desde/hasta inválidos.",
            texto(o(subtitulo, "texto", &vacio))
        ));
    }
    let normal = Value::from("normal");
    if !dentro(o(subtitulo, "estilo", &normal), &ESTILOS) {
        errores.push(format!(
            "subtitulos[{n}]: estilo «{}» no existe ({}).",
            texto(&subtitulo["estilo"]),
            ESTILOS.join(", ")
        ));
    }
    let auto = Value::from("auto");
    if !dentro(o(subtitulo, "posicion", &auto), &POSICIONES) {
        errores.push(format!("subtitulos[{n}]: posición «{}» no existe.", texto(&subtitulo["posicion"])));
    }
}

fn revisar_recurso(
    n: usize,
    recurso: &Value,
    subtitulos: usize,
    material: &Material,
    anclas: &Anclas,
    errores: &mut Vec<String>,
) {
    if !material.existe(&recurso["clip"]) {
        errores.push(format!("recursos[{n}]: el clip «{}» no existe.", texto(&recurso["clip"])));
    }
    let cero = Value::from(0);
    if let Some(subtitulo) = recurso.get("subtitulo") {
        if subtitulo.as_u64().is_none_or(|s| s as usize >= subtitulos) {
            errores.push(format!("recursos[{n}]: subtítulo {} no existe.", texto(subtitulo)));
        }
    } else if !tiene(recurso, "palabra")
        && anclas.toma(&recurso["toma"]).is_some()
        && es_numero(o(recurso, "seg", &cero))
    {
        // Anchored to a second of the take.
    } else if !anclas.palabra(&recurso["toma"], &recurso["palabra"]) {
        errores.push(format!(
            "recursos[{n}]: necesita «subtitulo», «toma»+«palabra» o «toma»+«seg» válidos."
        ));
    }
    let garabato = &recurso["garabato"];
    if lleno(garabato) && !dentro(garabato, &GARABATOS) {
        errores.push(format!("recursos[{n}]: garabato «{}» no existe.", texto(garabato)));
    }
}

fn revisar_musica(musica: &Value, material: &Material, errores: &mut Vec<String>) {
    if !lleno(musica) {
        return;
    }
    let disponible = musica["clip"].as_str().is_some_and(|c| material.audios.iter().any(|a| a == c));
    if !disponible {
        let cuales = if material.audios.is_empty() {
            "no hay ninguna".to_owned()
        } else {
            material.audios.join(", ")
        };
        errores.push(format!(
            "musica: «{}» no es una música disponible ({cuales}).",
            texto(&musica["clip"])
        ));
    } else if !(0.0..=1.0).contains(&real(o(musica, "volumen", &Value::from(0.16)))) {
        errores.push("musica: volumen entre 0 y 1.".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn analisis() -> Value {
        json!({
            "clips": [
                {"id": "voz", "duracion": 10.0, "transcripcion": {"palabras": [{}, {}, {}, {}, {}]}},
                {"id": "plano", "duracion": 4.0, "transcripcion": null},
            ],
            "audios": [{"id": "musica_kit"}],
        })
    }

    fn inventario() -> Value {
        json!({"imagenes": [{"id": "foto"}]})
    }

    fn errores(plan: Value) -> Vec<String> {
        validar(&plan, &analisis(), &inventario())
    }

    #[test]
    fn a_good_plan_passes() {
        let plan = json!({
            "tomas": [{"clip": "voz", "desde": 0, "hasta": 4, "quitar": [[1, 2]]},
                      {"clip": "plano", "desde_seg": 1.0, "dur": 2.0, "audio": "ambiente"}],
            "subtitulos": [{"texto": "Hola", "toma": 0, "desde": 0, "hasta": 2, "estilo": "enfasis"},
                           {"texto": "Plano", "toma": 1}],
            "recursos": [{"clip": "foto", "subtitulo": 0, "garabato": "corazon", "garabato_pos": [0.5, 0.5]},
                         {"clip": "plano", "toma": 0, "palabra": 3, "dur": 2.0},
                         {"clip": "plano", "toma": 0, "seg": 1.5}],
            "musica": {"clip": "musica_kit", "volumen": 0.16},
        });
        assert_eq!(errores(plan), Vec::<String>::new());
    }

    #[test]
    fn wrong_types_are_told_before_anything_else() {
        let plan = json!({"tomas": [{"clip": "voz", "desde": "0", "hasta": 4.0, "quitar": [[1]]}],
                          "recursos": [{"clip": "x", "dur": -1}], "musica": "sí"});
        assert_eq!(
            errores(plan),
            [
                "tomas[0].desde debe ser un número entero (índice de palabra).",
                "tomas[0].hasta debe ser un número entero (índice de palabra).",
                "tomas[0].quitar debe ser una lista de pares [desde, hasta] enteros.",
                "recursos[0].dur debe ser un número positivo.",
                "musica debe ser un objeto o null.",
            ]
        );
        assert_eq!(errores(json!([])), ["El plan debe ser un objeto JSON."]);
        assert_eq!(errores(json!({"tomas": null})), ["«tomas» debe ser una lista de objetos."]);
        assert_eq!(errores(json!({})), ["El plan no tiene tomas."]);
    }

    #[test]
    fn what_points_nowhere_is_told() {
        let plan = json!({
            "tomas": [{"clip": "voz", "desde": 2, "hasta": 9, "quitar": [[0, 1]]},
                      {"clip": "nada", "desde": 0, "hasta": 1},
                      {"clip": "plano", "desde_seg": 5, "audio": "musica"}],
            "subtitulos": [{"texto": " ", "toma": 0, "desde": 3, "hasta": 2, "estilo": "grande", "posicion": "fuera"}],
            "recursos": [{"clip": "otro", "subtitulo": 3}, {"clip": "foto", "toma": 0, "palabra": 7, "garabato": "x"}],
            "musica": {"clip": "cancion"},
        });
        assert_eq!(
            errores(plan),
            [
                "tomas[0] (voz): desde/hasta = 2/9 fuera de 0..4.",
                "tomas[0]: quitar [0, 1] fuera del rango de la toma.",
                "tomas[1]: el clip «nada» no existe.",
                "tomas[2]: desde_seg 5 pasa del final del clip.",
                "tomas[2]: audio «musica» no existe (silencio | ambiente).",
                "subtitulos[0]: sin texto.",
                "subtitulos[0] « »: toma/desde/hasta inválidos.",
                "subtitulos[0]: estilo «grande» no existe (clave, enfasis, normal).",
                "subtitulos[0]: posición «fuera» no existe.",
                "recursos[0]: el clip «otro» no existe.",
                "recursos[0]: subtítulo 3 no existe.",
                "recursos[1]: necesita «subtitulo», «toma»+«palabra» o «toma»+«seg» válidos.",
                "recursos[1]: garabato «x» no existe.",
                "musica: «cancion» no es una música disponible (musica_kit).",
            ]
        );
    }
}
