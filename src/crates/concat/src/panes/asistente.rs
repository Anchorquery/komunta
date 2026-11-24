// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Komunta's assistant: an agent beside the timeline that edits the
//! project on screen.
//!
//! The person writes; a turn runs on a thread of its own - the AI answers
//! in seconds, not frames - with the editor's MCP tools and the agent's
//! recipes in its hands ([`super::asistente_vivo`]), each edit made on the
//! window's event loop under the turn's gesture, so one Ctrl+Z takes a
//! whole turn back. What the AI says, its plan, the tool it is using and
//! the calls that wait for a yes come back here as messages
//! ([`super::asistente_persona`]); Stop and the answers go back the same
//! way. The conversation is the open project's: closing it forgets it.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::time::Instant;

use asistente::externo::Faceta;
use asistente::{Conversacion, Estado, Evento, Fin, Limites, Proveedor, Tarea};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::panes::guion::GuionMsg;
use crate::panes::montaje::MontajeMsg;
use crate::panes::asistente_externo::{self, Abierto};
use crate::panes::asistente_ia;
use crate::panes::asistente_opciones::{self, Opcion};
use crate::panes::asistente_persona::{self, EnVentana, Mando};
use crate::panes::asistente_vivo::Herramientas;
use crate::studio::Studio;
use crate::ui::{App, AsistenteLineaData, AsistenteOpcionData, AsistentePanel, AsistenteTareaData, PaneKind};

/// The standing instruction: who the AI is here and how it works.
const SISTEMA: &str = "Eres el agente de edición de Komunta, un editor de vídeo. \
Trabajas sobre el proyecto que la persona tiene abierto en pantalla, con las herramientas que se te dan, hasta acabar lo que pide. \
Al final de estas instrucciones está la línea de tiempo como está al empezar el turno: los ids de clips y pistas salen de ahí; no los inventes. \
Cada herramienta que edita devuelve la línea de tiempo nueva, así que no hace falta leerla otra vez con timeline_read. \
Los tiempos van en segundos. \
Si la tarea tiene más de dos pasos, escribe primero el plan con plan_escribir y ve marcando cada tarea al hacerla. \
Las herramientas vienen por familias y al principio solo tienes las del núcleo: \
usar una herramienta de otra familia la activa, y herramientas_buscar te dice cuáles hay para lo que buscas. \
Después de un cambio que se ve (un zoom, un título, un texto detrás de la persona), mira el resultado con ver_fotograma antes de darlo por bueno. \
Algunas llamadas esperan el sí de la persona (traer un archivo, quitar muchos clips, cambiar el tamaño del vídeo): \
si dice que no, no insistas; propón otra cosa o pregunta. \
Haz lo que se te pide y nada más; si algo no está claro, pregunta antes de editar. \
Cuando acabes, escribe una o dos frases con lo que has hecho. \
Todo lo que hagas en un turno se deshace con un solo Ctrl+Z. \
Si el proyecto se montó con IA, recortar el montaje (palabras, tomas, subtítulos, recursos) se hace en su guion, con la familia guion: \
esos cambios se ven en el panel Guion, no entran en el Ctrl+Z del turno y llegan al vídeo con guion_aplicar, que se hace al acabar el turno. \
Montar o rehacer con IA (familia montaje) también empieza al acabar el turno, y gasta: pídelo solo si la persona lo quiere. \
Contesta en el idioma de la persona, también en euskera.";

/// Everything that can happen to the chat.
#[derive(Debug)]
pub enum AsistenteMsg {
    /// The button in the title bar: the pane shown, or put away.
    Toggle,
    /// The person's words, to send.
    Send(String),
    /// The pane is about this many characters wide.
    Resized(i32),
    /// How the turn under way goes.
    Event(Evento),
    /// A call waits for the person's yes.
    Ask(Pregunta),
    /// The person's answer to it.
    Answer(bool),
    /// Stop the turn under way.
    Stop,
    /// The turn is over: the conversation back, the outside agent if
    /// one ran it, and how it ended.
    Finished(Box<Conversacion>, Option<Box<Abierto>>, Result<Fin, String>),
    /// A new conversation.
    Clear,
    /// The agent picked under the field, by its place in the list.
    AgentChosen(i32),
    /// One of the choices under the field set: the choice, the value.
    OptionChosen(i32, i32),
}

/// A call put to the person: the tool, what it will do, and where the
/// answer goes - the agent's turn, or a caller on the socket.
#[derive(Debug)]
pub struct Pregunta {
    /// The tool's name.
    pub herramienta: String,
    /// What it will do, in a sentence.
    pub detalle: String,
    /// The person's answer.
    pub respuesta: Sender<bool>,
    /// Put by this pane's own turn, which Stop ends - not by a caller on
    /// the socket.
    pub del_turno: bool,
}

/// Who said a line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Quien {
    Persona = 0,
    Asistente = 1,
    /// What the window says: a failure, a notice.
    Aviso = 2,
    /// A tool the AI used.
    Accion = 3,
}

struct Linea {
    quien: Quien,
    texto: String,
    /// Small print under it: how long the turn took.
    meta: String,
}

/// The chat's state.
pub struct AsistentePane {
    lineas: Vec<Linea>,
    /// Taken by the turn under way, and given back when it ends.
    conversacion: Option<Conversacion>,
    /// The outside agent the conversation is with, when it is not
    /// Komunta's own; taken by the turn with the conversation.
    externo: Option<Box<Abierto>>,
    /// Turns sent, for each turn's gesture.
    turnos: u64,
    /// When the turn under way was sent.
    desde: Option<Instant>,
    estado: String,
    /// The agent's plan, as it last wrote it.
    plan: Vec<Tarea>,
    /// The calls waiting for a yes, the first one shown.
    preguntas: VecDeque<Pregunta>,
    /// Stop and the answers, for the turn under way.
    mando: Option<Mando>,
    /// The seat's view before the chat took it, to give back.
    sustituido: Option<PaneKind>,
    columnas: usize,
    /// The agent in the list under the field: 0 is Komunta's own.
    agente: i32,
    /// The models Komunta's own agent can ask, for its choice.
    modelos: Vec<String>,
    /// The choices under the field, for the agent in use.
    opciones: Vec<Opcion>,
    /// Bumped whenever the lines must be drawn again.
    revision: u64,
    publicado: Cell<u64>,
}

impl Default for AsistentePane {
    fn default() -> Self {
        Self {
            lineas: Vec::new(),
            conversacion: Some(Conversacion::default()),
            externo: None,
            turnos: 0,
            desde: None,
            estado: String::new(),
            plan: Vec::new(),
            preguntas: VecDeque::new(),
            mando: None,
            sustituido: None,
            columnas: 40,
            agente: 0,
            modelos: Vec::new(),
            opciones: Vec::new(),
            revision: 1,
            publicado: Cell::new(0),
        }
    }
}

impl AsistentePane {
    fn ocupado(&self) -> bool {
        self.conversacion.is_none()
    }

    /// Forgets the conversation, as when the project closes; the seat it
    /// holds stays.
    pub fn olvidar(&mut self) {
        *self = Self {
            sustituido: self.sustituido,
            columnas: self.columnas,
            agente: self.agente,
            modelos: std::mem::take(&mut self.modelos),
            opciones: std::mem::take(&mut self.opciones),
            revision: self.revision + 1,
            ..Self::default()
        };
    }

    /// Applies one message.
    pub fn update(&mut self, msg: AsistenteMsg, studio: &mut Studio) {
        match msg {
            AsistenteMsg::Toggle => {
                self.alternar(studio);
                self.al_dia(studio);
            }
            AsistenteMsg::Send(texto) => self.enviar(texto.trim(), studio),
            AsistenteMsg::Resized(columnas) => {
                let columnas = columnas.max(16) as usize;
                if columnas != self.columnas {
                    self.columnas = columnas;
                    self.revision += 1;
                }
                // The pane's first draw: its choices, once.
                if self.opciones.is_empty() {
                    self.al_dia(studio);
                }
            }
            AsistenteMsg::Event(evento) => self.evento(evento),
            AsistenteMsg::Ask(pregunta) => self.preguntar(pregunta, studio),
            AsistenteMsg::Answer(si) => self.responder(si),
            AsistenteMsg::Stop => self.parar(),
            AsistenteMsg::Finished(conversacion, externo, resultado) => {
                self.externo = externo;
                self.terminado(*conversacion, resultado, studio);
                self.al_dia(studio);
                // What the turn left for after it, once this message is done:
                // both end by opening the project again, with this pane put
                // back in place.
                if studio.guion.tomar_aplicar() {
                    crate::host::on_ui(|studio, _, _| studio.handle(Msg::Guion(GuionMsg::Apply)));
                }
                if studio.montaje.tomar_correr() {
                    crate::host::on_ui(|studio, _, _| studio.handle(Msg::Montaje(MontajeMsg::Run)));
                }
            }
            AsistenteMsg::Clear => {
                if !self.ocupado() {
                    self.olvidar();
                    self.al_dia(studio);
                }
            }
            AsistenteMsg::AgentChosen(indice) => self.cambiar_de_agente(indice, studio),
            AsistenteMsg::OptionChosen(opcion, valor) => self.elegir(opcion, valor, studio),
        }
    }

    /// The choices under the field, for the agent chosen: Komunta's own,
    /// with its models as its key allows, or an outside one's, as its
    /// session offers them once open.
    fn al_dia(&mut self, studio: &Studio) {
        let id = &studio.prefs.server.agent;
        self.agente = asistente_externo::indice(id);
        self.opciones = if id.is_empty() {
            self.modelos = asistente_ia::modelos(studio);
            asistente_opciones::propias(&self.modelos, &studio.prefs.asistente)
        } else {
            let abierto = self.externo.as_mut();
            abierto.map(|abierto| asistente_opciones::externas(&abierto.ajustes())).unwrap_or_default()
        };
        self.revision += 1;
    }

    /// The agent picked under the field, kept as Settings keeps it; an
    /// outside one running is let go.
    fn cambiar_de_agente(&mut self, indice: i32, studio: &mut Studio) {
        if self.ocupado() {
            return;
        }
        studio.prefs.server.agent = asistente_externo::id_de(indice);
        studio.prefs.save(&studio.host.dirs);
        self.externo = None;
        self.al_dia(studio);
    }

    /// The choice at `opcion` set to its value at `valor`: kept in the
    /// preferences for Komunta's own agent, told to an outside one.
    fn elegir(&mut self, opcion: i32, valor: i32, studio: &mut Studio) {
        let (Ok(opcion), Ok(valor)) = (usize::try_from(opcion), usize::try_from(valor)) else {
            return;
        };
        let Some(faceta) = self.opciones.get(opcion).map(|elegida| elegida.faceta) else {
            return;
        };
        if self.ocupado() {
            return;
        }
        if studio.prefs.server.agent.is_empty() {
            asistente_opciones::elegir_propia(&mut studio.prefs.asistente, &self.modelos, faceta, valor);
            studio.prefs.save(&studio.host.dirs);
        } else if let Some(abierto) = self.externo.as_mut() {
            abierto.ajustar(opcion, valor);
        }
        self.al_dia(studio);
    }

    /// Shows the chat in the inspector's seat - or the library's - or
    /// gives the seat back to what it showed.
    fn alternar(&mut self, studio: &mut Studio) {
        if studio.dock.contains(PaneKind::Asistente) {
            let vuelta = self.sustituido.take().unwrap_or(PaneKind::Inspector);
            studio.dock.replace(PaneKind::Asistente, vuelta);
        } else {
            self.sustituido = [PaneKind::Inspector, PaneKind::Media, PaneKind::Keyframes]
                .into_iter()
                .find(|&kind| studio.dock.replace(kind, PaneKind::Asistente));
        }
        studio.remember_dock();
    }

    /// Starts a turn with `texto`, unless one is under way.
    fn enviar(&mut self, texto: &str, studio: &mut Studio) {
        if texto.is_empty() || self.ocupado() {
            return;
        }
        let Some(ruta) = studio.session.as_ref().map(|s| s.path().to_owned()) else {
            return self.decir(Quien::Aviso, t("asistente.noProject"));
        };
        let trabajo = match self.trabajo(studio) {
            Ok(trabajo) => trabajo,
            Err(motivo) => return self.decir(Quien::Aviso, motivo),
        };
        let Some(mut conversacion) = self.conversacion.take() else {
            return;
        };
        self.decir(Quien::Persona, texto.to_owned());
        self.turnos += 1;
        self.desde = Some(Instant::now());
        self.estado = t("asistente.thinking");
        let gesto = format!("asistente:{}", self.turnos);
        let texto = texto.to_owned();
        let epoch = crate::host::project_epoch();
        let (mut persona, mando) = asistente_persona::par(epoch);
        self.mando = Some(mando);
        let traza = studio.host.dirs.data.join("asistente").join("trazas.jsonl");
        let lanzado = std::thread::Builder::new()
            .name("asistente".into())
            .spawn(move || {
                let (antes, inicio, agente) = (conversacion.mensajes.len(), Instant::now(), trabajo.nombre());
                let (externo, resultado) = trabajo.hacer(ruta.into(), gesto, &mut persona, &mut conversacion, &texto);
                let turno = conversacion.mensajes.get(antes..).unwrap_or_default();
                let linea = asistente::traza::linea(&texto, turno, &resultado, inicio.elapsed(), &agente);
                if let Err(error) = asistente::traza::anotar(&traza, &linea) {
                    log::warn!("asistente: the turn's trace was not written: {error}");
                }
                let fin = AsistenteMsg::Finished(Box::new(conversacion), externo, resultado);
                a_la_ventana(epoch, fin);
            });
        if let Err(error) = lanzado {
            self.conversacion = Some(Conversacion::default());
            self.externo = None;
            self.mando = None;
            self.estado.clear();
            self.decir(Quien::Aviso, tf("asistente.failed", &[&error]));
        }
    }

    /// Who runs the next turn: the agent chosen in Settings, which needs
    /// live mode to reach the project, or Komunta's own with its AI.
    fn trabajo(&mut self, studio: &Studio) -> Result<Trabajo, String> {
        let id = studio.prefs.server.agent.clone();
        if id.is_empty() {
            return asistente_ia::proveedor(studio).map(Trabajo::Propio);
        }
        if !(studio.prefs.server.enabled && studio.prefs.server.live) {
            return Err(t("asistente.externalNeedsLive"));
        }
        let clave = asistente_ia::clave_google(studio);
        let agente = asistente_externo::agente(&id, clave).ok_or_else(|| tf("asistente.failed", &[&id]))?;
        Ok(Trabajo::Externo {
            abierto: self.externo.take(),
            id,
            agente,
        })
    }

    fn evento(&mut self, evento: Evento) {
        match evento {
            Evento::Pensando => self.estado = t("asistente.thinking"),
            Evento::Herramienta(nombre) => {
                let etiqueta = etiqueta(&nombre);
                self.estado = format!("{etiqueta}…");
                self.decir(Quien::Accion, etiqueta);
            }
            Evento::Texto(texto) => self.decir(Quien::Asistente, texto),
            Evento::Plan(tareas) => self.plan = tareas,
        }
    }

    /// A call waits for the person's yes: the pane is put on screen if it
    /// is not, so the question is seen.
    fn preguntar(&mut self, pregunta: Pregunta, studio: &mut Studio) {
        if !studio.dock.contains(PaneKind::Asistente) {
            self.alternar(studio);
        }
        self.preguntas.push_back(pregunta);
    }

    /// The person's answer to the call shown.
    fn responder(&mut self, si: bool) {
        if let Some(pregunta) = self.preguntas.pop_front() {
            // A caller gone meanwhile has nobody to tell.
            let _ = pregunta.respuesta.send(si);
        }
    }

    /// Stop: the turn ends after the call under way, and a question of
    /// its waiting gets a no.
    fn parar(&mut self) {
        if let Some(mando) = self.mando.as_ref() {
            mando.parar();
            self.preguntas.retain(|pregunta| !pregunta.del_turno);
            self.estado = t("asistente.stopping");
        }
    }

    /// The turn is over: its last words said, its edits one undo step.
    fn terminado(
        &mut self,
        conversacion: Conversacion,
        resultado: Result<Fin, String>,
        studio: &mut Studio,
    ) {
        if let Some(session) = studio.session.as_mut() {
            session.end_gesture();
        }
        self.estado.clear();
        self.mando = None;
        self.preguntas.retain(|pregunta| !pregunta.del_turno);
        self.plan = conversacion.plan.clone();
        match resultado {
            Ok(Fin::Parada) => self.decir(Quien::Aviso, t("asistente.stopped")),
            Ok(Fin::Respuesta) => {
                let dicho = conversacion.respuesta();
                if !dicho.trim().is_empty() {
                    self.decir(Quien::Asistente, dicho);
                    self.lineas.last_mut().expect("just said").meta = self.duracion();
                }
            }
            Err(motivo) => {
                log::warn!("asistente: {motivo}");
                self.decir(Quien::Aviso, tf("asistente.failed", &[&motivo]));
            }
        }
        self.conversacion = Some(conversacion);
    }

    fn decir(&mut self, quien: Quien, texto: String) {
        self.lineas.push(Linea {
            quien,
            texto,
            meta: String::new(),
        });
        self.revision += 1;
    }

    /// How long the turn that just ended took, for the small print.
    fn duracion(&mut self) -> String {
        let Some(desde) = self.desde.take() else {
            return String::new();
        };
        let segundos = desde.elapsed().as_secs_f32().round().max(1.0);
        tf("asistente.took", &[&segundos])
    }

    /// Hands the chat to the window; the lines only when they changed.
    pub fn publish(&self, app: &App) {
        let panel = app.global::<AsistentePanel>();
        panel.set_busy(self.ocupado());
        panel.set_status(self.estado.as_str().into());
        panel.set_asking(!self.preguntas.is_empty());
        if let Some(pregunta) = self.preguntas.front() {
            let titulo = tf("asistente.approve.title", &[&etiqueta(&pregunta.herramienta)]);
            panel.set_ask_title(titulo.into());
            panel.set_ask_detail(pregunta.detalle.as_str().into());
        }
        panel.set_plan(model(self.plan.iter().map(tarea).collect()));
        if self.publicado.get() == self.revision {
            return;
        }
        self.publicado.set(self.revision);
        let agentes = asistente_externo::nombres().into_iter().map(SharedString::from).collect();
        panel.set_agents(model(agentes));
        panel.set_agent(self.agente);
        panel.set_options(model(self.opciones.iter().map(opcion).collect()));
        let lineas: Vec<AsistenteLineaData> = self
            .lineas
            .iter()
            .map(|linea| AsistenteLineaData {
                quien: linea.quien as i32,
                filas: model(filas(&linea.texto, self.columnas)),
                meta: linea.meta.as_str().into(),
            })
            .collect();
        panel.set_lines(model(lineas));
    }
}

/// A choice under the field as the pane draws it.
fn opcion(opcion: &Opcion) -> AsistenteOpcionData {
    AsistenteOpcionData {
        faceta: match opcion.faceta {
            Faceta::Modelo => 0,
            Faceta::Razonamiento => 1,
            Faceta::Modo => 2,
        },
        valores: model(opcion.valores.iter().map(|valor| SharedString::from(valor.as_str())).collect()),
        actual: opcion.actual as i32,
    }
}

/// A task of the plan as the pane draws it.
fn tarea(tarea: &Tarea) -> AsistenteTareaData {
    AsistenteTareaData {
        texto: tarea.texto.as_str().into(),
        estado: match tarea.estado {
            Estado::Pendiente => 0,
            Estado::EnCurso => 1,
            Estado::Hecha => 2,
        },
    }
}

/// Posts a message to the pane, for the project open when the turn began.
pub(crate) fn a_la_ventana(epoch: u64, msg: AsistenteMsg) {
    crate::host::on_ui_in_project(epoch, move |studio, _, _| {
        studio.handle(Msg::Asistente(msg));
    });
}

/// The standing instruction, with the timeline as the turn finds it.
fn sistema(linea_de_tiempo: Option<Value>) -> String {
    match linea_de_tiempo {
        Some(linea) => format!("{SISTEMA}

La línea de tiempo ahora:
{linea}"),
        None => SISTEMA.to_owned(),
    }
}

/// Who runs a turn, as it goes to the turn's thread.
enum Trabajo {
    /// Komunta's own agent, with this AI.
    Propio(Box<dyn Proveedor>),
    /// An outside agent: the one kept from the last turn, if any.
    Externo {
        abierto: Option<Box<Abierto>>,
        id: String,
        agente: asistente::externo::Agente,
    },
}

impl Trabajo {
    /// Who runs it, for the trace: Komunta's own agent, or the outside
    /// one's id.
    fn nombre(&self) -> String {
        match self {
            Trabajo::Propio(_) => "komunta".to_owned(),
            Trabajo::Externo { id, .. } => id.clone(),
        }
    }

    /// Runs the turn on the project in `ruta`, under `gesto`.
    fn hacer(
        self,
        ruta: std::path::PathBuf,
        gesto: String,
        persona: &mut EnVentana,
        conversacion: &mut Conversacion,
        texto: &str,
    ) -> (Option<Box<Abierto>>, Result<Fin, String>) {
        match self {
            Trabajo::Propio(mut ia) => {
                let mut herramientas = Herramientas::new(ruta.to_string_lossy().into_owned(), gesto, persona.parada());
                let sistema = sistema(herramientas.linea_de_tiempo());
                let resultado = asistente::turno(
                    ia.as_mut(),
                    &mut herramientas,
                    persona,
                    conversacion,
                    &sistema,
                    texto,
                    Limites::default(),
                );
                (None, resultado)
            }
            Trabajo::Externo { abierto, id, agente } => {
                let abierto = abierto.map(|abierto| *abierto);
                let (abierto, resultado) =
                    asistente_externo::turno(abierto, &id, &agente, ruta, persona, conversacion, texto);
                (abierto.map(Box::new), resultado)
            }
        }
    }
}

/// What a tool does, as the chat shows it while it runs and after. An
/// outside agent names Komunta's tools after its server
/// (`mcp__komunta__clip_add`); the last part is the tool.
fn etiqueta(herramienta: &str) -> String {
    let herramienta = herramienta.rsplit("__").next().unwrap_or(herramienta);
    let clave = match herramienta {
        "timeline_read" => "timelineRead",
        "edit_undo" => "editUndo",
        "edit_redo" => "editRedo",
        "clip_add" => "clipAdd",
        "clips_add_sequence" => "clipsAddSequence",
        "clip_trim" => "clipTrim",
        "clips_split" => "clipsSplit",
        "clips_move" => "clipsMove",
        "clips_remove" => "clipsRemove",
        "clip_update" => "clipUpdate",
        "clip_speed" => "clipSpeed",
        "track_add" => "trackAdd",
        "clip_transform" => "clipTransform",
        "clip_frame" => "clipFrame",
        "clip_mask" => "clipMask",
        "clip_stabilize" => "clipStabilize",
        "timeline_background" => "timelineBackground",
        "title_add" => "titleAdd",
        "captions_add" => "captionsAdd",
        "keyframe_set" => "keyframeSet",
        "keyframe_clear" => "keyframeClear",
        "effects_list" => "effectsList",
        "effect_layer_add" => "effectLayerAdd",
        "effect_key_set" => "effectKeySet",
        "media_probe" => "mediaProbe",
        "media_import" => "mediaImport",
        "project_set_video" => "projectSetVideo",
        "edit_apply" => "editApply",
        "ver_fotograma" => "verFotograma",
        "zoom_en" => "zoomEn",
        "titulo_animado" => "tituloAnimado",
        "texto_detras" => "textoDetras",
        "seguir_cara" => "seguirCara",
        otra => return otra.to_owned(),
    };
    t(&format!("asistente.tool.{clave}"))
}

/// `texto` broken into lines `columnas` wide, its own line breaks kept.
fn filas(texto: &str, columnas: usize) -> Vec<SharedString> {
    texto
        .lines()
        .flat_map(|parrafo| {
            let partidas = montaje::guion::lineas(parrafo, columnas);
            if partidas.is_empty() { vec![String::new()] } else { partidas }
        })
        .map(SharedString::from)
        .collect()
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::{asistente_catalogo, asistente_recetas};

    #[test]
    fn every_tool_the_agent_has_has_its_label() {
        let mut nombres: Vec<String> =
            asistente_catalogo::OFRECIDAS.iter().map(|o| o.nombre.to_owned()).collect();
        nombres.extend(asistente_recetas::herramientas().into_iter().map(|h| h.nombre));
        for nombre in nombres {
            assert_ne!(etiqueta(&nombre), nombre, "{nombre} has no label");
        }
    }

    #[test]
    fn a_text_keeps_its_paragraphs_and_wraps_within_them() {
        let filas = filas("uno dos tres\n\ncuatro", 7);
        let filas: Vec<&str> = filas.iter().map(SharedString::as_str).collect();
        assert_eq!(filas, ["uno dos", "tres", "", "cuatro"]);
    }
}
