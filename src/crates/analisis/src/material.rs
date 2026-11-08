//! What the job has besides its clips: the faces in each still, and for
//! each sound file - the brand kit's music among them - how much of it is
//! speech and where its beats fall, for cutting to it. The `imagenes` and
//! `audios` of `analisis.json`, as the engine's `motor/analisis.py`
//! writes them.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::caras::{BuscaCaras, LADO};
#[cfg(feature = "escucha")]
use crate::escucha::Oido;
use crate::foto;
use crate::inventario;
use crate::musica::Ritmo;
use crate::visual::Cara;

/// The id the brand kit's music goes by.
pub const MUSICA_KIT: &str = "musica_kit";

/// A still's entry.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImagenAnalizada {
    /// Its id in the inventory.
    pub id: String,
    /// The faces in it, most likely first.
    pub caras: Vec<Cara>,
}

/// A sound file's entry.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudioAnalizado {
    /// Its id in the inventory, or [`MUSICA_KIT`].
    pub id: String,
    /// Seconds long.
    pub duracion: f64,
    /// How much of it is speech: a voice recorded apart, rather than music.
    pub ratio_voz: f64,
    /// Its tempo and beats.
    #[serde(flatten)]
    pub ritmo: Ritmo,
    /// The file, for the kit's music, which is not in the inventory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ruta: Option<PathBuf>,
}

/// A sound file to analyse.
#[derive(Clone, Debug, PartialEq)]
pub struct Sonido {
    /// Its id.
    pub id: String,
    /// The file.
    pub ruta: PathBuf,
    /// Seconds long.
    pub duracion: f64,
}

impl From<&inventario::Audio> for Sonido {
    fn from(audio: &inventario::Audio) -> Self {
        Self {
            id: audio.id.clone(),
            ruta: audio.ruta.clone(),
            duracion: audio.duracion,
        }
    }
}

/// The faces in each of `imagenes`. With no detector, or a still that
/// will not open, there are none.
pub fn imagenes(
    mut detector: Option<&mut dyn BuscaCaras>,
    imagenes: &[inventario::Imagen],
) -> Vec<ImagenAnalizada> {
    imagenes
        .iter()
        .map(|imagen| ImagenAnalizada {
            id: imagen.id.clone(),
            caras: match detector.as_deref_mut() {
                Some(detector) => caras(detector, &imagen.ruta).unwrap_or_else(|motivo| {
                    log::warn!("{}: no faces looked for: {motivo}", imagen.id);
                    Vec::new()
                }),
                None => Vec::new(),
            },
        })
        .collect()
}

/// The faces in the still at `ruta`, shrunk first to what the detector
/// sees.
fn caras(detector: &mut dyn BuscaCaras, ruta: &Path) -> Result<Vec<Cara>, String> {
    let foto = foto::abrir(ruta)?;
    let lado = LADO as u32;
    let foto = if foto.width().max(foto.height()) > lado {
        foto.resize(lado, lado, image::imageops::FilterType::Triangle)
    } else {
        foto
    };
    let rgba = foto.to_rgba8();
    detector.caras(rgba.as_raw(), rgba.width() as usize, rgba.height() as usize)
}

/// Each of `sonidos` heard for speech and for its beats. One that will not
/// decode is left out, with a warning: it is then not offered as music.
#[cfg(feature = "escucha")]
pub fn audios(oido: &mut Oido, sonidos: &[Sonido]) -> Vec<AudioAnalizado> {
    sonidos
        .iter()
        .filter_map(|sonido| {
            uno(oido, sonido)
                .inspect_err(|motivo| log::warn!("{}: sound not analysed: {motivo}", sonido.id))
                .ok()
        })
        .collect()
}

#[cfg(feature = "escucha")]
fn uno(oido: &mut Oido, sonido: &Sonido) -> Result<AudioAnalizado, String> {
    use crate::{audio, musica, voz};

    let texto = |error: concat_media::Error| error.to_string();
    let para_voz = audio::leer(&sonido.ruta, Vec::new()).map_err(texto)?;
    let tramos = oido.voz(&para_voz).map_err(|error| error.to_string())?;
    let voz = voz::tramos(tramos, sonido.duracion);
    let para_ritmo = audio::leer_a(&sonido.ruta, Vec::new(), musica::MUESTREO).map_err(texto)?;
    Ok(AudioAnalizado {
        id: sonido.id.clone(),
        duracion: sonido.duracion,
        ratio_voz: voz::ratio(&voz, sonido.duracion),
        ritmo: Ritmo::medir(&para_ritmo),
        ruta: (sonido.id == MUSICA_KIT).then(|| sonido.ruta.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Says one face, of the picture's own size, so the test can tell what
    /// the detector was handed.
    struct Medidor;

    impl BuscaCaras for Medidor {
        fn caras(&mut self, rgba: &[u8], ancho: usize, alto: usize) -> Result<Vec<Cara>, String> {
            assert_eq!(rgba.len(), ancho * alto * 4);
            Ok(vec![Cara::from([0.0, 0.0, ancho as f64, alto as f64])])
        }
    }

    #[test]
    fn a_still_is_shrunk_for_the_detector_and_a_broken_one_has_no_faces() {
        let carpeta = std::env::temp_dir().join(format!("analisis-material-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&carpeta);
        std::fs::create_dir_all(&carpeta).unwrap();
        image::RgbImage::new(1280, 960).save(carpeta.join("grande.png")).unwrap();
        image::RgbImage::new(100, 50).save(carpeta.join("pequena.png")).unwrap();
        std::fs::write(carpeta.join("rota.png"), b"no").unwrap();
        let imagen = |id: &str| inventario::Imagen {
            id: id.into(),
            ruta: carpeta.join(format!("{id}.png")),
            ..inventario::Imagen::default()
        };
        let todas = [imagen("grande"), imagen("pequena"), imagen("rota")];

        let vistas = imagenes(Some(&mut Medidor), &todas);

        assert_eq!(vistas[0].caras, [Cara::from([0.0, 0.0, 640.0, 480.0])]);
        assert_eq!(vistas[1].caras, [Cara::from([0.0, 0.0, 100.0, 50.0])]);
        assert!(vistas[2].caras.is_empty());
        assert!(imagenes(None, &todas).iter().all(|i| i.caras.is_empty()));
    }

    #[test]
    fn a_sound_s_entry_is_the_engine_s() {
        let entrada = AudioAnalizado {
            id: MUSICA_KIT.into(),
            duracion: 61.2,
            ratio_voz: 0.0,
            ritmo: Ritmo {
                bpm: Some(120.0),
                pulsos: vec![0.5, 1.0],
            },
            ruta: Some("kit/musica.mp3".into()),
        };
        assert_eq!(
            serde_json::to_value(entrada).unwrap(),
            serde_json::json!({"id": "musica_kit", "duracion": 61.2, "ratio_voz": 0.0,
                "bpm": 120.0, "pulsos": [0.5, 1.0], "ruta": "kit/musica.mp3"})
        );
    }
}
