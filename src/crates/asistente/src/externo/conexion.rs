// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The ACP side of an outside agent, on its own thread: the process
//! started, the session opened, each prompt sent and raced against Stop,
//! and what the agent says on the way turned into [`Suceso`]s.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    AuthenticateRequest, CancelNotification, ContentBlock, InitializeRequest, InitializeResponse, McpServer, McpServerStdio, NewSessionRequest,
    PermissionOption, PermissionOptionKind, PlanEntry, PlanEntryStatus, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigOptionValue, SessionId,
    SessionConfigOption, SessionModeState, SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
    SetSessionModeRequest, StopReason, TextContent, ToolCallUpdateFields,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, LineDirection};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use futures::future::{self, Either};

use super::ajustes::{self, Ajuste};
use super::{Agente, Servidor};
use crate::{Aprobacion, Clase, Estado, Fin, Tarea};

/// What the agent did, for the turn to play to the person.
#[derive(Debug)]
pub enum Suceso {
    /// A piece of what it says.
    Trozo(String),
    /// It is thinking.
    Pensando,
    /// It runs a tool: its name or its title.
    Herramienta(String),
    /// Its plan, whole.
    Plan(Vec<Tarea>),
    /// It asks to run a tool; the answer goes back on the channel.
    Permiso(Aprobacion, oneshot::Sender<bool>),
    /// What the person can choose for the session, as it stands now.
    Ajustes(Vec<Ajuste>),
    /// The turn ended.
    Fin(Result<Fin, String>),
}

/// What the session is asked to do.
pub enum Orden {
    /// A turn with the person's words.
    Turno(String),
    /// A choice set to one of its values: `id` to `valor`.
    Ajustar { id: String, valor: String },
}

/// What the thread starts with.
pub struct Arranque {
    pub agente: Agente,
    pub carpeta: PathBuf,
    pub servidores: Vec<Servidor>,
    pub pedidos: UnboundedReceiver<Orden>,
    pub cancelar: UnboundedReceiver<()>,
    pub sucesos: Sender<Suceso>,
    /// The session is open, with what can be chosen for it.
    pub listo: Sender<Result<Vec<Ajuste>, String>>,
}

/// Runs the agent until the session is dropped or the agent goes.
pub fn servir(arranque: Arranque) {
    let Arranque {
        agente,
        carpeta,
        servidores,
        mut pedidos,
        mut cancelar,
        sucesos,
        listo,
    } = arranque;
    let proceso = AcpAgent::new(
        AcpAgentConfig::new(&agente.orden)
            .args(agente.argumentos.clone())
            .envs(agente.entorno.clone()),
    )
    .with_debug(registrar);
    let avisos = sucesos.clone();
    let permisos = sucesos.clone();
    let abierto = listo.clone();
    let servido = futures::executor::block_on(
        Client
            .v1()
            .on_receive_notification(
                async move |aviso: SessionNotification, _| {
                    if let Some(suceso) = suceso_de(aviso.update) {
                        let _ = avisos.send(suceso);
                    }
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |pedido: RequestPermissionRequest, respuesta, _| {
                    let (si, espera) = oneshot::channel();
                    let _ = permisos.send(Suceso::Permiso(aprobacion_de(&pedido), si));
                    let si = espera.await.unwrap_or(false);
                    respuesta.respond(RequestPermissionResponse::new(desenlace(&pedido.options, si)))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(proceso, async move |conexion: ConnectionTo<Agent>| {
                let inicio = conexion
                    .send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                if let Some(acceso) = acceso(&inicio, agente.acceso.as_deref()) {
                    conexion
                        .send_request(AuthenticateRequest::new(acceso))
                        .block_task()
                        .await?;
                }
                let nueva = conexion
                    .send_request(NewSessionRequest::new(carpeta).mcp_servers(mcp(servidores)))
                    .block_task()
                    .await?;
                let sesion = nueva.session_id;
                let mut vigentes = Vigentes {
                    opciones: nueva.config_options.unwrap_or_default(),
                    modos: nueva.modes,
                };
                let _ = abierto.send(Ok(vigentes.ajustes()));
                while let Some(orden) = pedidos.next().await {
                    let suceso = match orden {
                        Orden::Turno(texto) => Suceso::Fin(preguntar(&conexion, &sesion, texto, &mut cancelar).await),
                        Orden::Ajustar { id, valor } => match vigentes.ajustar(&conexion, &sesion, id, valor).await {
                            Ok(ajustes) => Suceso::Ajustes(ajustes),
                            Err(error) => {
                                log::warn!("agente externo: no aceptó el ajuste: {error}");
                                continue;
                            }
                        },
                    };
                    let _ = sucesos.send(suceso);
                }
                Ok(())
            }),
    );
    if let Err(error) = servido {
        log::warn!("agente externo: {error}");
        let _ = listo.send(Err(error.to_string()));
    }
}

/// What the agent writes on stderr goes to the log, where a hang or a
/// failed sign-in shows; the protocol itself only at trace.
fn registrar(linea: &str, direccion: LineDirection) {
    match direccion {
        LineDirection::Stderr => log::info!("agente externo: {linea}"),
        _ => log::trace!("agente externo {direccion:?}: {linea}"),
    }
}

/// One prompt, sent and waited for; Stop on the way is passed on, and the
/// agent then ends the turn as cancelled.
async fn preguntar(
    conexion: &ConnectionTo<Agent>,
    sesion: &SessionId,
    texto: String,
    cancelar: &mut UnboundedReceiver<()>,
) -> Result<Fin, String> {
    // A Stop pressed between turns is not this turn's.
    while cancelar.try_recv().is_ok() {}
    let peticion = conexion
        .send_request(PromptRequest::new(
            sesion.clone(),
            vec![ContentBlock::Text(TextContent::new(texto))],
        ))
        .block_task();
    let respuesta = match future::select(Box::pin(peticion), cancelar.next()).await {
        Either::Left((respuesta, _)) => respuesta,
        Either::Right((_, peticion)) => {
            conexion
                .send_notification(CancelNotification::new(sesion.clone()))
                .map_err(|error| error.to_string())?;
            peticion.await
        }
    };
    match respuesta.map_err(|error| error.to_string())?.stop_reason {
        StopReason::Cancelled => Ok(Fin::Parada),
        StopReason::Refusal => Err("el agente se negó".to_owned()),
        _ => Ok(Fin::Respuesta),
    }
}

/// The session's choices as the agent last said them.
struct Vigentes {
    opciones: Vec<SessionConfigOption>,
    /// The older modes, from an agent that speaks them.
    modos: Option<SessionModeState>,
}

impl Vigentes {
    fn ajustes(&self) -> Vec<Ajuste> {
        ajustes::de_acp(&self.opciones, self.modos.as_ref())
    }

    /// Sets the choice `id` to `valor`, and says every choice as it now
    /// stands: a config option's answer brings them all, since one can
    /// change what another offers; an older mode changes only itself.
    async fn ajustar(
        &mut self,
        conexion: &ConnectionTo<Agent>,
        sesion: &SessionId,
        id: String,
        valor: String,
    ) -> Result<Vec<Ajuste>, String> {
        if id == ajustes::MODO_ANTIGUO {
            conexion
                .send_request(SetSessionModeRequest::new(sesion.clone(), valor.clone()))
                .block_task()
                .await
                .map_err(|error| error.to_string())?;
            if let Some(modos) = self.modos.as_mut() {
                modos.current_mode_id = valor.into();
            }
        } else {
            let pedido = SetSessionConfigOptionRequest::new(sesion.clone(), id, SessionConfigOptionValue::value_id(valor));
            let respuesta = conexion.send_request(pedido).block_task().await.map_err(|error| error.to_string())?;
            self.opciones = respuesta.config_options;
        }
        Ok(self.ajustes())
    }
}

/// The sign-in to ask for: `pedido`, if the agent offers it.
fn acceso(inicio: &InitializeResponse, pedido: Option<&str>) -> Option<String> {
    let pedido = pedido?;
    inicio
        .auth_methods
        .iter()
        .any(|metodo| metodo.id().to_string() == pedido)
        .then(|| pedido.to_owned())
}

/// The servers as ACP names them.
fn mcp(servidores: Vec<Servidor>) -> Vec<McpServer> {
    servidores
        .into_iter()
        .map(|servidor| {
            McpServer::Stdio(McpServerStdio::new(servidor.nombre, servidor.orden).args(servidor.argumentos))
        })
        .collect()
}

/// What an update is to the person, if anything.
fn suceso_de(update: SessionUpdate) -> Option<Suceso> {
    match update {
        SessionUpdate::AgentMessageChunk(trozo) => match trozo.content {
            ContentBlock::Text(texto) => Some(Suceso::Trozo(texto.text)),
            _ => None,
        },
        SessionUpdate::AgentThoughtChunk(_) => Some(Suceso::Pensando),
        SessionUpdate::ToolCall(llamada) => Some(Suceso::Herramienta(llamada.name.unwrap_or(llamada.title))),
        SessionUpdate::Plan(plan) => Some(Suceso::Plan(plan.entries.iter().map(tarea_de).collect())),
        SessionUpdate::ConfigOptionUpdate(cambio) => Some(Suceso::Ajustes(ajustes::de_acp(&cambio.config_options, None))),
        _ => None,
    }
}

fn tarea_de(entrada: &PlanEntry) -> Tarea {
    Tarea {
        texto: entrada.content.clone(),
        estado: match entrada.status {
            PlanEntryStatus::Pending => Estado::Pendiente,
            PlanEntryStatus::InProgress => Estado::EnCurso,
            _ => Estado::Hecha,
        },
    }
}

/// The agent's question as the person is asked it.
fn aprobacion_de(pedido: &RequestPermissionRequest) -> Aprobacion {
    Aprobacion {
        herramienta: nombre(&pedido.tool_call.fields),
        clase: Clase::Edicion,
        detalle: pedido.tool_call.fields.title.clone().unwrap_or_default(),
    }
}

fn nombre(campos: &ToolCallUpdateFields) -> String {
    campos.name.clone().or_else(|| campos.title.clone()).unwrap_or_default()
}

/// The option that says the person's answer: once, never for always -
/// each call is the person's to allow.
fn desenlace(opciones: &[PermissionOption], si: bool) -> RequestPermissionOutcome {
    let clases: &[PermissionOptionKind] = if si {
        &[PermissionOptionKind::AllowOnce]
    } else {
        &[PermissionOptionKind::RejectOnce, PermissionOptionKind::RejectAlways]
    };
    clases
        .iter()
        .find_map(|clase| opciones.iter().find(|opcion| opcion.kind == *clase))
        .map_or(RequestPermissionOutcome::Cancelled, |opcion| {
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(opcion.option_id.clone()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opcion(id: &str, kind: PermissionOptionKind) -> PermissionOption {
        PermissionOption::new(id.to_owned(), id.to_owned(), kind)
    }

    #[test]
    fn a_yes_allows_once_and_a_no_rejects() {
        let opciones = [
            opcion("siempre", PermissionOptionKind::AllowAlways),
            opcion("una", PermissionOptionKind::AllowOnce),
            opcion("no", PermissionOptionKind::RejectOnce),
        ];
        let elegida = |si| match desenlace(&opciones, si) {
            RequestPermissionOutcome::Selected(elegida) => elegida.option_id.to_string(),
            _ => "cancelada".to_owned(),
        };
        assert_eq!(elegida(true), "una");
        assert_eq!(elegida(false), "no");
        let solo_siempre = [opcion("siempre", PermissionOptionKind::AllowAlways)];
        assert!(matches!(
            desenlace(&solo_siempre, true),
            RequestPermissionOutcome::Cancelled
        ));
    }
}
