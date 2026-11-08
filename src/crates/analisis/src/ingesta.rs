//! The first step of a montage: the material folder listed into the job's
//! `inventario.json` - each video, still, sound file and document with
//! its id - the port of the engine's `motor/ingesta.py`. A file that is
//! damaged or half copied is set aside with a reason for the team, so it
//! does not sink the whole video.
//!
//! Files are listed in the order the engine listed them, so they get the
//! same ids: by the path's components, without case on Windows.

use std::io;
use std::path::{Path, PathBuf};

use crate::inventario::{Audio, Descartado, Documento, Ids, Imagen, Inventario, Rol, Video};
use crate::{documento, foto};

/// What a probe of a media file found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Medio {
    /// Seconds; 0 when the file does not say.
    pub duracion: f64,
    /// The picture, if it has one.
    pub cuadro: Option<Cuadro>,
    /// Whether it has sound.
    pub audio: bool,
}

/// A video stream's picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cuadro {
    /// Displayed width (a quarter turn already applied).
    pub ancho: u32,
    /// Displayed height.
    pub alto: u32,
    /// Frames per second.
    pub fps: f64,
}

/// Probes a media file, or says why it cannot.
pub type Sondear<'a> = &'a dyn Fn(&Path) -> Result<Medio, String>;

/// Files the engine's drop folder keeps beside the material, not material.
const INTERNOS: [&str; 2] = ["pedido.txt", "estado.json"];

/// Lists the material folder `entrada` into `trabajo/inventario.json`.
/// Only a folder that cannot be read or a file that cannot be written is
/// an error: a file that will not open is set aside in `descartados`.
pub fn inventariar(entrada: &Path, trabajo: &Path, sondear: Sondear) -> io::Result<Inventario> {
    let mut inventario = Inventario {
        entrada: entrada.to_path_buf(),
        ..Inventario::default()
    };
    let mut ids = Ids::default();
    for ruta in archivos(entrada)? {
        let Some(tipo) = Tipo::de(&ruta) else {
            continue;
        };
        if let Err(detalle) = anotar(&mut inventario, &mut ids, tipo, ruta.clone(), sondear) {
            let archivo = nombre(&ruta);
            log::warn!("ingesta: {archivo} set aside: {detalle}");
            inventario.descartados.push(Descartado {
                motivo: tipo.motivo(),
                archivo,
            });
        }
    }
    inventario.escribir(trabajo)?;
    Ok(inventario)
}

/// Adds `guion`, a script from outside the material folder, to the
/// inventory in `trabajo` and writes it back.
pub fn anadir_guion(inventario: &mut Inventario, guion: &Path, trabajo: &Path) -> io::Result<()> {
    inventario.documentos.push(Documento {
        id: inventario.ids().nuevo(&nombre(guion)),
        ruta: guion.to_path_buf(),
        rol: Rol::Guion,
        lineas: documento::lineas(guion)?,
    });
    inventario.escribir(trabajo)
}

/// The kinds of material, by extension.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tipo {
    Video,
    Imagen,
    Audio,
    Documento,
}

impl Tipo {
    fn de(ruta: &Path) -> Option<Self> {
        let extension = ruta.extension()?.to_string_lossy().to_lowercase();
        Some(match extension.as_str() {
            "mov" | "mp4" | "m4v" | "mkv" | "avi" => Self::Video,
            "jpg" | "jpeg" | "png" | "webp" | "heic" => Self::Imagen,
            "mp3" | "m4a" | "wav" | "aac" | "ogg" | "flac" => Self::Audio,
            "docx" | "txt" | "md" => Self::Documento,
            _ => return None,
        })
    }

    /// Why a file of this kind was set aside, in the team's words, with no
    /// path or command in it: the detail goes to the log.
    fn motivo(self) -> String {
        let tipo = match self {
            Self::Video => "un vídeo",
            Self::Imagen => "una imagen",
            Self::Audio => "un audio",
            Self::Documento => "un documento",
        };
        format!("No se pudo abrir como {tipo}: está dañado, incompleto o no es {tipo}.")
    }
}

/// Adds the file at `ruta` to the inventory, or says why it cannot be.
///
/// A sound file or a document takes its id before it is opened, and keeps
/// it taken when it fails to: the engine did so, and the ids of the files
/// after it depend on it.
fn anotar(
    inventario: &mut Inventario,
    ids: &mut Ids,
    tipo: Tipo,
    ruta: PathBuf,
    sondear: Sondear,
) -> Result<(), String> {
    let nombre = nombre(&ruta);
    match tipo {
        Tipo::Video => {
            let medio = sondear(&ruta)?;
            let cuadro = medio.cuadro.ok_or("no video stream")?;
            inventario.videos.push(Video {
                id: ids.nuevo(&nombre),
                ruta,
                duracion: milesimas(medio.duracion),
                ancho: cuadro.ancho,
                alto: cuadro.alto,
                fps: milesimas(cuadro.fps),
                audio: medio.audio,
            });
        }
        Tipo::Imagen => {
            let foto = foto::abrir(&ruta)?;
            let (ancho, alto) = (foto.width(), foto.height());
            inventario.imagenes.push(Imagen {
                id: ids.nuevo(&nombre),
                ruta,
                ancho,
                alto,
            });
        }
        Tipo::Audio => {
            let id = ids.nuevo(&nombre);
            let medio = sondear(&ruta)?;
            if !medio.audio {
                return Err("no sound stream".into());
            }
            inventario.audios.push(Audio {
                id,
                ruta,
                duracion: milesimas(medio.duracion),
            });
        }
        Tipo::Documento => {
            let id = ids.nuevo(&nombre);
            let lineas = documento::lineas(&ruta).map_err(|error| error.to_string())?;
            inventario.documentos.push(Documento {
                id,
                rol: documento::rol(&nombre),
                ruta,
                lineas,
            });
        }
    }
    Ok(())
}

/// Every file under `entrada`, in the engine's order. Folders reached
/// through a link are not walked, and a subfolder that cannot be read is
/// skipped, as Python's `rglob` did; files named like the drop folder's own
/// are left out.
fn archivos(entrada: &Path) -> io::Result<Vec<PathBuf>> {
    let mut archivos = Vec::new();
    recorrer(entrada, &mut archivos, true)?;
    archivos.retain(|ruta| !INTERNOS.contains(&nombre(ruta).to_lowercase().as_str()));
    archivos.sort_by_cached_key(|ruta| clave_de_orden(ruta, entrada));
    Ok(archivos)
}

fn recorrer(carpeta: &Path, archivos: &mut Vec<PathBuf>, raiz: bool) -> io::Result<()> {
    let entradas = match std::fs::read_dir(carpeta) {
        Ok(entradas) => entradas,
        Err(error) if raiz => return Err(error),
        Err(_) => return Ok(()),
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        let Ok(tipo) = entrada.file_type() else {
            continue;
        };
        if tipo.is_dir() {
            recorrer(&ruta, archivos, false)?;
        } else if tipo.is_file() || (tipo.is_symlink() && ruta.is_file()) {
            archivos.push(ruta);
        }
    }
    Ok(())
}

/// How Python sorts paths: component by component, and on Windows without
/// case.
fn clave_de_orden(ruta: &Path, entrada: &Path) -> Vec<String> {
    ruta.strip_prefix(entrada)
        .unwrap_or(ruta)
        .components()
        .map(|c| {
            let parte = c.as_os_str().to_string_lossy();
            if cfg!(windows) {
                parte.to_lowercase()
            } else {
                parte.into_owned()
            }
        })
        .collect()
}

fn nombre(ruta: &Path) -> String {
    ruta.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Rounded to the thousandth, as the engine wrote it.
fn milesimas(valor: f64) -> f64 {
    (valor * 1000.0).round() / 1000.0
}

/// Probes a media file with the app's own FFmpeg.
#[cfg(feature = "imagen")]
pub fn sondear(ruta: &Path) -> Result<Medio, String> {
    let info = concat_media::probe(ruta).map_err(|error| error.to_string())?;
    Ok(Medio {
        duracion: info.duration.map_or(0.0, |d| d.as_f64()),
        cuadro: info.video.map(|v| Cuadro {
            ancho: v.width,
            alto: v.height,
            fps: v.frame_rate.fps().as_f64(),
        }),
        audio: info.audio.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carpeta(nombre: &str) -> PathBuf {
        let carpeta =
            std::env::temp_dir().join(format!("analisis-ingesta-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        carpeta
    }

    /// A probe that reads the "file": `video WxH fps dur [sonido]`,
    /// `audio dur`, anything else is broken.
    fn falso(ruta: &Path) -> Result<Medio, String> {
        let texto = std::fs::read_to_string(ruta).map_err(|e| e.to_string())?;
        let partes: Vec<&str> = texto.split_whitespace().collect();
        let numero = |i: usize| partes[i].parse::<f64>().unwrap();
        match partes.first() {
            Some(&"video") => {
                let (ancho, alto) = partes[1].split_once('x').unwrap();
                Ok(Medio {
                    duracion: numero(3),
                    cuadro: Some(Cuadro {
                        ancho: ancho.parse().unwrap(),
                        alto: alto.parse().unwrap(),
                        fps: numero(2),
                    }),
                    audio: partes.get(4) == Some(&"sonido"),
                })
            }
            Some(&"audio") => Ok(Medio {
                duracion: numero(1),
                cuadro: None,
                audio: true,
            }),
            _ => Err("roto".into()),
        }
    }

    #[test]
    fn the_folder_is_listed_by_kind_with_its_ids() {
        let entrada = carpeta("material");
        let trabajo = carpeta("trabajo");
        std::fs::create_dir(entrada.join("B")).unwrap();
        std::fs::write(
            entrada.join("Toma 1.MOV"),
            "video 1080x1920 29.97 12.3456 sonido",
        )
        .unwrap();
        std::fs::write(entrada.join("B").join("Toma 1.mp4"), "video 1920x1080 25 3").unwrap();
        std::fs::write(entrada.join("musica.mp3"), "audio 61.2").unwrap();
        std::fs::write(entrada.join("Gidoia.txt"), "Kaixo\n\nAgur\n").unwrap();
        crate::foto::tests::jpeg_con_exif(&entrada.join("foto.jpg"), 4, 3, 6);
        std::fs::write(entrada.join("pedido.txt"), "no es material").unwrap();
        std::fs::write(entrada.join("notas.pdf"), "otro tipo").unwrap();

        let inventario = inventariar(&entrada, &trabajo, &falso).unwrap();

        let ids: Vec<&str> = inventario.videos.iter().map(|v| v.id.as_str()).collect();
        // The folder "B" sorts before "Toma 1.MOV": the clip in it is first.
        assert_eq!(ids, ["Toma_1", "Toma_1_2"]);
        let toma = &inventario.videos[1];
        assert_eq!((toma.ancho, toma.alto, toma.fps), (1080, 1920, 29.97));
        assert_eq!(toma.duracion, 12.346);
        assert!(toma.audio && !inventario.videos[0].audio);
        assert_eq!(inventario.audios[0].duracion, 61.2);
        // Stored lying down: measured standing.
        assert_eq!(
            (inventario.imagenes[0].ancho, inventario.imagenes[0].alto),
            (3, 4)
        );
        let guion = &inventario.documentos[0];
        assert_eq!(guion.rol, Rol::Guion);
        assert_eq!(guion.lineas, ["Kaixo", "Agur"]);
        assert!(inventario.descartados.is_empty());
        assert_eq!(Inventario::leer(&trabajo).unwrap(), inventario);
    }

    #[test]
    fn a_file_that_will_not_open_is_set_aside_with_a_reason() {
        let entrada = carpeta("rotos");
        let trabajo = carpeta("rotos-trabajo");
        std::fs::write(entrada.join("a.mov"), "roto").unwrap();
        std::fs::write(entrada.join("b.mp4"), "audio 3").unwrap();
        std::fs::write(entrada.join("c.jpg"), "no es una foto").unwrap();
        std::fs::write(entrada.join("d.mov"), "video 10x10 30 1").unwrap();

        let inventario = inventariar(&entrada, &trabajo, &falso).unwrap();

        let apartados: Vec<&str> = inventario
            .descartados
            .iter()
            .map(|d| d.archivo.as_str())
            .collect();
        assert_eq!(apartados, ["a.mov", "b.mp4", "c.jpg"]);
        assert_eq!(
            inventario.descartados[2].motivo,
            "No se pudo abrir como una imagen: está dañado, incompleto o no es una imagen."
        );
        assert_eq!(inventario.videos.len(), 1);
    }

    #[test]
    fn a_broken_sound_file_keeps_its_id_taken() {
        let entrada = carpeta("audio-roto");
        let trabajo = carpeta("audio-roto-trabajo");
        std::fs::write(entrada.join("voz.mp3"), "roto").unwrap();
        std::fs::write(entrada.join("voz.wav"), "audio 2").unwrap();

        let inventario = inventariar(&entrada, &trabajo, &falso).unwrap();

        assert_eq!(inventario.audios[0].id, "voz_2");
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let trabajo = carpeta("sin-material");
        assert!(inventariar(&trabajo.join("no"), &trabajo, &falso).is_err());
    }

    #[test]
    fn a_script_from_elsewhere_joins_the_documents() {
        let entrada = carpeta("guion-fuera");
        let trabajo = carpeta("guion-fuera-trabajo");
        let otra = carpeta("guion-fuera-otra");
        std::fs::write(entrada.join("texto.txt"), "notas").unwrap();
        std::fs::write(otra.join("texto.md"), "# Guion\nHola").unwrap();
        let mut inventario = inventariar(&entrada, &trabajo, &falso).unwrap();

        anadir_guion(&mut inventario, &otra.join("texto.md"), &trabajo).unwrap();

        let guion = &Inventario::leer(&trabajo).unwrap().documentos[1];
        assert_eq!((guion.id.as_str(), guion.rol), ("texto_2", Rol::Guion));
        assert_eq!(guion.lineas, ["# Guion", "Hola"]);
    }
}
