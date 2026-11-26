// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! The pane with someone else's agent in it: Gemini CLI, Claude Code or
//! Codex over ACP ([`asistente::externo`]), each with its own AI and its
//! own account, handed Komunta's MCP server in window mode so that, with
//! live mode on, it edits the project on screen - each call one Ctrl+Z,
//! a call that cannot be undone in place put to the person first.
//!
//! The agent's process and its session live as long as the conversation
//! ([`Abierto`]); they go with it, or when the agent or the project
//! changes.

use std::path::{Path, PathBuf};

use asistente::externo::{Agente, Ajuste, Servidor, Sesion};
use asistente::{Conversacion, Fin, Persona};

/// The model Gemini CLI runs on in Komunta: the newest Flash, quick with
/// tools.
const MODELO_GEMINI: &str = "gemini-3.8-flash";

/// What Komunta changes in Codex's settings for its sessions (the
/// codex-acp adapter reads them as JSON from `CODEX_CONFIG`): no skills.
/// With the person's 466 it overran its budget for them and said so in
/// the chat, in English, on every first turn (November 2026); Komunta's
/// tools are all it needs.
const AJUSTES_CODEX: &str = r#"{"skills":{"include_instructions":false,"bundled":{"enabled":false}}}"#;

/// The agents the pane can hold besides Komunta's own, by the id the
/// preferences keep and the name the person sees.
pub const AGENTES: [(&str, &str); 3] = [("gemini", "Gemini CLI"), ("claude", "Claude Code"), ("codex", "Codex")];

/// How to start the agent `id`. The Node ones go through `npx`, which
/// fetches them the first time. Gemini CLI is handed Komunta's Google key,
/// when there is one, and signed in with it: its own Google sign-in for
/// individuals is gone (November 2026). It is also told to trust the
/// project's folder: in a folder it does not trust it starts no MCP
/// server, Komunta's included, and has no tools to edit with. And it reads
/// Komunta's own settings for it, `ajustes_gemini` ([`ajustes_gemini`]),
/// over the person's. Claude Code and Codex use the account they are
/// signed in with (Codex, a ChatGPT plan: no API), Codex without skills
/// ([`AJUSTES_CODEX`]).
pub fn agente(id: &str, clave_google: Option<String>, ajustes_gemini: Option<&Path>) -> Option<Agente> {
    let (orden, argumentos): (&str, &[&str]) = match id {
        "gemini" => ("gemini", &["--acp"]),
        "claude" => ("npx", &["-y", "@agentclientprotocol/claude-agent-acp"]),
        "codex" => ("npx", &["-y", "@agentclientprotocol/codex-acp"]),
        _ => return None,
    };
    let gemini = id == "gemini";
    let clave = clave_google.filter(|_| gemini);
    let mut entorno: Vec<(String, String)> = Vec::new();
    if gemini {
        entorno.push(("GEMINI_CLI_TRUST_WORKSPACE".to_owned(), "true".to_owned()));
        // Its settings' model went unheeded in ACP: it asked 3.1 Pro.
        entorno.push(("GEMINI_MODEL".to_owned(), MODELO_GEMINI.to_owned()));
        if let Some(ajustes) = ajustes_gemini {
            let ruta = ajustes.display().to_string();
            entorno.push(("GEMINI_CLI_SYSTEM_SETTINGS_PATH".to_owned(), ruta));
        }
    }
    if id == "codex" {
        entorno.push(("CODEX_CONFIG".to_owned(), AJUSTES_CODEX.to_owned()));
    }
    let acceso = clave.is_some().then(|| "gemini-api-key".to_owned());
    entorno.extend(clave.map(|clave| ("GEMINI_API_KEY".to_owned(), clave)));
    Some(Agente {
        orden: programa(orden),
        argumentos: argumentos.iter().map(|&argumento| argumento.to_owned()).collect(),
        acceso,
        entorno,
    })
}

/// Writes Gemini CLI's settings for Komunta in `carpeta`, and says where:
/// they weigh over the person's own (Gemini CLI reads them as the
/// system's). No skills: Gemini
/// CLI puts every skill it finds in each request, 124 000 tokens with the
/// 712 of `~/.agents/skills` (November 2026), past the free tier's
/// 250 000 a minute in two calls - and Komunta's tools are all it needs.
pub fn ajustes_gemini(carpeta: &Path) -> std::io::Result<PathBuf> {
    let ajustes = serde_json::json!({"skills": {"enabled": false}});
    std::fs::create_dir_all(carpeta)?;
    let ruta = carpeta.join("gemini-cli.json");
    std::fs::write(&ruta, serde_json::to_string_pretty(&ajustes)?)?;
    Ok(ruta)
}

/// npm's programs are `.cmd` scripts on Windows, which a process is not
/// started from by their bare name.
fn programa(nombre: &str) -> String {
    if cfg!(windows) { format!("{nombre}.cmd") } else { nombre.to_owned() }
}

/// Komunta's MCP server as the agent starts it: this program, reaching
/// the open window.
fn servidor() -> Result<Servidor, String> {
    Ok(Servidor {
        nombre: "komunta".to_owned(),
        orden: std::env::current_exe().map_err(|error| error.to_string())?,
        argumentos: vec!["--mcp".to_owned(), "window".to_owned()],
    })
}

/// Where the agent `id` sits in a list of Komunta's own and then
/// [`AGENTES`]: 0 for Komunta's own, and for an id no longer listed.
pub fn indice(id: &str) -> i32 {
    AGENTES
        .iter()
        .position(|(agente, _)| *agente == id)
        .map_or(0, |indice| indice as i32 + 1)
}

/// The id of the agent at `indice` in that list: empty for Komunta's own.
pub fn id_de(indice: i32) -> String {
    usize::try_from(indice - 1)
        .ok()
        .and_then(|indice| AGENTES.get(indice))
        .map_or_else(String::new, |(agente, _)| (*agente).to_owned())
}

/// Komunta's own and then the outside agents, by name.
pub fn nombres() -> Vec<&'static str> {
    std::iter::once("Komunta").chain(AGENTES.iter().map(|(_, nombre)| *nombre)).collect()
}

/// An agent running, for the project in `ruta`.
#[derive(Debug)]
pub struct Abierto {
    id: String,
    ruta: PathBuf,
    sesion: Sesion,
}

impl Abierto {
    /// What the person can choose for its session.
    pub fn ajustes(&mut self) -> Vec<Ajuste> {
        self.sesion.ajustes().to_vec()
    }

    /// Sets its choice at `indice` to the value at `valor`.
    pub fn ajustar(&mut self, indice: usize, valor: usize) {
        let elegido = self.sesion.ajustes().get(indice).and_then(|ajuste| {
            let valor = ajuste.valores.get(valor)?;
            Some((ajuste.id.clone(), valor.id.clone()))
        });
        if let Some((id, valor)) = elegido {
            self.sesion.ajustar(&id, &valor);
        }
    }
}

/// One turn with `agente` (its id `id`) on the project in `ruta`, on the
/// turn's thread: the agent kept from the last turn if it is the same,
/// started otherwise. What runs is given back for the next turn.
pub fn turno(
    abierto: Option<Abierto>,
    id: &str,
    agente: &Agente,
    ruta: PathBuf,
    persona: &mut dyn Persona,
    conversacion: &mut Conversacion,
    texto: &str,
) -> (Option<Abierto>, Result<Fin, String>) {
    let abierto = match abierto.filter(|abierto| abierto.id == id && abierto.ruta == ruta) {
        Some(abierto) => abierto,
        None => match abrir(id, agente, ruta) {
            Ok(abierto) => abierto,
            Err(motivo) => return (None, Err(motivo)),
        },
    };
    let texto = if conversacion.mensajes.is_empty() {
        contexto(&abierto.ruta, texto)
    } else {
        texto.to_owned()
    };
    let mut abierto = abierto;
    let fin = abierto.sesion.turno(persona, conversacion, &texto);
    (Some(abierto), fin)
}

fn abrir(id: &str, agente: &Agente, ruta: PathBuf) -> Result<Abierto, String> {
    let sesion = Sesion::abrir(agente, ruta.clone(), vec![servidor()?])
        .map_err(|motivo| format!("{}: {motivo}", agente.orden))?;
    Ok(Abierto {
        id: id.to_owned(),
        ruta,
        sesion,
    })
}

/// The first words to the agent: where the project is and how it is
/// reached, before the person's.
fn contexto(ruta: &Path, texto: &str) -> String {
    format!(
        "La persona tiene abierto en Komunta, un editor de vídeo, el proyecto de la carpeta «{}». \
Edítalo con las herramientas del servidor MCP «komunta», pasando esa carpeta como `path`: \
cada cambio se ve al momento en la ventana y se deshace con Ctrl+Z. \
Mira cómo queda con preview_frame. Para montar un vídeo con IA a partir de una carpeta de material \
(también de Google Drive para escritorio, G:\\…) usa montaje_lanzar con esa carpeta como «material». \
No guardes, cierres ni exportes el proyecto: eso es de la persona. \
Contesta en su idioma.\n\n{texto}",
        ruta.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_listed_can_be_started_and_komunta_s_own_is_not_one() {
        for (id, _) in AGENTES {
            let agente = agente(id, None, None).expect(id);
            assert!(!agente.argumentos.is_empty());
        }
        assert!(agente("", None, None).is_none());
        let ajustes = Path::new("/datos/gemini-cli.json");
        let gemini = agente("gemini", Some("k".into()), Some(ajustes)).unwrap();
        assert_eq!(gemini.acceso.as_deref(), Some("gemini-api-key"));
        let confia = ("GEMINI_CLI_TRUST_WORKSPACE".to_owned(), "true".to_owned());
        assert!(gemini.entorno.contains(&confia), "else it starts no MCP server");
        assert!(gemini.entorno.iter().any(|(nombre, _)| nombre == "GEMINI_CLI_SYSTEM_SETTINGS_PATH"));
        let modelo = ("GEMINI_MODEL".to_owned(), MODELO_GEMINI.to_owned());
        assert!(gemini.entorno.contains(&modelo));
        let claude = agente("claude", Some("k".into()), Some(ajustes)).unwrap();
        assert!(claude.entorno.is_empty(), "the key and the settings are Gemini's");
    }

    #[test]
    fn codex_runs_on_its_sign_in_without_skills() {
        let codex = agente("codex", Some("k".into()), None).unwrap();
        assert!(codex.acceso.is_none(), "it uses the ChatGPT sign-in, not a key");
        let (nombre, valor) = codex.entorno.iter().find(|(nombre, _)| nombre == "CODEX_CONFIG").unwrap();
        assert_eq!(nombre, "CODEX_CONFIG");
        let ajustes: serde_json::Value = serde_json::from_str(valor).unwrap();
        assert_eq!(ajustes["skills"]["include_instructions"], false);
        assert!(!codex.entorno.iter().any(|(nombre, _)| nombre.starts_with("GEMINI")));
    }

    #[test]
    fn gemini_cli_s_settings_take_no_skills() {
        let carpeta = std::env::temp_dir().join(format!("komunta-gemini-cli-{}", std::process::id()));
        let ruta = ajustes_gemini(&carpeta).unwrap();
        let ajustes: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&ruta).unwrap()).unwrap();
        assert_eq!(ajustes["skills"]["enabled"], false);
        let _ = std::fs::remove_dir_all(&carpeta);
    }

    #[test]
    fn an_agent_s_place_in_the_list_round_trips() {
        for lugar in 0..=AGENTES.len() as i32 {
            assert_eq!(indice(&id_de(lugar)), lugar);
        }
        assert_eq!(id_de(0), "");
        assert_eq!(indice("desconocido"), 0);
        assert_eq!(nombres().len(), AGENTES.len() + 1);
    }

    #[test]
    fn the_first_words_say_where_the_project_is() {
        let dicho = contexto(Path::new("/p/boda"), "quita los silencios");
        assert!(dicho.contains("/p/boda"));
        assert!(dicho.ends_with("quita los silencios"));
    }
}
