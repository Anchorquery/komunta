// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The agent's own tools, beyond concat-mcp's: looking at a frame, and the
//! animation recipes of `crates/animacion` - a zoom on a phrase, a title
//! that comes in with a style, a title behind the person, a title that
//! follows a face, free keys, the named moves (a pop, a shake, a slide...)
//! and a photo's parallax. Each reads the
//! project and applies the recipe's commands through a [`Lienzo`]: the
//! window when the agent runs, an editor of its own in the tests.

use animacion::movimiento::{self, Curva, Efecto, Lado as LadoEntrada, Punto};
use animacion::titulo::{self, Entrada};
use animacion::seguir::{self, Caja, Lado, Muestra};
use animacion::zoom::{self, Estilo, Zoom};
use animacion::{copia, tiempo};
use asistente::{Clase, Herramienta, Imagen, NUCLEO, Salida, Tipo};
use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{Clip, ClipKind, Cutout, CutoutMode, Project, Subject, TextStyle};
use serde_json::{Value, json};

/// What the recipes act on: the project, its edits and its frames.
pub trait Lienzo {
    /// The project as it is now.
    fn proyecto(&mut self) -> Result<Project, String>;
    /// Applies one command; the id it minted, if any.
    fn aplicar(&mut self, comando: Command) -> Result<Option<String>, String>;
    /// The frame at timeline second `tiempo`, for the AI to look at.
    fn fotograma(&mut self, tiempo: f64) -> Result<Imagen, String>;
    /// The faces in the frame at timeline second `tiempo`.
    fn caras(&mut self, tiempo: f64) -> Result<Vec<Caja>, String>;
}

/// Looks at a frame.
pub const VER_FOTOGRAMA: &str = "ver_fotograma";
const ZOOM_EN: &str = "zoom_en";
const TITULO_ANIMADO: &str = "titulo_animado";
const TEXTO_DETRAS: &str = "texto_detras";
const SEGUIR_CARA: &str = "seguir_cara";
const ANIMAR: &str = "animar";
const EFECTO: &str = "efecto";
const PARALLAX_FOTO: &str = "parallax_foto";

/// Where a title stands, as `offsetY` from the middle of the frame.
const POSICIONES: [(&str, f64); 3] = [("arriba", -0.3), ("centro", 0.0), ("abajo", 0.3)];

/// The tools, as the agent sees them.
pub fn herramientas() -> Vec<Herramienta> {
    vec![
        Herramienta::nueva(
            VER_FOTOGRAMA,
            "Mira el fotograma de la línea de tiempo en un segundo dado, tal como sale en el monitor, \
             títulos y efectos incluidos. Úsalo para comprobar lo que has editado antes de decir que está hecho.",
            json!({
                "type": "object",
                "properties": {"tiempo": {"type": "number", "minimum": 0, "description": "segundo de la línea de tiempo"}},
                "required": ["tiempo"]
            }),
            Clase::Lectura,
            NUCLEO,
        ),
        Herramienta::nueva(
            ZOOM_EN,
            "Acerca la cámara a un clip durante una frase y vuelve: suave (con rampa) o golpe (corte seco, \
             el punch-in de los vídeos de una persona hablando). Opcionalmente hacia un punto, como una cara, \
             que queda en su sitio. Reemplaza los zooms que el clip tenga en ese tramo.",
            json!({
                "type": "object",
                "properties": {
                    "clipId": {"type": "string"},
                    "desde": {"type": "number", "description": "segundo de la línea de tiempo"},
                    "hasta": {"type": "number"},
                    "factor": {"type": "number", "minimum": 1.02, "maximum": 2.5, "description": "1.15 por defecto"},
                    "estilo": {"type": "string", "enum": ["suave", "golpe"]},
                    "centro": {
                        "type": "object",
                        "description": "punto que queda fijo, 0..1 desde arriba a la izquierda",
                        "properties": {"x": {"type": "number"}, "y": {"type": "number"}},
                        "required": ["x", "y"]
                    }
                },
                "required": ["clipId", "desde", "hasta"]
            }),
            Clase::Edicion,
            "animacion",
        ),
        Herramienta::nueva(
            TITULO_ANIMADO,
            "Pone un título que entra con un estilo (aparecer, subir, escalar) y se desvanece al salir.",
            json!({
                "type": "object",
                "properties": {
                    "texto": {"type": "string"},
                    "desde": {"type": "number"},
                    "hasta": {"type": "number"},
                    "entrada": {"type": "string", "enum": Entrada::NOMBRES},
                    "posicion": {"type": "string", "enum": ["arriba", "centro", "abajo"]},
                    "tamano": {"type": "number", "description": "alto de letra como fracción del cuadro; 0.09 por defecto"},
                    "color": {"type": "string", "description": "#rrggbb"}
                },
                "required": ["texto", "desde", "hasta"]
            }),
            Clase::Edicion,
            "texto",
        ),
        Herramienta::nueva(
            TEXTO_DETRAS,
            "Pone un texto grande detrás de la persona de un clip: el texto va encima del clip y una copia \
             recortada de la persona encima del texto, solo durante el tramo. El recorte se calcula en segundo \
             plano; hasta entonces el texto se ve delante.",
            json!({
                "type": "object",
                "properties": {
                    "clipId": {"type": "string", "description": "el clip de vídeo con la persona"},
                    "texto": {"type": "string"},
                    "desde": {"type": "number"},
                    "hasta": {"type": "number"},
                    "entrada": {"type": "string", "enum": Entrada::NOMBRES},
                    "color": {"type": "string"}
                },
                "required": ["clipId", "texto", "desde", "hasta"]
            }),
            Clase::Edicion,
            "animacion",
        ),
        Herramienta::nueva(
            SEGUIR_CARA,
            "Hace que un título siga la cara más grande del cuadro: busca la cara cada 0,2 s del tramo y \
             pone el título justo encima de ella, o debajo, mientras se mueve. Reemplaza las posiciones \
             animadas que el título tenga en ese tramo.",
            json!({
                "type": "object",
                "properties": {
                    "tituloId": {"type": "string", "description": "el clip de título que se mueve"},
                    "desde": {"type": "number", "description": "por defecto, el principio del título"},
                    "hasta": {"type": "number", "description": "por defecto, su final"},
                    "lado": {"type": "string", "enum": ["encima", "debajo"]}
                },
                "required": ["tituloId"]
            }),
            Clase::Edicion,
            "animacion",
        ),
        Herramienta::nueva(
            ANIMAR,
            "Anima un clip con fotogramas clave libres: para cada propiedad, los puntos por los que pasa \
             (segundo de la línea de tiempo y valor) y la curva con la que llega a cada uno. Valores: escala \
             (1 = llena el cuadro), x e y (fracción del cuadro desde el centro; y positiva hacia abajo), giro \
             (grados, horario) y opacidad (0..1). Reemplaza las claves que el clip tenga en esas propiedades \
             entre el primer punto y el último. Para movimientos típicos usa «efecto».",
            json!({
                "type": "object",
                "properties": {
                    "clipId": {"type": "string"},
                    "pistas": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "propiedad": {"type": "string", "enum": movimiento::PROPIEDADES.map(|(n, _)| n)},
                                "puntos": {
                                    "type": "array",
                                    "minItems": 2,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "t": {"type": "number", "description": "segundo de la línea de tiempo"},
                                            "valor": {"type": "number"},
                                            "curva": {"type": "string", "enum": Curva::NOMBRES, "description": "cómo llega a este punto; lineal por defecto"},
                                            "bezier": {
                                                "type": "array", "items": {"type": "number"}, "minItems": 4, "maxItems": 4,
                                                "description": "curva propia [x1, y1, x2, y2] como en CSS, en vez de «curva»"
                                            }
                                        },
                                        "required": ["t", "valor"]
                                    }
                                }
                            },
                            "required": ["propiedad", "puntos"]
                        }
                    }
                },
                "required": ["clipId", "pistas"]
            }),
            Clase::Edicion,
            "animacion",
        ),
        Herramienta::nueva(
            EFECTO,
            "Aplica a un clip (vídeo, foto o título) un movimiento hecho: rebote (entra pequeño y se pasa), \
             sacudida (tiembla y para), pulso (late dos veces por segundo), deslizar (entra desde fuera por un \
             lado), giro (entra girando) o deriva (Ken Burns: acercamiento lento con desplazamiento). Empieza y \
             acaba en el encuadre del clip.",
            json!({
                "type": "object",
                "properties": {
                    "clipId": {"type": "string"},
                    "efecto": {"type": "string", "enum": Efecto::NOMBRES},
                    "desde": {"type": "number", "description": "por defecto, el principio del clip"},
                    "hasta": {"type": "number", "description": "por defecto, su final"},
                    "intensidad": {"type": "number", "minimum": 0.2, "maximum": 3, "description": "1 por defecto"},
                    "lado": {"type": "string", "enum": LadoEntrada::NOMBRES, "description": "para deslizar; izquierda por defecto"}
                },
                "required": ["clipId", "efecto"]
            }),
            Clase::Edicion,
            "animacion",
        ),
        Herramienta::nueva(
            PARALLAX_FOTO,
            "Da profundidad a una foto o a un plano quieto (efecto 2.5D): copia el clip encima, recorta en la \
             copia a la persona o al objeto principal, y anima el fondo y el sujeto con movimientos distintos. \
             El recorte se calcula en segundo plano; hasta entonces se ve la foto entera moviéndose.",
            json!({
                "type": "object",
                "properties": {
                    "clipId": {"type": "string"},
                    "desde": {"type": "number", "description": "por defecto, el principio del clip"},
                    "hasta": {"type": "number", "description": "por defecto, su final"},
                    "intensidad": {"type": "number", "minimum": 0.2, "maximum": 3, "description": "1 por defecto"},
                    "sujeto": {"type": "string", "enum": ["auto", "persona", "objeto"], "description": "auto por defecto"}
                },
                "required": ["clipId"]
            }),
            Clase::Edicion,
            "animacion",
        ),
    ]
}

/// Whether `nombre` is one of these tools.
pub fn es_receta(nombre: &str) -> bool {
    [VER_FOTOGRAMA, ZOOM_EN, TITULO_ANIMADO, TEXTO_DETRAS, SEGUIR_CARA, ANIMAR, EFECTO, PARALLAX_FOTO].contains(&nombre)
}

/// Runs the tool `nombre` with `argumentos` on `lienzo`.
pub fn ejecutar(lienzo: &mut dyn Lienzo, nombre: &str, argumentos: &Value) -> Salida {
    let resultado = match nombre {
        VER_FOTOGRAMA => return ver(lienzo, argumentos),
        ZOOM_EN => zoom_en(lienzo, argumentos),
        TITULO_ANIMADO => titulo_animado(lienzo, argumentos),
        TEXTO_DETRAS => texto_detras(lienzo, argumentos),
        SEGUIR_CARA => seguir_cara(lienzo, argumentos),
        ANIMAR => animar(lienzo, argumentos),
        EFECTO => efecto(lienzo, argumentos),
        PARALLAX_FOTO => parallax_foto(lienzo, argumentos),
        otra => Err(format!("no hay ninguna receta «{otra}»")),
    };
    match resultado {
        Ok(valor) => Salida::bien(valor),
        Err(motivo) => Salida::mal(Tipo::Error, motivo),
    }
}

fn ver(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Salida {
    let Some(t) = argumentos["tiempo"].as_f64() else {
        return Salida::mal(Tipo::Error, "falta «tiempo», en segundos");
    };
    match lienzo.fotograma(t) {
        Ok(imagen) => Salida::bien(json!({"tiempo": t})).con_imagen(imagen),
        Err(motivo) => Salida::mal(Tipo::Error, motivo),
    }
}

fn zoom_en(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let clip = clip_de(lienzo, argumentos)?;
    let (desde, hasta) = tramo_de(argumentos)?;
    let estilo = match argumentos["estilo"].as_str() {
        Some("golpe") => Estilo::Golpe,
        _ => Estilo::Suave,
    };
    let centro = argumentos["centro"]
        .as_object()
        .and_then(|c| Some((c.get("x")?.as_f64()?, c.get("y")?.as_f64()?)));
    let comandos = zoom::en(
        &clip,
        Zoom {
            desde,
            hasta,
            factor: argumentos["factor"].as_f64().unwrap_or(1.15),
            centro,
            estilo,
        },
    )?;
    lienzo.aplicar(Command::Batch { commands: comandos })?;
    Ok(json!({"clipId": clip.id, "desde": desde, "hasta": hasta}))
}

fn titulo_animado(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let (desde, hasta) = tramo_de(argumentos)?;
    let posicion = argumentos["posicion"].as_str().unwrap_or("abajo");
    let offset_y = POSICIONES.iter().find(|(n, _)| *n == posicion).map_or(0.3, |(_, y)| *y);
    let id = poner_titulo(lienzo, argumentos, desde, hasta, offset_y, 0.09)?;
    Ok(json!({"tituloId": id}))
}

/// The title of `argumentos` on top of everything over `desde..hasta`,
/// animated as its `entrada` says; its id.
fn poner_titulo(
    lienzo: &mut dyn Lienzo,
    argumentos: &Value,
    desde: f64,
    hasta: f64,
    offset_y: f64,
    tamano: f64,
) -> Result<String, String> {
    let texto = argumentos["texto"].as_str().map(str::trim).unwrap_or_default();
    if texto.is_empty() {
        return Err("falta «texto»".into());
    }
    let entrada = match argumentos["entrada"].as_str() {
        None => Entrada::Subir,
        Some(nombre) => Entrada::por_nombre(nombre)
            .ok_or_else(|| format!("«{nombre}» no es una entrada: {}", Entrada::NOMBRES.join(", ")))?,
    };
    let estilo = TextStyle {
        content: texto.to_owned(),
        font_size: argumentos["tamano"].as_f64().unwrap_or(tamano),
        color: argumentos["color"].as_str().unwrap_or("#ffffff").to_owned(),
        ..TextStyle::default()
    };
    let id = lienzo
        .aplicar(Command::AddTextClip {
            track_id: None,
            above: true,
            start: desde,
            style: Some(estilo),
            duration: Some(hasta - desde),
            offset_y: Some(offset_y),
        })?
        .ok_or("el título no se pudo poner")?;
    let clip = buscar(&lienzo.proyecto()?, &id)?;
    lienzo.aplicar(Command::Batch {
        commands: titulo::animar(&clip, entrada),
    })?;
    Ok(id)
}

/// The free keys of `argumentos` on its clip.
fn animar(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let clip = clip_de(lienzo, argumentos)?;
    let pistas = argumentos["pistas"].as_array().filter(|p| !p.is_empty()).ok_or("falta «pistas»")?;
    let mut comandos = Vec::new();
    let mut claves = 0;
    for pista in pistas {
        let nombre = pista["propiedad"].as_str().unwrap_or_default();
        let propiedad = movimiento::propiedad(nombre).ok_or_else(|| {
            let nombres: Vec<&str> = movimiento::PROPIEDADES.iter().map(|(n, _)| *n).collect();
            format!("«{nombre}» no es una propiedad: {}", nombres.join(", "))
        })?;
        let puntos = pista["puntos"]
            .as_array()
            .ok_or_else(|| format!("faltan los «puntos» de {nombre}"))?
            .iter()
            .map(punto_de)
            .collect::<Result<Vec<_>, String>>()?;
        claves += puntos.len();
        comandos.extend(movimiento::claves(&clip, propiedad, &puntos)?);
    }
    lienzo.aplicar(Command::Batch { commands: comandos })?;
    Ok(json!({"clipId": clip.id, "claves": claves}))
}

/// One point of a track: `t`, `valor` and its curve, named or a bezier.
fn punto_de(punto: &Value) -> Result<Punto, String> {
    let (Some(t), Some(valor)) = (punto["t"].as_f64(), punto["valor"].as_f64()) else {
        return Err("cada punto lleva «t» y «valor»".into());
    };
    let curva = match (punto["bezier"].as_array(), punto["curva"].as_str()) {
        (Some(bezier), _) => {
            let numeros: Vec<f64> = bezier.iter().filter_map(Value::as_f64).collect();
            let cuatro: [f64; 4] = numeros.try_into().map_err(|_| "«bezier» son cuatro números")?;
            Curva::Propia(cuatro)
        }
        (None, None) => Curva::Lineal,
        (None, Some(nombre)) => Curva::por_nombre(nombre)
            .ok_or_else(|| format!("«{nombre}» no es una curva: {}", Curva::NOMBRES.join(", ")))?,
    };
    Ok(Punto { t, valor, curva })
}

/// The named move of `argumentos` on its clip.
fn efecto(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let clip = clip_de(lienzo, argumentos)?;
    let nombre = argumentos["efecto"].as_str().ok_or("falta «efecto»")?;
    let lado = match argumentos["lado"].as_str() {
        None => None,
        Some(lado) => Some(
            LadoEntrada::por_nombre(lado)
                .ok_or_else(|| format!("«{lado}» no es un lado: {}", LadoEntrada::NOMBRES.join(", ")))?,
        ),
    };
    let movimiento = Efecto::por_nombre(nombre, lado)
        .ok_or_else(|| format!("«{nombre}» no es un efecto: {}", Efecto::NOMBRES.join(", ")))?;
    let (desde, hasta) = tramo_o_clip(&clip, argumentos);
    let intensidad = argumentos["intensidad"].as_f64().unwrap_or(1.0);
    let comandos = movimiento::efecto(&clip, movimiento, desde, hasta, intensidad)?;
    lienzo.aplicar(Command::Batch { commands: comandos })?;
    Ok(json!({"clipId": clip.id, "efecto": nombre, "desde": desde, "hasta": hasta}))
}

/// A photo given depth: the clip as the background, a copy above it with
/// the subject cut out, and the two keyed to move apart.
fn parallax_foto(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let clip = clip_de(lienzo, argumentos)?;
    let sujeto = match argumentos["sujeto"].as_str() {
        None | Some("auto") => Subject::Auto,
        Some("persona") => Subject::Person,
        Some("objeto") => Subject::Object,
        Some(otro) => return Err(format!("«{otro}» no es un sujeto: auto, persona, objeto")),
    };
    let (desde, hasta) = tramo_o_clip(&clip, argumentos);
    let intensidad = argumentos["intensidad"].as_f64().unwrap_or(1.0);
    let (desde, hasta) = tiempo::tramo(&clip, desde, hasta)?;
    let (copia, pista) = copia_recortada(lienzo, &clip, desde, hasta, sujeto)?;
    let proyecto = lienzo.proyecto()?;
    let (fondo, recortada) = (buscar(&proyecto, &clip.id)?, buscar(&proyecto, &copia)?);
    let comandos = movimiento::parallax(&fondo, &recortada, desde, hasta, intensidad)?;
    lienzo.aplicar(Command::Batch { commands: comandos })?;
    Ok(json!({"fondoId": clip.id, "sujetoId": copia, "pista": pista, "desde": desde, "hasta": hasta}))
}

/// The stretch `argumentos` say, or the whole of `clip`.
fn tramo_o_clip(clip: &Clip, argumentos: &Value) -> (f64, f64) {
    (
        argumentos["desde"].as_f64().unwrap_or(clip.start),
        argumentos["hasta"].as_f64().unwrap_or(clip.start + clip.duration),
    )
}

/// A copy of `clip` over `desde..hasta` on a new lane above everything,
/// muted, with `sujeto` cut out of it; the copy's id and its lane.
fn copia_recortada(
    lienzo: &mut dyn Lienzo,
    clip: &Clip,
    desde: f64,
    hasta: f64,
    sujeto: Subject,
) -> Result<(String, String), String> {
    if clip.kind != ClipKind::Video && clip.kind != ClipKind::Image {
        return Err(format!("{} no es un vídeo ni una foto: no hay nada que recortar", clip.id));
    }
    let pista = lienzo
        .aplicar(Command::AddTrack { bottom: false })?
        .ok_or("no se pudo añadir una pista")?;
    let colocada = lienzo
        .aplicar(Command::AddClip {
            media_id: clip.media_id.clone(),
            track_id: pista.clone(),
            start: 0.0,
            ripple: false,
        })?
        .ok_or("no se pudo copiar el clip")?;
    let puesta = buscar(&lienzo.proyecto()?, &colocada)?;
    let mut comandos = copia::comandos(&copia::tramo(clip, desde, hasta), &puesta, &pista, desde);
    comandos.push(Command::SetClipCutout {
        clip_id: colocada.clone(),
        cutout: Some(Cutout {
            mode: CutoutMode::Auto,
            subject: sujeto,
            ..recorte_por_defecto()
        }),
    });
    comandos.push(Command::UpdateClip {
        clip_id: colocada.clone(),
        patch: ClipPatch {
            muted: Some(true),
            ..ClipPatch::default()
        },
    });
    lienzo.aplicar(Command::Batch { commands: comandos })?;
    Ok((colocada, pista))
}

/// The title over the clip, then a copy of the person, cut out, over the
/// title: the person stands in front of the words.
fn texto_detras(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let clip = clip_de(lienzo, argumentos)?;
    if clip.kind != ClipKind::Video && clip.kind != ClipKind::Image {
        return Err(format!("{} no es un vídeo ni una foto: no hay persona que recortar", clip.id));
    }
    let (desde, hasta) = tiempo::tramo(&clip, tramo_de(argumentos)?.0, tramo_de(argumentos)?.1)?;
    let titulo = poner_titulo(lienzo, argumentos, desde, hasta, -0.05, 0.2)?;
    let (colocada, pista) = copia_recortada(lienzo, &clip, desde, hasta, Subject::Person)?;
    Ok(json!({"tituloId": titulo, "personaId": colocada, "pista": pista}))
}

/// The title of `argumentos` keyed to follow the largest face over its
/// stretch.
fn seguir_cara(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Value, String> {
    let id = argumentos["tituloId"].as_str().ok_or("falta «tituloId»")?;
    let titulo = buscar(&lienzo.proyecto()?, id)?;
    if titulo.kind != ClipKind::Text {
        return Err(format!("{id} no es un título"));
    }
    let desde = argumentos["desde"].as_f64().unwrap_or(titulo.start);
    let hasta = argumentos["hasta"].as_f64().unwrap_or(titulo.start + titulo.duration);
    let lado = match argumentos["lado"].as_str() {
        None => Lado::Encima,
        Some(nombre) => Lado::por_nombre(nombre).ok_or_else(|| format!("«{nombre}» no es un lado: encima, debajo"))?,
    };
    let muestras = seguir::instantes(desde, hasta)
        .into_iter()
        .map(|t| Ok(Muestra { t, cara: seguir::mayor(&lienzo.caras(t)?) }))
        .collect::<Result<Vec<_>, String>>()?;
    let comandos = seguir::claves(&titulo, desde, hasta, &muestras, lado)?;
    lienzo.aplicar(Command::Batch { commands: comandos })?;
    let con_cara = muestras.iter().filter(|m| m.cara.is_some()).count();
    Ok(json!({"tituloId": id, "miradas": muestras.len(), "conCara": con_cara}))
}

/// A cutout as the document reads one with nothing but a mode: its
/// feather the default.
fn recorte_por_defecto() -> Cutout {
    serde_json::from_value(json!({"mode": "auto"})).expect("a cutout reads from its mode alone")
}

fn clip_de(lienzo: &mut dyn Lienzo, argumentos: &Value) -> Result<Clip, String> {
    let id = argumentos["clipId"].as_str().ok_or("falta «clipId»")?;
    buscar(&lienzo.proyecto()?, id)
}

fn buscar(proyecto: &Project, id: &str) -> Result<Clip, String> {
    proyecto
        .active()
        .clip(id)
        .cloned()
        .ok_or_else(|| format!("no hay ningún clip {id} en la línea de tiempo"))
}

fn tramo_de(argumentos: &Value) -> Result<(f64, f64), String> {
    match (argumentos["desde"].as_f64(), argumentos["hasta"].as_f64()) {
        (Some(desde), Some(hasta)) if hasta > desde && desde >= 0.0 => Ok((desde, hasta)),
        (Some(desde), Some(hasta)) => Err(format!("el tramo {desde}–{hasta} s no tiene duración")),
        _ => Err("faltan «desde» y «hasta», en segundos".into()),
    }
}

#[cfg(test)]
mod tests {
    use concat_project::Editor;
    use concat_project::commands::NewMedia;
    use concat_project::model::MediaKind;

    use super::*;

    /// An editor of its own, standing in for the window.
    struct Mesa(Editor);

    impl Lienzo for Mesa {
        fn proyecto(&mut self) -> Result<Project, String> {
            Ok(self.0.project().clone())
        }

        fn aplicar(&mut self, comando: Command) -> Result<Option<String>, String> {
            self.0.apply(comando).map(|o| o.created_id).map_err(|e| e.to_string())
        }

        fn fotograma(&mut self, tiempo: f64) -> Result<Imagen, String> {
            Ok(Imagen {
                mime: "image/jpeg".into(),
                datos: vec![0xff, 0xd8],
                nota: format!("t={tiempo} s"),
            })
        }

        /// A face that walks left to right over the ten seconds.
        fn caras(&mut self, tiempo: f64) -> Result<Vec<Caja>, String> {
            Ok(vec![Caja {
                x: tiempo / 20.0,
                y: 0.4,
                ancho: 0.2,
                alto: 0.25,
            }])
        }
    }

    /// A ten-second video on the first lane; the table and the clip's id.
    fn mesa() -> (Mesa, String) {
        let mut editor = Editor::new();
        let medio = editor
            .apply(Command::AddMedia {
                item: NewMedia {
                    path: "/toma.mp4".into(),
                    name: "toma.mp4".into(),
                    duration: Some(10.0),
                    kind: MediaKind::Video,
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some(30.0),
                    frame_rate_fraction: Some("30/1".into()),
                    video_codec: Some("h264".into()),
                    audio_codec: Some("aac".into()),
                    has_audio: true,
                    audio_tracks: Vec::new(),
                    origin: None,
                    color_space: Default::default(),
                },
            })
            .unwrap()
            .created_id
            .unwrap();
        let pista = editor.project().active().tracks[0].id.clone();
        let clip = editor
            .apply(Command::AddClip {
                media_id: medio,
                track_id: pista,
                start: 0.0,
                ripple: false,
            })
            .unwrap()
            .created_id
            .unwrap();
        (Mesa(editor), clip)
    }

    #[test]
    fn every_recipe_has_a_schema_and_is_known_by_name() {
        for herramienta in herramientas() {
            assert!(es_receta(&herramienta.nombre), "{}", herramienta.nombre);
            assert_eq!(herramienta.esquema["type"], "object");
        }
        assert!(!es_receta("clips_split"));
    }

    #[test]
    fn a_zoom_keys_the_clip() {
        let (mut mesa, clip) = mesa();
        let salida = ejecutar(&mut mesa, ZOOM_EN, &json!({"clipId": clip, "desde": 2.0, "hasta": 4.0}));
        assert!(salida.ok(), "{:?}", salida.valor);
        assert_eq!(mesa.0.project().active().clip(&clip).unwrap().keys.len(), 4);
        let mal = ejecutar(&mut mesa, ZOOM_EN, &json!({"clipId": "c99", "desde": 2.0, "hasta": 4.0}));
        assert!(mal.valor.as_str().unwrap().contains("c99"));
    }

    #[test]
    fn an_animated_title_lands_on_top_and_rises_into_place() {
        let (mut mesa, _) = mesa();
        let salida = ejecutar(
            &mut mesa,
            TITULO_ANIMADO,
            &json!({"texto": "Hola", "desde": 1.0, "hasta": 3.0, "entrada": "subir", "posicion": "arriba"}),
        );
        assert!(salida.ok(), "{:?}", salida.valor);
        let id = salida.valor["tituloId"].as_str().unwrap();
        let titulo = mesa.0.project().active().clip(id).unwrap().clone();
        assert_eq!(titulo.kind, ClipKind::Text);
        assert!((titulo.offset_y + 0.3).abs() < 1e-9);
        assert_eq!(titulo.keys.len(), 2);
        assert!(titulo.fade_in > 0.0);
    }

    #[test]
    fn a_title_behind_the_person_is_the_title_then_the_person_cut_out_above_it() {
        let (mut mesa, clip) = mesa();
        let salida = ejecutar(
            &mut mesa,
            TEXTO_DETRAS,
            &json!({"clipId": clip, "texto": "WOW", "desde": 2.0, "hasta": 5.0}),
        );
        assert!(salida.ok(), "{:?}", salida.valor);
        let linea = mesa.0.project().active().clone();
        let titulo = linea.clip(salida.valor["tituloId"].as_str().unwrap()).unwrap();
        let persona = linea.clip(salida.valor["personaId"].as_str().unwrap()).unwrap();
        let fila = |pista: &str| linea.tracks.iter().position(|t| t.id == pista).unwrap();
        let original = linea.clip(&clip).unwrap();
        // Lanes are listed bottom first.
        assert!(fila(&original.track_id) < fila(&titulo.track_id));
        assert!(fila(&titulo.track_id) < fila(&persona.track_id));
        assert_eq!((persona.start, persona.duration, persona.source_start), (2.0, 3.0, 2.0));
        assert_eq!(persona.cutout.as_ref().unwrap().subject, Subject::Person);
        assert_eq!(persona.muted, Some(true));
    }

    #[test]
    fn a_title_follows_the_face_over_it() {
        let (mut mesa, clip) = mesa();
        let titulo = ejecutar(
            &mut mesa,
            TITULO_ANIMADO,
            &json!({"texto": "Ane", "desde": 2.0, "hasta": 4.0, "entrada": "aparecer"}),
        );
        let id = titulo.valor["tituloId"].as_str().unwrap().to_owned();
        let salida = ejecutar(&mut mesa, SEGUIR_CARA, &json!({"tituloId": id}));
        assert!(salida.ok(), "{:?}", salida.valor);
        assert_eq!(salida.valor["conCara"], salida.valor["miradas"]);
        let titulo = mesa.0.project().active().clip(&id).unwrap().clone();
        let xs: Vec<f64> = titulo
            .keys
            .iter()
            .filter(|k| k.property == concat_project::model::KeyProperty::OffsetX)
            .map(|k| k.value)
            .collect();
        assert!(xs.len() > 5 && xs.windows(2).all(|par| par[1] >= par[0]), "{xs:?}");
        let no = ejecutar(&mut mesa, SEGUIR_CARA, &json!({"tituloId": clip}));
        assert!(no.valor.as_str().unwrap().contains("no es un título"));
    }

    #[test]
    fn free_keys_move_the_clip_through_its_points() {
        let (mut mesa, clip) = mesa();
        let salida = ejecutar(
            &mut mesa,
            ANIMAR,
            &json!({"clipId": clip, "pistas": [
                {"propiedad": "giro", "puntos": [{"t": 1, "valor": 0}, {"t": 2, "valor": 30, "curva": "rebote"}]},
                {"propiedad": "x", "puntos": [{"t": 1, "valor": 0}, {"t": 3, "valor": 0.2, "bezier": [0.2, 0, 0.2, 1]}]}
            ]}),
        );
        assert!(salida.ok(), "{:?}", salida.valor);
        assert_eq!(salida.valor["claves"], 4);
        assert_eq!(mesa.0.project().active().clip(&clip).unwrap().keys.len(), 4);
        let mal = ejecutar(
            &mut mesa,
            ANIMAR,
            &json!({"clipId": clip, "pistas": [{"propiedad": "brillo", "puntos": []}]}),
        );
        assert!(mal.valor.as_str().unwrap().contains("brillo"));
    }

    #[test]
    fn a_named_move_covers_the_whole_clip_when_no_stretch_is_said() {
        let (mut mesa, clip) = mesa();
        let salida = ejecutar(&mut mesa, EFECTO, &json!({"clipId": clip, "efecto": "deriva"}));
        assert!(salida.ok(), "{:?}", salida.valor);
        assert_eq!((salida.valor["desde"].as_f64(), salida.valor["hasta"].as_f64()), (Some(0.0), Some(10.0)));
        let mal = ejecutar(&mut mesa, EFECTO, &json!({"clipId": clip, "efecto": "volar"}));
        assert!(mal.valor.as_str().unwrap().contains("volar"));
    }

    #[test]
    fn a_parallax_is_the_photo_below_and_its_subject_cut_out_above_moving_apart() {
        let (mut mesa, clip) = mesa();
        let salida = ejecutar(
            &mut mesa,
            PARALLAX_FOTO,
            &json!({"clipId": clip, "desde": 0, "hasta": 4, "sujeto": "objeto"}),
        );
        assert!(salida.ok(), "{:?}", salida.valor);
        let linea = mesa.0.project().active().clone();
        let fondo = linea.clip(&clip).unwrap();
        let sujeto = linea.clip(salida.valor["sujetoId"].as_str().unwrap()).unwrap();
        let fila = |pista: &str| linea.tracks.iter().position(|t| t.id == pista).unwrap();
        assert!(fila(&fondo.track_id) < fila(&sujeto.track_id));
        assert_eq!(sujeto.cutout.as_ref().unwrap().subject, Subject::Object);
        assert!(!fondo.keys.is_empty() && !sujeto.keys.is_empty());
    }

    #[test]
    fn a_frame_is_shown_to_the_ai_as_a_picture() {
        let (mut mesa, _) = mesa();
        let salida = ejecutar(&mut mesa, VER_FOTOGRAMA, &json!({"tiempo": 2.5}));
        assert_eq!(salida.imagen.unwrap().nota, "t=2.5 s");
        assert!(!ejecutar(&mut mesa, VER_FOTOGRAMA, &json!({})).ok());
    }
}
