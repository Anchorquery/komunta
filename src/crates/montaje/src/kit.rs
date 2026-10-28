//! Brand kits: a client's fonts, colours, closing card, warm flare and
//! music. Read from the engine's `marcas/<id>/kit.yaml` (with the shared
//! `marcas/_comun`), the same files its settings page edits.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Error;

/// One caption look: font, size in kit pixels (on 720x1280, ASS-sized),
/// outline and margins.
#[derive(Clone, Debug, PartialEq)]
pub struct EstiloTexto {
    /// Font family as the font file names it.
    pub familia: String,
    /// Weight of that face.
    pub peso: f64,
    /// Font file, when the kit brings it.
    pub archivo: Option<PathBuf>,
    /// Font size in kit pixels (ASS size: ascent plus descent).
    pub tam: f64,
    /// Outline width in kit pixels.
    pub borde: f64,
    /// Side margin as a fraction of the frame width.
    pub margen: f64,
    /// Letter spacing in frame pixels (the engine does not scale it).
    pub espaciado: f64,
    /// Shown in capitals.
    pub mayusculas: bool,
}

/// The kit's text: four looks and two colours.
#[derive(Clone, Debug, PartialEq)]
pub struct KitTexto {
    /// Ordinary captions.
    pub normal: EstiloTexto,
    /// Emphasised captions.
    pub enfasis: EstiloTexto,
    /// Key phrases.
    pub clave: EstiloTexto,
    /// The big flash behind an emphasised caption.
    pub gigante: EstiloTexto,
    /// Fill colour, `#rrggbb`.
    pub color: String,
    /// Outline colour, `#rrggbb`.
    pub borde: String,
}

/// A brand kit, ready to lay out.
#[derive(Clone, Debug, PartialEq)]
pub struct Kit {
    /// Folder name.
    pub id: String,
    /// What the team calls it.
    pub nombre: String,
    /// Caption looks.
    pub texto: KitTexto,
    /// Warm flare over the speaker, `0..=1`; zero for none.
    pub destello: f64,
    /// The flare's video (screen-blended over the speaker).
    pub destello_video: Option<PathBuf>,
    /// Closing card per frame ("vertical", "16x9", "1x1").
    pub cierres: HashMap<String, PathBuf>,
    /// The kit's own music.
    pub musica: Option<PathBuf>,
    /// Its level under the voice.
    pub volumen_musica: f64,
}

impl Kit {
    /// The engine's defaults with no kit folder at all: its two fonts when
    /// `fuentes` has them, no card, no flare, no music.
    pub fn neutro(fuentes: &Path) -> Kit {
        let caras = caras_en(&[fuentes.to_path_buf()]);
        Kit {
            id: "neutro".to_owned(),
            nombre: "Neutro".to_owned(),
            texto: texto(&Yaml::default().texto, &caras),
            destello: 0.0,
            destello_video: None,
            cierres: HashMap::new(),
            musica: None,
            volumen_musica: 0.16,
        }
    }

    /// The kit `id` from the engine's `marcas` folder. `destello_defecto`
    /// is the engine's setting, for a kit that leaves the flare to it.
    pub fn cargar(marcas: &Path, id: &str, destello_defecto: f64) -> Result<Kit, Error> {
        let carpeta = marcas.join(id);
        let ruta = carpeta.join("kit.yaml");
        let texto_yaml =
            std::fs::read_to_string(&ruta).map_err(|e| Error::Leer(ruta.clone(), e))?;
        let datos: Yaml =
            serde_yaml::from_str(&texto_yaml).map_err(|e| Error::Kit(ruta, e.to_string()))?;
        let comun = marcas.join("_comun");
        let caras = caras_en(&[comun.join("fuentes"), carpeta.join("fuentes")]);
        // Only the kit's own files, as the engine allows.
        let archivo = |nombre: &Option<String>| {
            let nombre = nombre.as_deref()?;
            let ruta = carpeta.join(nombre);
            (ruta.is_file() && !nombre.contains("..")).then_some(ruta)
        };
        let mut cierres = HashMap::new();
        if let Some(vertical) = archivo(&datos.cierre) {
            cierres.insert("vertical".to_owned(), vertical);
        }
        for formato in ["16x9", "1x1"] {
            if let Some(ruta) = archivo(&Some(format!("cierre_{formato}.mp4"))) {
                cierres.insert(formato.to_owned(), ruta);
            }
        }
        let destello = datos.destello.unwrap_or(destello_defecto);
        let destello_video = ["destello.mp4", "destello_generado.mp4"]
            .iter()
            .map(|n| comun.join(n))
            .find(|p| p.is_file());
        Ok(Kit {
            id: id.to_owned(),
            nombre: datos.nombre.unwrap_or_else(|| id.to_owned()),
            texto: texto(&datos.texto, &caras),
            destello,
            destello_video,
            cierres,
            musica: archivo(&datos.musica),
            volumen_musica: datos.volumen_musica.unwrap_or(0.16),
        })
    }

    /// The closing card for a frame: its own, or the portrait one.
    pub fn cierre(&self, formato: &str) -> Option<&Path> {
        self.cierres
            .get(formato)
            .or_else(|| self.cierres.get("vertical"))
            .map(PathBuf::as_path)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Yaml {
    nombre: Option<String>,
    texto: YamlTexto,
    destello: Option<f64>,
    cierre: Option<String>,
    musica: Option<String>,
    volumen_musica: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct YamlTexto {
    normal: YamlEstilo,
    enfasis: YamlEstilo,
    clave: YamlEstilo,
    gigante: YamlEstilo,
    color: String,
    borde: String,
}

impl Default for YamlTexto {
    /// The engine's `TEXTO_DEFECTO`.
    fn default() -> Self {
        let estilo = |fuente: &str, tam| YamlEstilo {
            fuente: fuente.to_owned(),
            tam,
        };
        YamlTexto {
            normal: estilo("Montserrat Extra Bold", 54.0),
            enfasis: estilo("Bebas Neue", 72.0),
            clave: estilo("Montserrat Extra Bold", 80.0),
            gigante: estilo("Bebas Neue", 300.0),
            color: "#FFFFFF".to_owned(),
            borde: "#000000".to_owned(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct YamlEstilo {
    fuente: String,
    tam: f64,
}

/// A font face found on disk.
#[derive(Clone, Debug)]
struct Cara {
    familia: String,
    peso: f64,
    /// Every name the kit might use for it, squashed (see [`clave`]).
    nombres: Vec<String>,
    ruta: PathBuf,
}

/// A name with case and spaces gone: "Montserrat Extra Bold" and
/// "Montserrat ExtraBold" are the same face.
fn clave(nombre: &str) -> String {
    nombre
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every face in the font files of `carpetas`.
fn caras_en(carpetas: &[PathBuf]) -> Vec<Cara> {
    let mut caras = Vec::new();
    for carpeta in carpetas {
        let Ok(entradas) = std::fs::read_dir(carpeta) else {
            continue;
        };
        for ruta in entradas.filter_map(Result::ok).map(|e| e.path()) {
            let es_fuente = ruta
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_lowercase().as_str(), "ttf" | "otf"));
            if !es_fuente {
                continue;
            }
            let Ok(datos) = std::fs::read(&ruta) else {
                continue;
            };
            let Ok(cara) = ttf_parser::Face::parse(&datos, 0) else {
                continue;
            };
            let nombre = |id: u16| {
                cara.names()
                    .into_iter()
                    .filter(|n| n.name_id == id && n.is_unicode())
                    .find_map(|n| n.to_string())
            };
            let familia_tipografica = nombre(ttf_parser::name_id::TYPOGRAPHIC_FAMILY);
            let familia_basica = nombre(ttf_parser::name_id::FAMILY);
            let Some(familia) = familia_tipografica.clone().or(familia_basica.clone()) else {
                continue;
            };
            let subfamilia = nombre(ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY)
                .or_else(|| nombre(ttf_parser::name_id::SUBFAMILY))
                .unwrap_or_default();
            let mut nombres = vec![clave(&familia), clave(&format!("{familia} {subfamilia}"))];
            nombres.extend(nombre(ttf_parser::name_id::FULL_NAME).map(|n| clave(&n)));
            nombres.extend(familia_basica.map(|n| clave(&n)));
            caras.push(Cara {
                familia,
                peso: f64::from(cara.weight().to_number()),
                nombres,
                ruta: ruta.clone(),
            });
        }
    }
    caras
}

/// The kit's four looks, each font name matched to a face on disk when
/// one answers to it, or left as a system family otherwise.
fn texto(yaml: &YamlTexto, caras: &[Cara]) -> KitTexto {
    let estilo = |e: &YamlEstilo, borde, margen, espaciado, mayusculas| {
        let buscada = clave(&e.fuente);
        let cara = caras.iter().find(|c| c.nombres.contains(&buscada));
        EstiloTexto {
            familia: cara.map_or_else(|| e.fuente.clone(), |c| c.familia.clone()),
            peso: cara.map_or(700.0, |c| c.peso),
            archivo: cara.map(|c| c.ruta.clone()),
            tam: e.tam,
            borde,
            margen,
            espaciado,
            mayusculas,
        }
    };
    KitTexto {
        normal: estilo(&yaml.normal, 3.0, 0.153, -1.0, false),
        enfasis: estilo(&yaml.enfasis, 3.0, 0.167, 0.0, true),
        clave: estilo(&yaml.clave, 3.5, 0.083, -1.0, false),
        gigante: estilo(&yaml.gigante, 0.0, 0.0, -4.0, true),
        color: yaml.color.to_lowercase(),
        borde: yaml.borde.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_without_spaces_or_case() {
        assert_eq!(
            clave("Montserrat Extra Bold"),
            clave("Montserrat ExtraBold")
        );
        assert_eq!(clave("Bebas Neue"), "bebasneue");
    }

    #[test]
    fn a_kit_without_fonts_on_disk_keeps_the_names() {
        let kit = Kit::neutro(Path::new("no-existe"));
        assert_eq!(kit.texto.normal.familia, "Montserrat Extra Bold");
        assert_eq!(kit.texto.normal.archivo, None);
        assert!(kit.texto.enfasis.mayusculas);
    }
}
