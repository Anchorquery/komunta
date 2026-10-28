//! A [`Linea`] laid out as an editable project. Lanes, bottom first: the
//! voice line (and the closing card), the warm flare, B-roll, hand-drawn
//! marks, captions (as many lanes as piled lines need), the emphasis
//! flashes and the music. Every piece stays a plain clip, so the editor
//! can move, trim and restyle all of it.
//!
//! Each frame the recipe asks for (portrait, 16:9, 1:1) is a timeline of
//! its own in the same project, sharing the bin.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use concat_project::Project;
use concat_project::model::{
    AppliedFilter, Background, Clip, ClipKey, ClipKind, ClipMask, CustomFont, Framing, KeyEase,
    KeyProperty, MaskShape, MediaItem, MediaKind, TextAlign, TextStyle, Timeline, Track,
    VideoSettings, framing_scale,
};

use crate::Error;
use crate::entrada::Trabajo;
use crate::garabatos;
use crate::kit::{EstiloTexto, Kit};
use crate::linea::{Linea, Sub};
use crate::sondeo::Sondeo;

/// A frame the montage is made in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formato {
    /// "vertical", "16x9" or "1x1".
    pub nombre: String,
    /// Pixels.
    pub ancho: u32,
    /// Pixels.
    pub alto: u32,
}

impl Formato {
    /// One of the three frames by name, at full HD.
    pub fn de(nombre: &str) -> Option<Formato> {
        let (ancho, alto) = match nombre {
            "vertical" => (1080, 1920),
            "16x9" => (1920, 1080),
            "1x1" => (1080, 1080),
            _ => return None,
        };
        Some(Formato {
            nombre: nombre.to_owned(),
            ancho,
            alto,
        })
    }

    fn titulo(&self) -> &str {
        match self.nombre.as_str() {
            "vertical" => "Vertical",
            "16x9" => "16:9",
            "1x1" => "1:1",
            otro => otro,
        }
    }
}

/// How the project is made.
#[derive(Clone, Debug)]
pub struct Opciones {
    /// The frames, the main one first; each becomes a timeline.
    pub formatos: Vec<Formato>,
    /// Frames per second.
    pub fps: u32,
    /// Brand kit.
    pub kit: Kit,
    /// The project folder; drawn marks go in its `montaje/` folder.
    pub carpeta: PathBuf,
    /// How a kit file (card, flare, music) is probed.
    pub sondear: fn(&Path) -> Option<Sondeo>,
}

/// The B&W look the engine's "bn" effect was: black and white with
/// contrast.
const EFECTO_BN: &str = "concat.high-contrast-mono";
/// How far the music drops under the voice.
const DUCKING: f64 = 0.35;

/// Lanes, bottom first. Captions take as many lanes as overlap needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Capa {
    Voz,
    Destello,
    Recursos,
    Garabatos,
    Texto(usize),
    Gigante,
    Musica,
}

/// Ids the way the editor mints them: one counter shared by every prefix.
struct Ids(u64);

impl Ids {
    fn next(&mut self, prefijo: &str) -> String {
        self.0 += 1;
        format!("{prefijo}{}", self.0)
    }
}

/// The bin: every file the timelines use, once, keyed by path, with its
/// picture size.
#[derive(Default)]
struct Bin {
    media: Vec<MediaItem>,
    por_ruta: HashMap<String, (String, (f64, f64))>,
}

impl Bin {
    /// A file from the job's inventory, by its inventory id.
    fn del_trabajo(
        &mut self,
        ids: &mut Ids,
        trabajo: &Trabajo,
        id: &str,
        ruta: &str,
    ) -> (String, (f64, f64)) {
        let inv = &trabajo.inventario;
        self.agregar(ids, ruta, |item| {
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
                        let audio = trabajo.analisis.audios.iter().find(|a| a.id == id)?;
                        audio.duracion
                    });
            }
        })
    }

    /// A file from outside the inventory (kit card, flare, music, drawn
    /// mark), described by its probe.
    fn de_archivo(
        &mut self,
        ids: &mut Ids,
        ruta: &Path,
        kind: MediaKind,
        sondeo: Sondeo,
    ) -> (String, (f64, f64)) {
        self.agregar(ids, &ruta.to_string_lossy(), |item| {
            item.kind = kind;
            if kind != MediaKind::Image {
                item.duration = Some(sondeo.duracion);
            }
            if kind != MediaKind::Audio {
                item.width = Some(sondeo.ancho);
                item.height = Some(sondeo.alto);
            }
            if kind == MediaKind::Video {
                item.frame_rate = Some(sondeo.fps);
                item.frame_rate_fraction = Some(fraccion(sondeo.fps));
            }
            item.has_audio = sondeo.audio || kind == MediaKind::Audio;
        })
    }

    fn agregar(
        &mut self,
        ids: &mut Ids,
        ruta: &str,
        describir: impl FnOnce(&mut MediaItem),
    ) -> (String, (f64, f64)) {
        if let Some(hecho) = self.por_ruta.get(ruta) {
            return hecho.clone();
        }
        let nombre = Path::new(ruta)
            .file_name()
            .map_or_else(|| ruta.to_owned(), |n| n.to_string_lossy().into_owned());
        let mut item = MediaItem {
            id: ids.next("m"),
            path: ruta.to_owned(),
            name: nombre,
            ..MediaItem::default()
        };
        describir(&mut item);
        let tam = (
            f64::from(item.width.unwrap_or(0)),
            f64::from(item.height.unwrap_or(0)),
        );
        let hecho = (item.id.clone(), tam);
        self.media.push(item);
        self.por_ruta.insert(ruta.to_owned(), hecho.clone());
        hecho
    }

    fn duracion(&self, media_id: &str) -> Option<f64> {
        self.media.iter().find(|m| m.id == media_id)?.duration
    }
}

/// Lays a timeline out as a project: one timeline per frame, the first
/// active.
pub fn construir_proyecto(
    trabajo: &Trabajo,
    linea: &Linea,
    opciones: &Opciones,
) -> Result<Project, Error> {
    let vertical = Formato::de("vertical").expect("a known frame");
    let formatos = if opciones.formatos.is_empty() {
        vec![vertical]
    } else {
        opciones.formatos.clone()
    };
    let mut project = Project::with_video(video(&formatos[0], opciones.fps));
    let mut ids = Ids(0);
    let mut bin = Bin::default();
    let mut timelines = Vec::new();
    for (n, formato) in formatos.iter().enumerate() {
        let id = if n == 0 {
            "TL1".to_owned()
        } else {
            ids.next("tl")
        };
        let timeline = linea_de_tiempo(trabajo, linea, opciones, formato, id, &mut bin, &mut ids)?;
        timelines.push(Arc::new(timeline));
    }
    project.active_timeline_id = timelines[0].id.clone();
    project.timelines = timelines;
    project.media = bin.media;
    let t = &opciones.kit.texto;
    let mut fuentes: Vec<CustomFont> = Vec::new();
    for estilo in [&t.normal, &t.enfasis, &t.clave, &t.gigante] {
        if let Some(archivo) = &estilo.archivo {
            let path = archivo.to_string_lossy().into_owned();
            if !fuentes.iter().any(|f| f.path == path) {
                fuentes.push(CustomFont {
                    family: estilo.familia.clone(),
                    path,
                });
            }
        }
    }
    project.fonts = fuentes;
    Ok(project)
}

fn video(formato: &Formato, fps: u32) -> VideoSettings {
    VideoSettings {
        width: formato.ancho,
        height: formato.alto,
        rate_num: i64::from(fps),
        rate_den: 1,
        ..VideoSettings::default()
    }
}

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

/// One frame's timeline.
fn linea_de_tiempo(
    trabajo: &Trabajo,
    linea: &Linea,
    opciones: &Opciones,
    formato: &Formato,
    id: String,
    bin: &mut Bin,
    ids: &mut Ids,
) -> Result<Timeline, Error> {
    let frame = (f64::from(formato.ancho), f64::from(formato.alto));
    let (w, h) = frame;
    let vertical = formato.alto > formato.ancho;
    // Kit sizes are for 720x1280; other frames scale them as the engine does.
    let escala = (w / 720.0).min(h / 800.0);
    let kit = &opciones.kit;
    let mut clips: Vec<(Capa, Clip)> = Vec::new();

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
    let efectos = |efecto: &Option<String>| -> Vec<AppliedFilter> {
        match efecto.as_deref() {
            Some("bn") => vec![AppliedFilter {
                id: EFECTO_BN.to_owned(),
                params: BTreeMap::new(),
                enabled: true,
                keys: BTreeMap::new(),
            }],
            _ => Vec::new(),
        }
    };

    // The voice line.
    for trozo in &linea.trozos {
        let (media_id, tam) = bin.del_trabajo(ids, trabajo, &trozo.clip, &trozo.ruta);
        let kind = if trozo.imagen {
            ClipKind::Image
        } else {
            ClipKind::Video
        };
        let mut clip = Clip::blank(
            ids.next("c"),
            "",
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
        clip.video_effects = efectos(&trozo.efecto);
        encuadrar(&mut clip, tam, centro(&trozo.clip), frame);
        clips.push((Capa::Voz, clip));
    }

    // The closing card, after the voice.
    if let Some(ruta) = kit.cierre(&formato.nombre)
        && let Some(sondeo) = (opciones.sondear)(ruta)
        && sondeo.duracion > 0.0
    {
        let (media_id, tam) = bin.de_archivo(ids, ruta, MediaKind::Video, sondeo);
        let mut clip = Clip::blank(
            ids.next("c"),
            "",
            ClipKind::Video,
            "Cierre",
            linea.duracion_voz,
            sondeo.duracion,
        );
        clip.media_id = media_id;
        encuadrar(&mut clip, tam, None, frame);
        clips.push((Capa::Voz, clip));
    }

    // The warm flare over the speaker (not over shots).
    if kit.destello > 0.0
        && let Some(ruta) = &kit.destello_video
        && let Some(sondeo) = (opciones.sondear)(ruta)
        && sondeo.duracion > 0.5
    {
        let (media_id, tam) = bin.de_archivo(ids, ruta, MediaKind::Video, sondeo);
        for (a, b) in tramos_de_voz(linea, 0.0) {
            // The flare loops: cut it into pieces that keep its phase.
            let mut t = a;
            while b - t > 0.04 {
                let dentro = t % sondeo.duracion;
                let largo = (sondeo.duracion - dentro).min(b - t);
                let mut clip =
                    Clip::blank(ids.next("c"), "", ClipKind::Video, "Destello", t, largo);
                clip.media_id = media_id.clone();
                clip.source_start = dentro;
                clip.blend = "screen".to_owned();
                clip.opacity = kit.destello.clamp(0.0, 1.0);
                clip.muted = Some(true);
                encuadrar(&mut clip, tam, None, frame);
                clips.push((Capa::Destello, clip));
                t += largo;
            }
        }
    }

    // B-roll, and the marks drawn over it.
    let carpeta_garabatos = opciones.carpeta.join("montaje").join("garabatos");
    for recurso in &linea.recursos {
        let (media_id, tam) = bin.del_trabajo(ids, trabajo, &recurso.clip, &recurso.ruta);
        let kind = if recurso.imagen {
            ClipKind::Image
        } else {
            ClipKind::Video
        };
        let mut clip = Clip::blank(
            ids.next("c"),
            "",
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
        clip.video_effects = efectos(&recurso.efecto);
        encuadrar(&mut clip, tam, centro(&recurso.clip), frame);
        clips.push((Capa::Recursos, clip));

        if let Some(forma) = &recurso.garabato
            && let Some(dibujo) = garabatos::dibujar(
                forma,
                escala as f32,
                (14.0 * escala) as f32,
                &carpeta_garabatos,
            )?
        {
            let sondeo = Sondeo {
                ancho: dibujo.ancho,
                alto: dibujo.alto,
                ..Sondeo::default()
            };
            let (media_id, _) = bin.de_archivo(ids, &dibujo.ruta, MediaKind::Image, sondeo);
            let marca = garabato(
                ids.next("c"),
                media_id,
                &dibujo,
                recurso.t,
                recurso.dur,
                recurso.garabato_pos,
                frame,
            );
            clips.push((Capa::Garabatos, marca));
        }
    }

    // Captions, piled lines included.
    let estilo_de = |estilo: &EstiloTexto, contenido: &str| {
        TextStyle {
            content: if estilo.mayusculas {
                contenido.to_uppercase()
            } else {
                contenido.to_owned()
            },
            font_family: format!("\"{}\"", estilo.familia),
            // ASS sizes a font by ascent plus descent, about 1.2 em for the
            // kit faces; the editor sizes it by the em.
            font_size: estilo.tam * escala / 1.2 / h,
            font_weight: estilo.peso,
            color: kit.texto.color.clone(),
            align: TextAlign::Center,
            stroke_width: estilo.borde * escala / h,
            stroke_color: kit.texto.borde.clone(),
            shadow: estilo.borde > 0.0,
            max_width: (1.0 - 2.0 * estilo.margen).clamp(0.0, 1.0),
            tracking: estilo.espaciado / h,
            ..TextStyle::default()
        }
        .tidy()
    };
    let pilas = pilas(&linea.subtitulos);
    let mut textos: Vec<Clip> = Vec::new();
    for (n, sub) in linea.subtitulos.iter().enumerate() {
        let texto = sub.texto.trim();
        let mut y = posicion(&sub.posicion, vertical);
        let mut fin = sub.fin;
        if let Some((j, total, ultimo)) = pilas.get(&n).copied() {
            let alto_linea = if sub.estilo == "clave" { 92.0 } else { 62.0 } * escala / h;
            let mut desplazamiento = j as f64 - (total as f64 - 1.0) / 2.0;
            if sub.apilar_arriba {
                desplazamiento = -desplazamiento;
            }
            y += desplazamiento * alto_linea;
            fin = linea.subtitulos[ultimo].fin;
        }
        let (estilo, inicio) = match sub.estilo.as_str() {
            "enfasis" => (&kit.texto.enfasis, sub.ini + 0.45),
            "clave" => (&kit.texto.clave, sub.ini),
            _ => (&kit.texto.normal, sub.ini),
        };
        if sub.estilo == "enfasis" {
            // The word large and pale in the middle, shrinking as it fades.
            let mut destello = Clip::blank(ids.next("c"), "", ClipKind::Text, texto, sub.ini, 0.55);
            let mut grande = estilo_de(&kit.texto.gigante, texto);
            grande.opacity = f64::from(0xff - 0x70) / 255.0;
            grande.shadow = false;
            // One line, running off both edges, as the engine draws it.
            grande.max_width = 100.0;
            destello.text = Some(grande);
            destello.fade_out = 0.12;
            destello.keys = vec![
                llave(KeyProperty::Scale, 0.0, 1.35),
                llave(KeyProperty::Scale, 1.0, 1.0),
            ];
            clips.push((Capa::Gigante, destello));
        }
        let mut clip = Clip::blank(
            ids.next("c"),
            "",
            ClipKind::Text,
            texto,
            inicio,
            fin - inicio,
        );
        clip.offset_y = y - 0.5;
        clip.text = Some(estilo_de(estilo, texto));
        if sub.estilo == "enfasis" {
            clip.fade_in = 0.08;
            clip.fade_out = 0.08;
        }
        textos.push(clip);
    }
    for (carril, clip) in carriles(textos) {
        clips.push((Capa::Texto(carril), clip));
    }

    // Music, dropping under the voice.
    if let Some(musica) = &linea.musica {
        let del_kit = musica.clip == "musica_kit";
        let ruta = if del_kit {
            kit.musica
                .as_ref()
                .map(|r| r.to_string_lossy().into_owned())
        } else {
            musica.ruta.clone()
        };
        if let Some(ruta) = ruta {
            let (media_id, _) = if del_kit {
                let sondeo = (opciones.sondear)(Path::new(&ruta)).unwrap_or_default();
                bin.de_archivo(ids, Path::new(&ruta), MediaKind::Audio, sondeo)
            } else {
                bin.del_trabajo(ids, trabajo, &musica.clip, &ruta)
            };
            let total = bin
                .duracion(&media_id)
                .filter(|d| *d > 0.0)
                .unwrap_or(f64::INFINITY);
            let dur = linea.duracion_voz.min(total - musica.desde_seg);
            if dur > 0.0 {
                let mut clip =
                    Clip::blank(ids.next("c"), "", ClipKind::Audio, &musica.clip, 0.0, dur);
                clip.media_id = media_id;
                clip.source_start = musica.desde_seg;
                clip.volume = musica.volumen;
                clip.fade_in = 0.6;
                clip.fade_out = 1.2;
                clip.keys = ducking(&tramos_de_voz(linea, 0.45), musica.volumen, dur);
                clips.push((Capa::Musica, clip));
            }
        }
    }

    // Lanes: only the ones used, in order, and always the voice.
    let mut capas: Vec<Capa> = clips.iter().map(|(c, _)| *c).collect();
    capas.push(Capa::Voz);
    capas.sort();
    capas.dedup();
    let pistas: HashMap<Capa, String> = capas.iter().map(|c| (*c, ids.next("t"))).collect();
    let tracks = capas
        .iter()
        .map(|c| Track {
            id: pistas[c].clone(),
            ..Track::default()
        })
        .collect();
    let clips = clips
        .into_iter()
        .map(|(capa, mut clip)| {
            clip.track_id = pistas[&capa].clone();
            Arc::new(clip)
        })
        .collect();
    Ok(Timeline {
        id,
        name: format!("Montaje · {}", formato.titulo()),
        video: video(formato, opciones.fps),
        background: Background::None,
        tracks,
        clips,
        extra: Default::default(),
    })
}

fn llave(property: KeyProperty, at: f64, value: f64) -> ClipKey {
    ClipKey {
        property,
        at,
        value,
        ease: KeyEase::default(),
    }
}

/// For each caption in a pile: its place in it, how many lines it has
/// and the index of the last one.
fn pilas(subs: &[Sub]) -> HashMap<usize, (usize, usize, usize)> {
    let mut grupos: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (n, sub) in subs.iter().enumerate() {
        if let Some(grupo) = &sub.apilar {
            grupos.entry(grupo).or_default().push(n);
        }
    }
    let mut sitio = HashMap::new();
    for miembros in grupos.values() {
        let ultimo = *miembros.last().expect("a group has members");
        for (j, n) in miembros.iter().enumerate() {
            sitio.insert(*n, (j, miembros.len(), ultimo));
        }
    }
    sitio
}

/// Clips spread over as few lanes as keep them from overlapping, first
/// lane first.
fn carriles(mut clips: Vec<Clip>) -> Vec<(usize, Clip)> {
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut fines: Vec<f64> = Vec::new();
    clips
        .into_iter()
        .map(|clip| {
            let carril = match fines.iter().position(|fin| *fin <= clip.start + 1e-6) {
                Some(libre) => libre,
                None => {
                    fines.push(0.0);
                    fines.len() - 1
                }
            };
            fines[carril] = clip.start + clip.duration;
            (carril, clip)
        })
        .collect()
}

/// When someone is speaking: the voice line's speech pieces, joined when
/// the gap between them is under `junta` seconds.
fn tramos_de_voz(linea: &Linea, junta: f64) -> Vec<(f64, f64)> {
    let mut tramos: Vec<(f64, f64)> = Vec::new();
    for trozo in linea.trozos.iter().filter(|t| !t.plano) {
        let (a, b) = (trozo.t, trozo.t + trozo.fin - trozo.ini);
        match tramos.last_mut() {
            Some(ultimo) if a - ultimo.1 <= junta + 1e-6 => ultimo.1 = ultimo.1.max(b),
            _ => tramos.push((a, b)),
        }
    }
    tramos
}

/// Volume keys that drop the music to [`DUCKING`] of its level while
/// someone speaks: down in 20 ms, back up in 400, as the engine's
/// compressor did.
fn ducking(tramos: &[(f64, f64)], volumen: f64, dur: f64) -> Vec<ClipKey> {
    if tramos.is_empty() || dur <= 0.0 {
        return Vec::new();
    }
    let bajo = volumen * DUCKING;
    let mut puntos: Vec<(f64, f64)> = Vec::new();
    for &(a, b) in tramos {
        let (a, b) = (a.max(0.0), b.min(dur));
        if b <= a {
            continue;
        }
        if a > 0.02 {
            puntos.push((a - 0.02, volumen));
        }
        puntos.push((a, bajo));
        puntos.push((b, bajo));
        if b + 0.4 < dur {
            puntos.push((b + 0.4, volumen));
        }
    }
    if puntos.first().is_some_and(|(t, _)| *t > 0.0) {
        puntos.insert(0, (0.0, volumen));
    }
    // A key at a time already passed would fold the curve back.
    let mut llaves: Vec<ClipKey> = Vec::new();
    let mut ultimo = -1.0;
    for (t, v) in puntos {
        let at = (t / dur).clamp(0.0, 1.0);
        if at > ultimo + 1e-4 {
            llaves.push(llave(KeyProperty::Volume, at, v));
            ultimo = at;
        }
    }
    llaves
}

/// A drawn mark placed at `pos` (fractions of the frame) at its own pixel
/// size, revealed left to right by a mask over the first part of its time.
fn garabato(
    id: String,
    media_id: String,
    dibujo: &garabatos::Dibujo,
    t: f64,
    dur: f64,
    pos: [f64; 2],
    frame: (f64, f64),
) -> Clip {
    let (w, h) = frame;
    let (pw, ph) = (f64::from(dibujo.ancho), f64::from(dibujo.alto));
    let mut clip = Clip::blank(id, "", ClipKind::Image, "Garabato", t, dur);
    clip.media_id = media_id;
    // A still is contain-fitted first; undo the fit to show it pixel for
    // pixel.
    clip.scale = 1.0 / (w / pw).min(h / ph);
    clip.offset_x = pos[0] - 0.5;
    clip.offset_y = pos[1] - 0.5;
    clip.fade_out = 0.2;
    // The mask's box is in units of the picture's shorter side; its left
    // edge stays on the picture's while it widens.
    let corto = pw.min(ph);
    let entero = pw / corto * 1.02;
    let centro = |ancho: f64| -0.5 + ancho * corto / pw / 2.0;
    clip.mask = Some(ClipMask {
        shape: MaskShape::Rectangle,
        x: centro(entero),
        y: 0.0,
        width: entero,
        height: ph / corto * 1.1,
        rotation: 0.0,
        roundness: 0.0,
        feather: 0.0,
        invert: false,
    });
    let dibujo_seg = (dur.min(2.0) * 0.45).max(0.3);
    let hasta = (dibujo_seg / dur).min(1.0);
    let inicio = ClipMask::MIN_SIZE;
    clip.keys = vec![
        llave(KeyProperty::MaskWidth, 0.0, inicio),
        llave(KeyProperty::MaskWidth, hasta, entero),
        llave(KeyProperty::MaskX, 0.0, centro(inicio)),
        llave(KeyProperty::MaskX, hasta, centro(entero)),
    ];
    clip
}

/// Covers the frame and keeps the face in it: centred across, and at 40 %
/// from the top when the picture is cut above and below - the engine's
/// `vf_encuadre`. A picture far taller than the frame is fitted whole
/// over a blurred copy of itself.
fn encuadrar(clip: &mut Clip, tam: (f64, f64), centro: Option<(f64, f64)>, frame: (f64, f64)) {
    let ((sw, sh), (w, h)) = (tam, frame);
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    if (sw / sh) / (w / h) < 0.6 {
        clip.background = Some(Background::Blur {
            amount: Background::DEFAULT_BLUR,
        });
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
    fn a_tall_shot_in_a_wide_frame_is_fitted_over_a_blur() {
        let mut clip = Clip::blank("c1", "T1", ClipKind::Video, "x", 0.0, 1.0);
        encuadrar(&mut clip, (1080.0, 1920.0), None, (1920.0, 1080.0));
        assert_eq!(clip.scale, 1.0);
        assert!(matches!(clip.background, Some(Background::Blur { .. })));
    }

    #[test]
    fn rates_are_written_as_fractions() {
        assert_eq!(fraccion(30.0), "30/1");
        assert_eq!(fraccion(29.97), "30000/1001");
        assert_eq!(fraccion(25.0), "25/1");
    }

    #[test]
    fn overlapping_captions_take_another_lane() {
        let clip = |start, duration| Clip::blank("c", "", ClipKind::Text, "x", start, duration);
        let lanes: Vec<usize> = carriles(vec![
            clip(0.0, 2.0),
            clip(1.0, 3.0),
            clip(2.0, 1.0),
            clip(4.5, 1.0),
        ])
        .into_iter()
        .map(|(lane, _)| lane)
        .collect();
        assert_eq!(lanes, [0, 1, 0, 0]);
    }

    #[test]
    fn music_drops_under_the_voice_and_comes_back() {
        let keys = ducking(&[(2.0, 4.0)], 0.2, 10.0);
        let at = |t: f64| {
            keys.iter()
                .find(|k| (k.at - t / 10.0).abs() < 1e-9)
                .map(|k| k.value)
        };
        assert_eq!(at(0.0), Some(0.2));
        assert_eq!(at(2.0), Some(0.2 * DUCKING));
        assert_eq!(at(4.0), Some(0.2 * DUCKING));
        assert_eq!(at(4.4), Some(0.2));
        assert!(keys.windows(2).all(|p| p[0].at < p[1].at));
    }
}
