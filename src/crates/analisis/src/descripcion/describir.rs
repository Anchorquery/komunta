//! A job's material described: what no AI has described yet in its
//! `analisis.json` goes to the first provider there is - Gemini with a
//! contact video, Claude with strips of frames - and what one cannot do the
//! next tries. Never an error for the montage: without descriptions the
//! planner still has the contact sheets. The engine's `describir`.

use std::io;
use std::path::Path;

use serde_json::Value;

use super::anthropic::{self, Claude, ErrorClaude};
use super::contacto;
use super::cuota::Cuota;
use super::google::{self, ErrorGemini, Gemini};
use super::respuesta::{self, Tramo};
use super::{Descripcion, Pendiente, Tipo, lotes, numero, pendientes, redondear, tiras};
use crate::cache::Cache;
use crate::inventario::Inventario;
use crate::trabajo::Archivo;

/// Requests left for the plan, its correction and the second pass: when
/// Gemini's quota would not leave them, the description is skipped.
pub const RESERVA_PLAN: u32 = 4;

/// How long a model may take to describe a batch before the next one is
/// asked: a batch takes Gemini well under a minute.
const ESPERA_GEMINI: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Who describes, with what.
#[derive(Clone, Debug, PartialEq)]
pub enum Proveedor {
    /// Gemini, by Google.
    Google {
        /// The API key.
        clave: String,
        /// The model and, after it, the ones to fall back to: each has a
        /// daily quota of its own.
        modelos: Vec<String>,
        /// The requests a day the team says its key has, when Google has
        /// not said.
        limite_diario: Option<u32>,
    },
    /// Claude, by Anthropic.
    Anthropic {
        /// The API key.
        clave: String,
        /// The model.
        modelo: String,
    },
}

impl Proveedor {
    /// The engine's name for it.
    pub fn id(&self) -> &'static str {
        match self {
            Proveedor::Google { .. } => "google",
            Proveedor::Anthropic { .. } => "anthropic",
        }
    }
}

/// What a job's description needs.
pub struct Encargo<'a> {
    /// The job's folder.
    pub trabajo: &'a Path,
    /// Who describes, in order.
    pub proveedores: Vec<Proveedor>,
    /// Where what is described is kept for other jobs.
    pub cache: Option<&'a Cache>,
    /// Gemini's quota.
    pub cuota: Cuota,
}

/// How a description went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resumen {
    /// Clips and stills described.
    pub descritos: usize,
    /// Left without a description.
    pub fallos: usize,
    /// Requests that got an answer.
    pub peticiones: usize,
    /// The last thing that went wrong, when something is left.
    pub error: Option<String>,
    /// The providers that described something.
    pub proveedores: Vec<&'static str>,
}

/// What one provider did.
#[derive(Debug, Default)]
struct Parcial {
    descritos: usize,
    peticiones: usize,
    error: Option<String>,
}

/// Describes what the job in `encargo` lacks, telling `avisar` what it
/// does. Only reading or writing the job's files is an error.
pub fn describir(encargo: &Encargo, mut avisar: impl FnMut(&str)) -> io::Result<Resumen> {
    let inventario = Inventario::leer(encargo.trabajo)?;
    let mut resumen = Resumen::default();
    for proveedor in &encargo.proveedores {
        let mut archivo = Archivo::abrir(encargo.trabajo)?;
        let pendientes = pendientes(archivo.datos(), &inventario);
        if pendientes.is_empty() {
            break;
        }
        let parcial = match proveedor {
            Proveedor::Google { clave, modelos, limite_diario } => {
                let gemini = Gemini::new(clave.clone(), ESPERA_GEMINI);
                con_gemini(encargo, &gemini, modelos, *limite_diario, pendientes, &mut archivo, &mut avisar)?
            }
            Proveedor::Anthropic { clave, modelo } => {
                let claude = Claude::new(clave.clone(), modelo.clone());
                con_claude(encargo, &claude, pendientes, &mut archivo, &mut avisar)?
            }
        };
        if let Some(error) = &parcial.error {
            avisar(&format!("No se pudo describir el material: {}", recorte(error, 160)));
        }
        resumen.descritos += parcial.descritos;
        resumen.peticiones += parcial.peticiones;
        if parcial.descritos > 0 {
            resumen.proveedores.push(proveedor.id());
        }
        resumen.error = parcial.error.or(resumen.error);
    }
    let archivo = Archivo::abrir(encargo.trabajo)?;
    resumen.fallos = pendientes(archivo.datos(), &inventario).len();
    if resumen.fallos == 0 {
        resumen.error = None;
    }
    Ok(resumen)
}

/// Gemini watching contact videos of up to 15 minutes, one request each.
fn con_gemini(
    encargo: &Encargo,
    gemini: &Gemini,
    modelos: &[String],
    limite_diario: Option<u32>,
    pendientes: Vec<Pendiente>,
    archivo: &mut Archivo,
    avisar: &mut impl FnMut(&str),
) -> io::Result<Parcial> {
    let lotes = lotes(pendientes);
    let mut parcial = Parcial::default();
    if let Some(error) = sin_cuota(&encargo.cuota, modelos, limite_diario, lotes.len()) {
        parcial.error = Some(error);
        return Ok(parcial);
    }
    let carpeta = encargo.trabajo.join("ia").join("contacto");
    let de = lotes.len();
    for (n, lote) in lotes.into_iter().enumerate() {
        let parte = if de > 1 { format!(", parte {} de {de}", n + 1) } else { String::new() };
        avisar(&format!("La IA mira el material ({} clips{parte})", lote.len()));
        let respuesta = mirar_lote(encargo, gemini, modelos, &lote, &carpeta.join(format!("contacto_{}.mp4", n + 1)));
        let (contacto, texto) = match respuesta {
            Ok(respuesta) => respuesta,
            Err(error) => {
                parcial.error = Some(error.to_string());
                if matches!(error, Fallido::Gemini(ErrorGemini::CuotaAgotada(_))) {
                    break;
                }
                continue;
            }
        };
        parcial.peticiones += 1;
        let entradas = respuesta::extraer_json(&texto).map(|j| respuesta::por_id(&j)).unwrap_or_default();
        let mut descripciones = Vec::new();
        for (pendiente, (_, tramo)) in lote.iter().zip(&contacto.tabla) {
            let Some(entrada) = entradas.get(&pendiente.id) else {
                continue;
            };
            let visto = pendiente.segundos_en_contacto();
            let largo = match pendiente.tipo {
                Tipo::Clip { duracion, .. } => duracion,
                Tipo::Foto => visto,
            };
            let mut descripcion = respuesta::descripcion(entrada, *tramo, largo);
            if matches!(pendiente.tipo, Tipo::Clip { .. }) && visto < largo {
                descripcion.visto_hasta = Some(visto);
            }
            descripciones.push((pendiente.id.clone(), descripcion));
        }
        parcial.descritos += guardar(encargo, archivo, descripciones)?;
    }
    let _ = std::fs::remove_dir_all(&carpeta);
    Ok(parcial)
}

/// Why a batch got no answer.
#[derive(Debug)]
enum Fallido {
    Contacto(String),
    Gemini(ErrorGemini),
}

impl std::fmt::Display for Fallido {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fallido::Contacto(error) => write!(f, "vídeo de contacto: {error}"),
            Fallido::Gemini(error) => error.fmt(f),
        }
    }
}

/// The contact video of `lote` at `ruta`, and what Gemini said of it.
fn mirar_lote(
    encargo: &Encargo,
    gemini: &Gemini,
    modelos: &[String],
    lote: &[Pendiente],
    ruta: &Path,
) -> Result<(contacto::Contacto, String), Fallido> {
    let inicio = std::time::Instant::now();
    let contacto = contacto::video(lote, ruta).map_err(|e| Fallido::Contacto(e.to_string()))?;
    log::info!("contact video written in {:.1} s", inicio.elapsed().as_secs_f64());
    let video = gemini
        .parte_de_video(&contacto.ruta, contacto::FPS)
        .map_err(|f| Fallido::Gemini(ErrorGemini::Otro(f.texto)))?;
    log::info!("contact video sent in {:.1} s", inicio.elapsed().as_secs_f64());
    let tabla: Vec<String> = contacto
        .tabla
        .iter()
        .map(|(id, tramo)| format!("- {id}: {}–{} s", numero(tramo.desde), numero(tramo.hasta)))
        .collect();
    let instruccion = serde_json::json!({"text": INSTRUCCION_VIDEO.replace("{tabla}", &tabla.join("\n"))});
    let partes = [video, instruccion];
    let texto = google::con_reintentos(&encargo.cuota, modelos, |modelo| gemini.generar(modelo, &partes), std::thread::sleep)
        .map_err(Fallido::Gemini)?;
    Ok((contacto, texto))
}

/// Why the description is skipped to spare Gemini's quota, if it is: the
/// models known to be limited would not have `lotes` requests left and
/// [`RESERVA_PLAN`] more for the plan.
fn sin_cuota(cuota: &Cuota, modelos: &[String], limite_diario: Option<u32>, lotes: usize) -> Option<String> {
    let quedan: Option<Vec<u32>> = modelos.iter().map(|m| cuota.restantes(m, limite_diario)).collect();
    let total: u32 = quedan?.iter().sum();
    (total < lotes as u32 + RESERVA_PLAN).then(|| {
        format!(
            "Quedan pocas peticiones de Gemini hoy ({total}); se guardan para el plan y la descripción se omite \
             (se hará en el próximo vídeo con este material)."
        )
    })
}

/// Claude looking at sheets of strips, up to [`anthropic::HOJAS_POR_PETICION`]
/// a request.
fn con_claude(
    encargo: &Encargo,
    claude: &Claude,
    pendientes: Vec<Pendiente>,
    archivo: &mut Archivo,
    avisar: &mut impl FnMut(&str),
) -> io::Result<Parcial> {
    avisar(&format!("Claude mira el material ({} clips en fotogramas)", pendientes.len()));
    let carpeta = encargo.trabajo.join("ia").join("tiras");
    let hojas = tiras::hojas(&pendientes, &carpeta)?;
    let por_peticion = anthropic::HOJAS_POR_PETICION * tiras::FILAS_HOJA;
    let mut parcial = Parcial::default();
    for (hojas, grupo) in hojas.chunks(anthropic::HOJAS_POR_PETICION).zip(pendientes.chunks(por_peticion)) {
        let texto = match claude.describir(hojas, &instruccion_imagenes(grupo)) {
            Ok(texto) => texto,
            Err(error @ (ErrorClaude::Rechazo | ErrorClaude::Cortado)) => {
                parcial.error = Some(error.to_string());
                continue;
            }
            Err(error) => {
                parcial.error = Some(error.to_string());
                break;
            }
        };
        parcial.peticiones += 1;
        let entradas = respuesta::extraer_json(&texto).map(|j| respuesta::por_id(&j)).unwrap_or_default();
        let descripciones = grupo
            .iter()
            .filter_map(|pendiente| {
                let entrada = entradas.get(&pendiente.id)?;
                let largo = pendiente.duracion();
                let mut descripcion = respuesta::descripcion(entrada, Tramo { desde: 0.0, hasta: largo }, largo);
                descripcion.visto_por = Some("fotogramas".into());
                Some((pendiente.id.clone(), descripcion))
            })
            .collect();
        parcial.descritos += guardar(encargo, archivo, descripciones)?;
    }
    let _ = std::fs::remove_dir_all(&carpeta);
    Ok(parcial)
}

fn instruccion_imagenes(grupo: &[Pendiente]) -> String {
    let tabla: Vec<String> = grupo
        .iter()
        .map(|p| match p.tipo {
            Tipo::Foto => format!("- {}: foto", p.id),
            Tipo::Clip { duracion, .. } => format!("- {}: {} s", p.id, numero(redondear(duracion, 1))),
        })
        .collect();
    INSTRUCCION_IMAGENES.replace("{tabla}", &tabla.join("\n"))
}

/// Writes `descripciones` into the job's `analisis.json`, and the clips'
/// entries into the cache; returns how many there were.
fn guardar(encargo: &Encargo, archivo: &mut Archivo, descripciones: Vec<(String, Descripcion)>) -> io::Result<usize> {
    let cuantas = descripciones.len();
    let valores = descripciones
        .into_iter()
        .map(|(id, d)| (id, serde_json::to_value(d).unwrap_or(Value::Null)))
        .collect();
    let clips = archivo.describir(valores)?;
    if let Some(cache) = encargo.cache {
        for clip in &clips {
            if let Err(error) = cache.guardar(encargo.trabajo, clip) {
                log::warn!("description not cached: {error}");
            }
        }
    }
    Ok(cuantas)
}

fn recorte(texto: &str, largo: usize) -> String {
    texto.chars().take(largo).collect()
}

/// What Gemini is asked of a contact video; `{tabla}` lists each id's
/// seconds in it.
const INSTRUCCION_VIDEO: &str = r#"Eres ayudante de un editor de vídeo. El vídeo adjunto es una sucesión de clips y fotos del material de un vídeo
(sin audio, en baja resolución). Arriba de cada tramo está escrito su id. Esta es la tabla de tramos, en segundos
del vídeo adjunto:
{tabla}

Para CADA id de la tabla, describe en español lo que se ve, pensando en si sirve como recurso (plano de apoyo) o
es una persona hablando a cámara. Sé concreto (lugares, objetos, carteles legibles, número de personas, acción).

Devuelve SOLO un JSON: {"clips": [{
  "id": "<id exacto de la tabla>",
  "descripcion": "frase de 10-25 palabras",
  "plano": "general | medio | primer_plano | detalle",
  "etiquetas": ["3-6 palabras clave: lugar, personas, objetos, acción"],
  "momentos": [{"desde": 12.0, "hasta": 14.5, "que": "lo que pasa"}],  // 1-3 tramos útiles, en SEGUNDOS DEL VÍDEO ADJUNTO
  "problemas": ["movido", "desenfocado", "oscuro", "contraluz", "dedo", "cámara buscando"],  // o []
  "uso": "toma_a_camara | recurso | no_usar"
}]}
"#;

/// What Claude is asked of the strips; `{tabla}` lists each id's length.
const INSTRUCCION_IMAGENES: &str = r#"Eres ayudante de un editor de vídeo. Las imágenes adjuntas son hojas con el material de un vídeo: cada fila es un
clip (su id escrito arriba en amarillo) con 5 fotogramas repartidos por el clip y el segundo de cada uno debajo; las
fotos sueltas ocupan una fila con una sola imagen. Clips de la tabla (id: duración):
{tabla}

Para CADA id, describe en español lo que se ve, pensando en si sirve como recurso (plano de apoyo) o es una persona
hablando a cámara. Sé concreto (lugares, objetos, carteles legibles, número de personas, acción).

Devuelve SOLO un JSON: {"clips": [{
  "id": "<id exacto>",
  "descripcion": "frase de 10-25 palabras",
  "plano": "general | medio | primer_plano | detalle",
  "etiquetas": ["3-6 palabras clave"],
  "momentos": [{"desde": 1.0, "hasta": 3.0, "que": "lo que pasa"}],  // 1-3 tramos útiles, en segundos DEL CLIP (los de los fotogramas)
  "problemas": ["movido", "desenfocado", "oscuro", "contraluz", "dedo", "cámara buscando"],  // o []
  "uso": "toma_a_camara | recurso | no_usar"
}]}
"#;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn claude_is_told_each_row_s_length() {
        let grupo = [
            Pendiente { id: "A".into(), ruta: PathBuf::new(), tipo: Tipo::Clip { duracion: 12.0, con_voz: false } },
            Pendiente { id: "F".into(), ruta: PathBuf::new(), tipo: Tipo::Foto },
        ];
        let texto = instruccion_imagenes(&grupo);
        assert!(texto.contains("(id: duración):\n- A: 12.0 s\n- F: foto\n\nPara CADA id"));
    }

    #[test]
    fn gemini_is_spared_when_the_plan_would_be_left_without_requests() {
        let carpeta = std::env::temp_dir().join(format!("analisis-reserva-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        let cuota = Cuota::en(carpeta);
        let modelos = ["flash".to_owned(), "pro".to_owned()];
        assert!(sin_cuota(&cuota, &modelos, None, 3).is_none());
        assert!(sin_cuota(&cuota, &modelos, Some(3), 2).is_none());
        assert!(sin_cuota(&cuota, &modelos, Some(2), 1).is_some());
    }
}
