//! A [`Linea`] laid out as an editable project: the voice line on the
//! bottom lane, B-roll above it, captions above that, the emphasis flashes
//! above those and the music at the top. Every piece stays a plain clip,
//! so the editor can move, trim and restyle all of it.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use concat_project::Project;
use concat_project::model::{
    Clip, ClipKey, ClipKind, CustomFont, Framing, KeyEase, KeyProperty, MediaItem, MediaKind,
    TextAlign, TextStyle, Track, VideoSettings, framing_scale,
};

use crate::entrada::Trabajo;
use crate::linea::Linea;

/// Caption heights as fractions of the frame, for a portrait frame (and
/// for a landscape or square one), from the engine's `POSICIONES`.
fn posicion(nombre: &str, vertical: bool) -> f64 {
    let (arriba, centro, abajo, auto) = if vertical {
        (0.20, 0.50, 0.78, 0.60)
    } else {
        (0.14, 0.50, 0.86, 0.80)
    };
    match nombre {
        "arriba" => arriba,
        "centro" => centro,
        "abajo" => abajo,
        _ => auto,
    }
}

/// One caption look from the brand kit. Sizes are pixels on a 720x1280
/// frame, as the kit writes them.
#[derive(Clone, Debug)]
pub struct EstiloTexto {
    /// Font family as the font file names it.
    pub familia: String,
    /// Weight of that face.
    pub peso: f64,
    /// Font size in kit pixels (ASS size: ascent plus descent).
    pub tam: f64,
    /// Outline width in kit pixels.
    pub borde: f64,
    /// Side margin as a fraction of the frame width.
    pub margen: f64,
    /// Shown in capitals.
    pub mayusculas: bool,
}

/// The brand kit's text: the four looks, colours and where the fonts are.
#[derive(Clone, Debug)]
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
    /// Font files the looks name: (family, path).
    pub fuentes: Vec<(String, String)>,
}

impl KitTexto {
    /// The engine's default kit (`TEXTO_DEFECTO`), with its two fonts in
    /// `fuentes` (the engine's `marcas/_comun/fuentes`).
    pub fn neutro(fuentes: &Path) -> KitTexto {
        let estilo = |familia: &str, peso, tam, borde, margen, mayusculas| EstiloTexto {
            familia: familia.to_owned(),
            peso,
            tam,
            borde,
            margen,
            mayusculas,
        };
        let ruta = |archivo: &str| fuentes.join(archivo).to_string_lossy().into_owned();
        KitTexto {
            normal: estilo("Montserrat", 800.0, 54.0, 3.0, 0.153, false),
            enfasis: estilo("Bebas Neue", 400.0, 72.0, 3.0, 0.167, true),
            clave: estilo("Montserrat", 800.0, 80.0, 3.5, 0.083, false),
            gigante: estilo("Bebas Neue", 400.0, 300.0, 0.0, 0.0, true),
            color: "#ffffff".to_owned(),
            borde: "#000000".to_owned(),
            fuentes: vec![
                ("Montserrat".to_owned(), ruta("Montserrat-ExtraBold.otf")),
                ("Bebas Neue".to_owned(), ruta("BebasNeue-Regular.ttf")),
            ],
        }
    }
}

/// How the project is framed.
#[derive(Clone, Debug)]
pub struct Opciones {
    /// Frame width in pixels.
    pub ancho: u32,
    /// Frame height in pixels.
    pub alto: u32,
    /// Frames per second.
    pub fps: u32,
    /// Caption looks.
    pub texto: KitTexto,
}

/// Lanes, bottom first.
const PISTAS: usize = 5;
const VOZ: usize = 0;
const RECURSOS: usize = 1;
const SUBTITULOS: usize = 2;
const DESTELLOS: usize = 3;
const MUSICA: usize = 4;

/// Ids the way the editor mints them: one counter shared by every prefix.
struct Ids(u64);

impl Ids {
    fn next(&mut self, prefijo: &str) -> String {
        self.0 += 1;
        format!("{prefijo}{}", self.0)
    }
}

/// The bin: every file the timeline uses, once, with its picture size.
#[derive(Default)]
struct Bin {
    media: Vec<MediaItem>,
    por_clip: HashMap<String, (String, (f64, f64))>,
}

impl Bin {
    /// The media id for inventory id `id`, adding it on first use.
    fn medio(
        &mut self,
        ids: &mut Ids,
        trabajo: &Trabajo,
        id: &str,
        ruta: &str,
    ) -> (String, (f64, f64)) {
        if let Some(hecho) = self.por_clip.get(id) {
            return hecho.clone();
        }
        let inv = &trabajo.inventario;
        let nombre = Path::new(ruta)
            .file_name()
            .map_or_else(|| id.to_owned(), |n| n.to_string_lossy().into_owned());
        let mut item = MediaItem {
            id: ids.next("m"),
            path: ruta.to_owned(),
            name: nombre,
            ..MediaItem::default()
        };
        if let Some(v) = inv.videos.iter().find(|v| v.id == id) {
            item.kind = MediaKind::Video;
            item.duration = Some(v.duracion);
            item.width = Some(v.ancho);
            item.height = Some(v.alto);
            item.frame_rate = Some(v.fps);
            item.frame_rate_fraction = Some(fraccion(v.fps));
            item.has_audio = v.audio.unwrap_or(true);
        } else if let Some(i) = inv.imagenes.iter().find(|i| i.id == id) {
            item.kind = MediaKind::Image;
            item.width = Some(i.ancho);
            item.height = Some(i.alto);
        } else {
            item.kind = MediaKind::Audio;
            item.has_audio = true;
            item.duration = inv
                .audios
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| a.duracion)
                .or_else(|| {
                    trabajo
                        .analisis
                        .audios
                        .iter()
                        .find(|a| a.id == id)
                        .and_then(|a| a.duracion)
                });
        }
        let tam = (
            f64::from(item.width.unwrap_or(0)),
            f64::from(item.height.unwrap_or(0)),
        );
        let hecho = (item.id.clone(), tam);
        self.media.push(item);
        self.por_clip.insert(id.to_owned(), hecho.clone());
        hecho
    }
}

/// Lays a timeline out as a project.
pub fn construir_proyecto(trabajo: &Trabajo, linea: &Linea, opciones: &Opciones) -> Project {
    let (w, h) = (f64::from(opciones.ancho), f64::from(opciones.alto));
    let vertical = opciones.alto > opciones.ancho;
    let mut project = Project::with_video(VideoSettings {
        width: opciones.ancho,
        height: opciones.alto,
        rate_num: i64::from(opciones.fps),
        rate_den: 1,
        ..VideoSettings::default()
    });
    let mut ids = Ids(0);
    let timeline = Arc::make_mut(&mut project.timelines[0]);
    while timeline.tracks.len() < PISTAS {
        timeline.tracks.push(Track {
            id: ids.next("t"),
            ..Track::default()
        });
    }
    let pistas: Vec<String> = timeline.tracks.iter().map(|t| t.id.clone()).collect();

    let mut bin = Bin::default();

    let centro = |id: &str| -> Option<(f64, f64)> {
        let analisis = &trabajo.analisis;
        if let Some(e) = analisis
            .clips
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.encuadre)
        {
            return Some((e.cx, e.cy));
        }
        analisis
            .imagenes
            .iter()
            .find(|i| i.id == id)
            .and_then(|i| {
                i.caras
                    .iter()
                    .max_by(|a, b| (a[2] * a[3]).total_cmp(&(b[2] * b[3])))
            })
            .map(|[x, y, cw, ch]| (x + cw / 2.0, y + ch / 2.0))
    };

    let mut clips: Vec<Clip> = Vec::new();

    for trozo in &linea.trozos {
        let (media_id, tam) = bin.medio(&mut ids, trabajo, &trozo.clip, &trozo.ruta);
        let kind = if trozo.imagen {
            ClipKind::Image
        } else {
            ClipKind::Video
        };
        let mut clip = Clip::blank(
            ids.next("c"),
            &pistas[VOZ],
            kind,
            &trozo.clip,
            trozo.t,
            trozo.fin - trozo.ini,
        );
        clip.media_id = media_id;
        if !trozo.imagen {
            clip.source_start = trozo.ini;
        }
        if trozo.plano {
            match trozo.audio.as_deref() {
                Some("ambiente") => clip.volume = 0.35,
                _ => clip.muted = Some(true),
            }
        }
        encuadrar(&mut clip, tam, centro(&trozo.clip), (w, h));
        clips.push(clip);
    }

    for recurso in &linea.recursos {
        let (media_id, tam) = bin.medio(&mut ids, trabajo, &recurso.clip, &recurso.ruta);
        let kind = if recurso.imagen {
            ClipKind::Image
        } else {
            ClipKind::Video
        };
        let mut clip = Clip::blank(
            ids.next("c"),
            &pistas[RECURSOS],
            kind,
            &recurso.clip,
            recurso.t,
            recurso.dur,
        );
        clip.media_id = media_id;
        if !recurso.imagen {
            clip.source_start = recurso.desde;
            clip.muted = Some(true);
        }
        encuadrar(&mut clip, tam, centro(&recurso.clip), (w, h));
        clips.push(clip);
    }

    // Kit sizes are for 720x1280; other frames scale them as the engine does.
    let escala = (w / 720.0).min(h / 800.0);
    let kit = &opciones.texto;
    let estilo_de = |estilo: &EstiloTexto, contenido: &str| {
        TextStyle {
            content: if estilo.mayusculas {
                contenido.to_uppercase()
            } else {
                contenido.to_owned()
            },
            font_family: format!("\"{}\"", estilo.familia),
            // ASS sizes a font by ascent plus descent, about 1.2 em for both
            // kit faces; the editor sizes it by the em.
            font_size: estilo.tam * escala / 1.2 / h,
            font_weight: estilo.peso,
            color: kit.color.clone(),
            align: TextAlign::Center,
            stroke_width: estilo.borde * escala / h,
            stroke_color: kit.borde.clone(),
            shadow: estilo.borde > 0.0,
            max_width: (1.0 - 2.0 * estilo.margen).clamp(0.0, 1.0),
            ..TextStyle::default()
        }
        .tidy()
    };

    for sub in &linea.subtitulos {
        let texto = sub.texto.trim();
        let y = posicion(&sub.posicion, vertical) - 0.5;
        let (estilo, inicio) = match sub.estilo.as_str() {
            "enfasis" => (&kit.enfasis, sub.ini + 0.45),
            "clave" => (&kit.clave, sub.ini),
            _ => (&kit.normal, sub.ini),
        };
        if sub.estilo == "enfasis" {
            // The word large and pale in the middle, shrinking as it fades.
            let mut destello = Clip::blank(
                ids.next("c"),
                &pistas[DESTELLOS],
                ClipKind::Text,
                texto,
                sub.ini,
                0.55,
            );
            let mut estilo = estilo_de(&kit.gigante, texto);
            estilo.opacity = f64::from(0xff - 0x70) / 255.0;
            estilo.shadow = false;
            estilo.max_width = 0.0;
            destello.text = Some(estilo);
            destello.fade_out = 0.12;
            destello.keys = vec![
                ClipKey {
                    property: KeyProperty::Scale,
                    at: 0.0,
                    value: 1.35,
                    ease: KeyEase::default(),
                },
                ClipKey {
                    property: KeyProperty::Scale,
                    at: 1.0,
                    value: 1.0,
                    ease: KeyEase::default(),
                },
            ];
            clips.push(destello);
        }
        let mut clip = Clip::blank(
            ids.next("c"),
            &pistas[SUBTITULOS],
            ClipKind::Text,
            texto,
            inicio,
            sub.fin - inicio,
        );
        clip.offset_y = y;
        clip.text = Some(estilo_de(estilo, texto));
        if sub.estilo == "enfasis" {
            clip.fade_in = 0.08;
            clip.fade_out = 0.08;
        }
        clips.push(clip);
    }

    if let Some(musica) = &linea.musica
        && let Some(ruta) = &musica.ruta
    {
        let (media_id, _) = bin.medio(&mut ids, trabajo, &musica.clip, ruta);
        let total = bin
            .media
            .iter()
            .find(|m| m.id == media_id)
            .and_then(|m| m.duration)
            .unwrap_or(f64::INFINITY);
        let dur = linea.duracion_voz.min(total - musica.desde_seg);
        if dur > 0.0 {
            let mut clip = Clip::blank(
                ids.next("c"),
                &pistas[MUSICA],
                ClipKind::Audio,
                &musica.clip,
                0.0,
                dur,
            );
            clip.media_id = media_id;
            clip.source_start = musica.desde_seg;
            clip.volume = musica.volumen;
            clip.fade_in = 0.6;
            clip.fade_out = 1.2;
            clips.push(clip);
        }
    }

    let timeline = Arc::make_mut(&mut project.timelines[0]);
    timeline.clips = clips.into_iter().map(Arc::new).collect();
    project.media = bin.media;
    project.fonts = kit
        .fuentes
        .iter()
        .map(|(family, path)| CustomFont {
            family: family.clone(),
            path: path.clone(),
        })
        .collect();
    project
}

/// Covers the frame and keeps the face in it: centred across, and at 40 %
/// from the top when the picture is cut above and below - the engine's
/// `vf_encuadre`. A picture far taller than the frame is fitted whole.
fn encuadrar(clip: &mut Clip, tam: (f64, f64), centro: Option<(f64, f64)>, frame: (f64, f64)) {
    let ((sw, sh), (w, h)) = (tam, frame);
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    if (sw / sh) / (w / h) < 0.6 {
        return;
    }
    let Some(escala) = framing_scale(Framing::Fill, tam, frame, 0.0) else {
        return;
    };
    clip.scale = escala;
    let ajuste = (w / sw).min(h / sh) * escala;
    let (dw, dh) = (sw * ajuste, sh * ajuste);
    let (cx, cy) = centro.unwrap_or((0.5, 0.42));
    // Pixels cut off the left and the top, held inside the picture.
    let x = (cx * dw - w / 2.0).clamp(0.0, (dw - w).max(0.0));
    let y = (cy * dh - h * 0.4).clamp(0.0, (dh - h).max(0.0));
    if dw > w + 0.5 {
        clip.offset_x = (dw / 2.0 - x - w / 2.0) / w;
    }
    if dh > h + 0.5 {
        clip.offset_y = (dh / 2.0 - y - h / 2.0) / h;
    }
}

/// A frame rate as the fraction the engine works in: NTSC rates over
/// 1001, the rest over one.
fn fraccion(fps: f64) -> String {
    let ntsc = fps * 1001.0 / 1000.0;
    if (fps - fps.round()).abs() > 0.005 && (ntsc - ntsc.round()).abs() < 0.01 {
        format!("{}/1001", ntsc.round() * 1000.0)
    } else {
        format!("{}/1", fps.round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_shot_in_a_tall_frame_keeps_the_face() {
        let mut clip = Clip::blank("c1", "T1", ClipKind::Video, "x", 0.0, 1.0);
        encuadrar(
            &mut clip,
            (1920.0, 1080.0),
            Some((0.25, 0.5)),
            (1080.0, 1920.0),
        );
        // Covering a 9:16 frame with 16:9 takes (16/9)^2 the fitted size.
        assert!((clip.scale - (16.0f64 / 9.0).powi(2)).abs() < 1e-3);
        // A face left of centre moves the picture right.
        assert!(clip.offset_x > 0.0);
        assert_eq!(clip.offset_y, 0.0);
    }

    #[test]
    fn the_same_shape_is_left_alone() {
        let mut clip = Clip::blank("c1", "T1", ClipKind::Video, "x", 0.0, 1.0);
        encuadrar(
            &mut clip,
            (2160.0, 3840.0),
            Some((0.1, 0.1)),
            (1080.0, 1920.0),
        );
        assert_eq!(clip.scale, 1.0);
        assert_eq!((clip.offset_x, clip.offset_y), (0.0, 0.0));
    }

    #[test]
    fn rates_are_written_as_fractions() {
        assert_eq!(fraccion(30.0), "30/1");
        assert_eq!(fraccion(29.97), "30000/1001");
        assert_eq!(fraccion(25.0), "25/1");
    }
}
