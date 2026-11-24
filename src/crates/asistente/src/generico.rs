// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! Every AI but Gemini - Claude, GPT, a model run by Ollama, Groq, xAI
//! and the rest genai speaks to - through genai's adapters, which turn
//! one conversation into each provider's wire format and keep the
//! provider's signed reasoning across tool turns.
//!
//! Gemini keeps its own adapter ([`crate::gemini`]): genai cannot put a
//! picture inside a function's response, nor narrow the calls with
//! `allowedFunctionNames`, and the quota the montage shares is counted
//! there. Here a frame a tool shows goes in a message of its own right
//! after the results, and a step that may only read is offered only the
//! tools that read.
//!
//! genai is asynchronous; a turn runs on a thread of its own, so each
//! request is driven to its end on a small runtime of that thread.

use std::time::Duration;

use base64::Engine;
use genai::chat::{
    Binary, ChatMessage, ChatOptions, ChatRequest, ChatResponse, ChatRole, ContentPart, MessageContent,
    ReasoningEffort, Tool, ToolCall, ToolChoice, ToolResponse,
};
use genai::resolver::{AuthData, AuthResolver};
use genai::{Client, ModelIden};
use serde_json::json;

use crate::conversacion::{Mensaje, Parte, Rol};
use crate::herramienta::Imagen;
use crate::proveedor::{Esfuerzo, Modo, Pedido, Proveedor};

/// How long one answer may take before the next model is asked.
const ESPERA: Duration = Duration::from_secs(60);

/// The key a message's raw form is kept under: only this adapter reads it.
const CRUDO: &str = "genai";

/// An AI reached through genai, with the models to try in order.
pub struct Generico {
    cliente: Client,
    modelos: Vec<String>,
    motor: tokio::runtime::Runtime,
    esfuerzo: Esfuerzo,
}

impl Generico {
    /// `modelos` tried in order - a name genai knows the provider of
    /// (`claude-…`, `gpt-…`) or one with its adapter before it
    /// (`ollama::llama3.3`) - with `clave` for whichever needs one.
    pub fn new(clave: Option<String>, modelos: Vec<String>) -> Result<Self, String> {
        let auth = AuthResolver::from_resolver_fn(move |_: ModelIden| Ok(clave.clone().map(AuthData::from_single)));
        let cliente = Client::builder()
            .with_auth_resolver(auth)
            .build()
            .map_err(|error| format!("no se pudo preparar el cliente de IA: {error}"))?;
        let motor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("no se pudo preparar el cliente de IA: {error}"))?;
        Ok(Self {
            cliente,
            modelos,
            motor,
            esfuerzo: Esfuerzo::Bajo,
        })
    }

    /// The same, thinking this much before each answer.
    pub fn con_esfuerzo(mut self, esfuerzo: Esfuerzo) -> Self {
        self.esfuerzo = esfuerzo;
        self
    }

    fn pedir(&self, modelo: &str, pedido: &Pedido<'_>) -> Result<ChatResponse, String> {
        let (peticion, opciones) = peticion(pedido, self.esfuerzo);
        let respuesta = self.motor.block_on(async {
            tokio::time::timeout(ESPERA, self.cliente.exec_chat(modelo, peticion, Some(&opciones))).await
        });
        match respuesta {
            Ok(Ok(respuesta)) => Ok(respuesta),
            Ok(Err(error)) => Err(format!("{modelo}: {error}")),
            Err(_) => Err(format!("{modelo} no contestó en {} s", ESPERA.as_secs())),
        }
    }
}

impl Proveedor for Generico {
    fn nombre(&self) -> String {
        self.modelos.first().cloned().unwrap_or_else(|| "IA".to_owned())
    }

    /// The first model that answers; each failure is logged and the next
    /// one asked.
    fn responder(&mut self, pedido: &Pedido<'_>) -> Result<Mensaje, String> {
        let mut fallos = Vec::new();
        for modelo in &self.modelos {
            match self.pedir(modelo, pedido) {
                Ok(respuesta) => return Ok(mensaje_de(respuesta)),
                Err(fallo) => {
                    log::warn!("asistente: {fallo}");
                    fallos.push(fallo);
                }
            }
        }
        Err(match fallos.last() {
            Some(ultimo) => format!("ningún modelo contestó; el último: {ultimo}"),
            None => "no hay ningún modelo con el que hablar".to_owned(),
        })
    }
}

/// The request for `pedido`, and its options: reasoning kept low, as a
/// chat wants it, and the calls the step allows.
fn peticion(pedido: &Pedido<'_>, esfuerzo: Esfuerzo) -> (ChatRequest, ChatOptions) {
    let herramientas: Vec<Tool> = pedido
        .herramientas
        .iter()
        .filter(|h| match &pedido.modo {
            Modo::SoloLeer(nombres) => nombres.contains(&h.nombre),
            Modo::Libre | Modo::Ninguna => true,
        })
        .map(|h| {
            Tool::new(h.nombre.clone())
                .with_description(h.descripcion.clone())
                .with_schema(h.esquema.clone())
        })
        .collect();
    let mensajes: Vec<ChatMessage> = pedido.mensajes.iter().flat_map(mensajes_de).collect();
    let peticion = ChatRequest::new(mensajes)
        .with_system(pedido.sistema)
        .with_tools(herramientas);
    let eleccion = match pedido.modo {
        Modo::Ninguna => ToolChoice::None,
        Modo::Libre | Modo::SoloLeer(_) => ToolChoice::Auto,
    };
    let opciones = ChatOptions::default()
        .with_tool_choice(eleccion)
        .with_reasoning_effort(razonamiento(esfuerzo));
    (peticion, opciones)
}

/// `esfuerzo` as genai names it.
fn razonamiento(esfuerzo: Esfuerzo) -> ReasoningEffort {
    match esfuerzo {
        Esfuerzo::Bajo => ReasoningEffort::Low,
        Esfuerzo::Medio => ReasoningEffort::Medium,
        Esfuerzo::Alto => ReasoningEffort::High,
    }
}

/// A message of the conversation as genai's: one of the AI's own goes
/// back as it came, signatures and all; the tools' results go as one
/// message, and the frames they show in another right after it.
fn mensajes_de(mensaje: &Mensaje) -> Vec<ChatMessage> {
    if mensaje.rol == Rol::Modelo {
        let propio = mensaje
            .crudo
            .as_ref()
            .and_then(|crudo| serde_json::from_value::<ChatMessage>(crudo.get(CRUDO)?.clone()).ok());
        return vec![propio.unwrap_or_else(|| del_modelo(&mensaje.partes))];
    }
    let mut textos = Vec::new();
    let mut resultados = Vec::new();
    let mut imagenes = Vec::new();
    for parte in &mensaje.partes {
        match parte {
            Parte::Texto(texto) => textos.push(ContentPart::from_text(texto.clone())),
            Parte::Resultado {
                id,
                nombre,
                valor,
                error,
                imagen,
            } => {
                let contenido = if *error { json!({"error": valor}) } else { valor.clone() };
                resultados.push(ToolResponse::new(id.clone(), contenido.to_string()).with_fn_name(nombre.clone()));
                if let Some(imagen) = imagen {
                    imagenes.extend(partes_de_imagen(nombre, imagen));
                }
            }
            Parte::Llamada { .. } => {}
        }
    }
    let mut salida = Vec::new();
    if !resultados.is_empty() {
        salida.push(ChatMessage::new(ChatRole::Tool, MessageContent::from_tool_responses(resultados)));
    }
    if !imagenes.is_empty() {
        salida.push(ChatMessage::user(MessageContent::from_parts(imagenes)));
    }
    if !textos.is_empty() {
        salida.push(ChatMessage::user(MessageContent::from_parts(textos)));
    }
    salida
}

/// A message of the AI's that genai did not make: words and calls.
fn del_modelo(partes: &[Parte]) -> ChatMessage {
    let partes: Vec<ContentPart> = partes
        .iter()
        .filter_map(|parte| match parte {
            Parte::Texto(texto) => Some(ContentPart::from_text(texto.clone())),
            Parte::Llamada { id, nombre, argumentos } => Some(ContentPart::ToolCall(ToolCall {
                call_id: id.clone(),
                fn_name: nombre.clone(),
                fn_arguments: argumentos.clone(),
                thought_signatures: None,
            })),
            Parte::Resultado { .. } => None,
        })
        .collect();
    ChatMessage::assistant(MessageContent::from_parts(partes))
}

/// A frame a tool shows, said and shown.
fn partes_de_imagen(herramienta: &str, imagen: &Imagen) -> [ContentPart; 2] {
    let datos = base64::engine::general_purpose::STANDARD.encode(&imagen.datos);
    [
        ContentPart::from_text(format!("Fotograma que devuelve {herramienta} ({})", imagen.nota)),
        ContentPart::Binary(Binary::from_base64(imagen.mime.clone(), datos, None)),
    ]
}

/// genai's answer as a message of the conversation, its raw form kept to
/// go back as it came.
fn mensaje_de(respuesta: ChatResponse) -> Mensaje {
    let partes = respuesta
        .content
        .parts()
        .iter()
        .filter_map(|parte| match parte {
            ContentPart::Text(texto) if !texto.trim().is_empty() => Some(Parte::Texto(texto.clone())),
            ContentPart::ToolCall(llamada) => Some(Parte::Llamada {
                id: llamada.call_id.clone(),
                nombre: llamada.fn_name.clone(),
                argumentos: llamada.fn_arguments.clone(),
            }),
            _ => None,
        })
        .collect();
    let propio = ChatMessage::assistant(respuesta.content).with_reasoning_content(respuesta.reasoning_content);
    Mensaje {
        rol: Rol::Modelo,
        partes,
        crudo: serde_json::to_value(propio).ok().map(|valor| json!({ CRUDO: valor })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herramienta::{Clase, Herramienta};

    fn herramientas() -> Vec<Herramienta> {
        vec![
            Herramienta::nueva("leer", "lee", json!({"type": "object"}), Clase::Lectura, crate::NUCLEO),
            Herramienta::nueva("cortar", "corta", json!({"type": "object"}), Clase::Edicion, crate::NUCLEO),
        ]
    }

    fn resultado(imagen: Option<Imagen>) -> Mensaje {
        Mensaje {
            rol: Rol::Usuario,
            partes: vec![Parte::Resultado {
                id: "call_1".into(),
                nombre: "ver_fotograma".into(),
                valor: json!({"tiempo": 2.0}),
                error: false,
                imagen,
            }],
            crudo: None,
        }
    }

    #[test]
    fn a_step_that_may_only_read_is_offered_only_the_tools_that_read() {
        let todas = herramientas();
        let mensajes = [Mensaje::del_usuario("hola")];
        let pedido = |modo| Pedido {
            sistema: "s",
            mensajes: &mensajes,
            herramientas: &todas,
            modo,
        };
        let (lectura, opciones) = peticion(&pedido(Modo::SoloLeer(vec!["leer".into()])), Esfuerzo::Bajo);
        let nombres: Vec<String> = lectura.tools.unwrap().into_iter().map(|t| t.name.to_string()).collect();
        assert_eq!(nombres, ["leer"]);
        assert_eq!(opciones.tool_choice, Some(ToolChoice::Auto));
        let (_, opciones) = peticion(&pedido(Modo::Ninguna), Esfuerzo::Alto);
        assert_eq!(opciones.tool_choice, Some(ToolChoice::None));
        assert!(matches!(opciones.reasoning_effort, Some(ReasoningEffort::High)));
    }

    #[test]
    fn a_frame_goes_right_after_the_results_in_a_message_of_its_own() {
        let imagen = Imagen {
            mime: "image/jpeg".into(),
            datos: vec![0xff, 0xd8],
            nota: "t=2.00 s".into(),
        };
        let mensajes = mensajes_de(&resultado(Some(imagen)));
        assert_eq!(mensajes.len(), 2);
        assert_eq!(mensajes[0].role, ChatRole::Tool);
        assert_eq!(mensajes[1].role, ChatRole::User);
        assert!(mensajes[1].content.parts().iter().any(ContentPart::is_image));
        assert_eq!(mensajes_de(&resultado(None)).len(), 1);
    }

    #[test]
    fn the_ai_s_own_message_goes_back_as_it_came_signature_and_all() {
        let llamada = ToolCall {
            call_id: "call_7".into(),
            fn_name: "cortar".into(),
            fn_arguments: json!({"t": 2}),
            thought_signatures: None,
        };
        let contenido = MessageContent::from_parts(vec![
            ContentPart::ThoughtSignature("firma".into()),
            ContentPart::ToolCall(llamada),
        ]);
        let propio = ChatMessage::assistant(contenido);
        let mensaje = Mensaje {
            rol: Rol::Modelo,
            partes: vec![Parte::Llamada {
                id: "call_7".into(),
                nombre: "cortar".into(),
                argumentos: json!({"t": 2}),
            }],
            crudo: Some(json!({ CRUDO: serde_json::to_value(&propio).unwrap() })),
        };
        let devuelto = mensajes_de(&mensaje);
        assert_eq!(devuelto[0].content.thought_signatures(), ["firma"]);
        // A message another provider made is rebuilt from its parts.
        let ajeno = Mensaje {
            crudo: Some(json!({"parts": []})),
            ..mensaje
        };
        assert_eq!(mensajes_de(&ajeno)[0].content.tool_calls().len(), 1);
    }
}
