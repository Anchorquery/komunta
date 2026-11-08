//! The planner's prompt, as the engine's `motor/ia.py` builds it: the
//! instructions and the format of the plan, the recipe's rules, then the
//! material - documents, clips with their words, B-roll, stills, music and
//! the contact sheets - with what the AI that watched it said of each.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use analisis::descripcion::Descripcion;
use serde_json::Value;

use crate::python::{lleno, numero, texto};

/// What the planner is told before the material; `{plantilla}` and
/// `{reglas_plantilla}` are filled in.
const INSTRUCCIONES: &str = r##"Eres el editor de vídeo del equipo de comunicación. Con los recursos de una carpeta tienes que decidir
el montaje de un vídeo vertical (720x1280) de la plantilla «{plantilla}». No editas vídeo: devuelves un
plan en JSON que un motor de render ejecuta tal cual.

## Material
- Tomas a cámara: clips con voz. Tienes su transcripción palabra a palabra con índices (i:palabra).
- Recursos: clips sin voz (planos del pueblo, del acto, del público...) e imágenes.
- Descripción de cada clip y foto hecha por una IA que los ha visto enteros (si la hay): qué se ve, tipo de
  plano, problemas (movido, desenfocado...) y momentos útiles con sus segundos. Fíate de ella para elegir
  recursos y desde_seg; evita los marcados "no_usar" o con problemas graves.
- Hojas de contacto adjuntas: 3 fotogramas por clip, con su id, duración y % de voz.
- Músicas disponibles (si hay): del material o del kit de marca, con su tempo.
- Documentos: guion (qué se dice y qué recursos van en cada sección) y subtítulos ya escritos, una línea por subtítulo.

## Reglas de estilo (sacadas de los vídeos que hace el editor)
1. La voz de las tomas suena continua; la imagen alterna entre la persona hablando y recursos.
   Aproximadamente la mitad del tiempo se ve un recurso. Cada recurso dura entre 1,5 y 2,5 s.
2. Se sigue el orden del guion. Si una frase tiene varias tomas, elige la más completa y limpia
   (sin errores, sin cortes, sin risas) y normalmente la última buena.
3. Recorta el principio y el final de cada toma: empieza en la primera palabra útil y acaba en la última.
   Marca para eliminar coletillas y titubeos ("eh", "pues", repeticiones) si están entre frases.
4. Subtítulos: usa los subtítulos escritos tal cual (no los traduzcas ni los cambies). Cada línea se
   ancla a las palabras de la toma que dicen lo mismo (aunque estén en otro idioma).
5. Estilo "enfasis" (mayúsculas condensadas + palabra gigante de transición) para preguntas,
   cifras, lemas y frases clave; el resto "normal". Aprox. 1 de cada 3 líneas en énfasis, nunca dos seguidas
   salvo preguntas encadenadas.
6. Recursos: elige el que mejor ilustra lo que se dice en ese momento (según el guion y lo que se ve).
   No repitas un recurso salvo necesidad. No dejes más de ~6 s seguidos de persona hablando sin recurso,
   aunque ese tramo no tenga subtítulo (ancla el recurso a una palabra de la toma). Los recursos con voz de fondo de otras personas no tapan la toma.
7. El lema final del guion cierra el vídeo, dicho a cámara (no lo tapes con recurso).

{reglas_plantilla}
Si una regla de la plantilla contradice las generales, manda la de la plantilla.

## Formato de respuesta
Devuelve SOLO un JSON con esta forma:
{
  "plantilla": "{plantilla}",
  "tomas": [            // en orden de aparición; definen la pista de voz
    {"clip": "IMG_8441", "desde": 0, "hasta": 16, "quitar": [[5, 6]]},  // índices de palabra (inclusive)
    {"clip": "2", "desde": 0, "hasta": 40, "solo_audio": true}  // voz en off: su imagen nunca se ve
  ],
  "subtitulos": [       // en orden
    {"texto": "Gauza bat azaldu nahi dizut:", "toma": 0, "desde": 0, "hasta": 5, "estilo": "normal"},
    // toma = índice en "tomas"; desde/hasta = índices de palabra de esa toma
    // estilo: normal | enfasis | clave (palabra clave grande, sin palabra gigante)
    // opcionales: "posicion": arriba|centro|abajo|auto; "apilar": "grupo" (líneas consecutivas que se acumulan,
    //   p. ej. enumeraciones) y "apilar_hacia": "arriba" si cada línea nueva entra encima de la anterior
  ],
  // también hay tomas SIN palabras (planos por segundos, para resúmenes con música o transiciones):
  //   {"clip": "IMG_8400", "desde_seg": 1.0, "dur": 2.0, "audio": "silencio"}   // audio: silencio | ambiente
  //   el corte se ajusta solo al pulso de la música; un subtítulo sobre un plano: {"texto": "...", "toma": 3}
  "recursos": [
    {"clip": "IMG_8355", "subtitulo": 2, "desde_seg": 1.2},
    // cubre la imagen durante el subtítulo indicado; desde_seg = segundo del clip donde empieza (usa mejor_tramo si dudas)
    {"clip": "IMG_8430", "toma": 12, "palabra": 3, "dur": 2.0, "desde_seg": 0.5},
    // para tramos sin subtítulo: empieza en esa palabra de la toma y dura "dur" segundos
    {"clip": "foto_1937", "toma": 1, "palabra": 10, "dur": 2.5, "efecto": "bn"}
    // "efecto": "bn" = blanco y negro con contraste (fotos o planos de archivo)
    // "garabato": "corazon"|"circulo"|"subrayado" = trazo blanco dibujado a mano sobre el recurso,
    //   "garabato_pos": [x, y] (0-1) centrado en lo que se quiere señalar. Con moderación (1 de cada 4-5 recursos).
  ],
  "musica": null,       // o {"clip": "<id de una música disponible>", "volumen": 0.16, "desde_seg": 0}
                        // baja sola cuando hay voz; volumen 0.12-0.2 bajo voz continua, 0.6-0.8 si casi no hay voz
  "lema_final": "ANDOAIN, ZUREKIN BATERA!",
  "notas": ["dudas o problemas para que los revise una persona"]
}
"##;

/// What the planner gets for the job in `trabajo`: its `inventario` and
/// `analisis` (the JSON files), the contact sheets at `hojas`, the
/// template's name and the rules of the recipe.
pub fn construir(
    trabajo: &Path,
    inventario: &Value,
    analisis: &Value,
    hojas: &[PathBuf],
    plantilla: &str,
    reglas: &str,
) -> String {
    let descripciones = descripciones(analisis);
    let mut partes = vec![
        INSTRUCCIONES
            .replace("{plantilla}", plantilla)
            .replace("{reglas_plantilla}", reglas),
        "## Documentos".to_owned(),
    ];
    partes.extend(lista(&inventario["documentos"]).map(documento));
    partes.push(CABECERA_CON_VOZ.to_owned());
    let clips = lista(&analisis["clips"]);
    let (con_voz, sin_voz): (Vec<&Value>, Vec<&Value>) = clips.partition(|c| lleno(&c["transcripcion"]));
    partes.extend(con_voz.into_iter().map(|c| clip_con_voz(c, &descripciones)));
    partes.push("## Clips sin voz".to_owned());
    partes.extend(sin_voz.into_iter().map(|c| clip_sin_voz(c, &descripciones)));
    partes.extend(lista(&inventario["imagenes"]).map(|i| imagen(i, &descripciones)));
    let audios: Vec<&Value> = lista(&analisis["audios"]).collect();
    if !audios.is_empty() {
        partes.push("## Músicas disponibles".to_owned());
        partes.extend(audios.into_iter().map(musica));
    }
    partes.push(hojas_de_contacto(trabajo, hojas));
    partes.join("\n\n")
}

const CABECERA_CON_VOZ: &str = "## Clips con voz (transcripción; * = coletilla)\n\
    Ojo: un clip con voz no siempre es una toma a cámara (puede ser el público oyendo un discurso). \
    Decide con los fotogramas y con `cara_cerca` (cara grande en plano = alguien hablando a cámara).";

fn lista(valor: &Value) -> impl Iterator<Item = &Value> {
    valor.as_array().into_iter().flatten()
}

/// Each clip's and still's description, as the line the planner reads.
fn descripciones(analisis: &Value) -> HashMap<String, String> {
    lista(&analisis["clips"])
        .chain(lista(&analisis["imagenes"]))
        .map(|elemento| {
            let descripcion: Descripcion = serde_json::from_value(elemento["descripcion"].clone()).unwrap_or_default();
            (texto(&elemento["id"]), descripcion.texto())
        })
        .collect()
}

/// ` — <description>`, when the element has one.
fn extra(id: &Value, descripciones: &HashMap<String, String>) -> String {
    descripciones
        .get(&texto(id))
        .filter(|d| !d.is_empty())
        .map(|d| format!(" — {d}"))
        .unwrap_or_default()
}

fn documento(documento: &Value) -> String {
    let lineas: Vec<String> = lista(&documento["lineas"]).map(texto).collect();
    format!(
        "### {} (rol: {})\n{}",
        texto(&documento["id"]),
        texto(&documento["rol"]),
        lineas.join("\n")
    )
}

/// A clip's words as `i:word`, a filler word starred.
fn palabras(transcripcion: &Value) -> String {
    let palabras: Vec<String> = lista(&transcripcion["palabras"])
        .map(|p| {
            let estrella = if lleno(&p["coletilla"]) { "*" } else { "" };
            format!("{}:{}{estrella}", texto(&p["i"]), texto(&p["txt"]))
        })
        .collect();
    palabras.join(" ")
}

fn clip_con_voz(clip: &Value, descripciones: &HashMap<String, String>) -> String {
    let transcripcion = &clip["transcripcion"];
    format!(
        "- {} ({}s, {}, cara_cerca={}, mejor_tramo={}s){}: {}",
        texto(&clip["id"]),
        numero(&clip["duracion"]),
        texto(&transcripcion["idioma"]),
        texto(&clip["cara_cerca"]),
        numero(&clip["mejor_tramo"]["desde"]),
        extra(&clip["id"], descripciones),
        palabras(transcripcion)
    )
}

fn clip_sin_voz(clip: &Value, descripciones: &HashMap<String, String>) -> String {
    format!(
        "- {} ({}s, cara_cerca={}) mejor_tramo={}s{}",
        texto(&clip["id"]),
        numero(&clip["duracion"]),
        texto(&clip["cara_cerca"]),
        numero(&clip["mejor_tramo"]["desde"]),
        extra(&clip["id"], descripciones)
    )
}

fn imagen(imagen: &Value, descripciones: &HashMap<String, String>) -> String {
    format!(
        "- {} (imagen {}x{}){}",
        texto(&imagen["id"]),
        numero(&imagen["ancho"]),
        numero(&imagen["alto"]),
        extra(&imagen["id"], descripciones)
    )
}

fn musica(audio: &Value) -> String {
    let origen = if audio["id"] == "musica_kit" { "del kit de marca" } else { "del material" };
    let aviso = if audio["ratio_voz"].as_f64().unwrap_or(0.0) > 0.5 {
        " (OJO: parece voz hablada, no música)"
    } else {
        ""
    };
    let bpm = if lleno(&audio["bpm"]) { numero(&audio["bpm"]) } else { "?".into() };
    format!(
        "- {} ({origen}, {:.0}s, {bpm} BPM){aviso}",
        texto(&audio["id"]),
        audio["duracion"].as_f64().unwrap_or(0.0)
    )
}

fn hojas_de_contacto(trabajo: &Path, hojas: &[PathBuf]) -> String {
    let rutas: Vec<String> = hojas
        .iter()
        .map(|hoja| hoja.strip_prefix(trabajo).unwrap_or(hoja).display().to_string())
        .collect();
    format!("## Hojas de contacto\n{}", rutas.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_material_is_listed_as_the_engine_lists_it() {
        let inventario = json!({
            "documentos": [{"id": "guion", "rol": "guion", "lineas": ["Hola", "Adiós"]}],
            "imagenes": [{"id": "foto", "ancho": 1080, "alto": 1920}],
        });
        let analisis = json!({
            "clips": [
                {"id": "A", "duracion": 9.5, "cara_cerca": true, "mejor_tramo": {"desde": 0.0},
                 "transcripcion": {"idioma": "eu", "palabras": [
                    {"i": 0, "txt": "Kaixo", "coletilla": false}, {"i": 1, "txt": "eh", "coletilla": true}]},
                 "descripcion": {"descripcion": "Mujer a cámara", "plano": "medio"}},
                {"id": "B", "duracion": 4, "cara_cerca": null, "mejor_tramo": {"desde": 1.5}, "transcripcion": null},
            ],
            "imagenes": [{"id": "foto", "descripcion": {"descripcion": "Cartel"}}],
            "audios": [{"id": "musica_kit", "duracion": 92.5, "bpm": 120, "ratio_voz": 0.1}],
        });
        let trabajo = Path::new("trabajo");
        let pedido = construir(trabajo, &inventario, &analisis, &[trabajo.join("hojas").join("hoja_01.jpg")], "x", "R");
        assert!(pedido.starts_with("Eres el editor"));
        assert!(pedido.contains("plantilla «x»"));
        assert!(pedido.contains("\"plantilla\": \"x\","));
        assert!(pedido.contains("\n\nR\nSi una regla"));
        assert!(pedido.contains("### guion (rol: guion)\nHola\nAdiós"));
        assert!(pedido.contains(
            "- A (9.5s, eu, cara_cerca=True, mejor_tramo=0.0s) — Mujer a cámara · plano medio: 0:Kaixo 1:eh*"
        ));
        assert!(pedido.contains("\n\n## Clips sin voz\n\n- B (4s, cara_cerca=None) mejor_tramo=1.5s\n\n"));
        assert!(pedido.contains("- foto (imagen 1080x1920) — Cartel"));
        assert!(pedido.contains("- musica_kit (del kit de marca, 92s, 120 BPM)"));
        assert!(pedido.ends_with(&format!("## Hojas de contacto\n{}", Path::new("hojas").join("hoja_01.jpg").display())));
    }
}
