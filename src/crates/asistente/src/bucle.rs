// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Komunikia

//! One turn of the agent: the person's words go in, the AI is asked, the
//! tools it calls are checked, approved when they must be, run and their
//! results handed back, and so on until it answers in words, the person
//! stops it, or the budget runs out.
//!
//! Each call goes through the same gate, in order: the harness's own tools
//! (the plan, the search for tools), the guards, the executor's check, the
//! person's yes for what cannot be undone in place, the Stop button, and
//! only then the tool. Every call gets its answer, run or not - a call
//! left unanswered is refused by the provider - and what the harness has
//! to tell the AI rides inside the last of them.

use std::time::Instant;

use serde_json::{Value, json};

use crate::catalogo;
use crate::conversacion::{Conversacion, Mensaje, Parte, Rol};
use crate::guardia::{Guardia, Limites};
use crate::herramienta::{Herramienta, Salida, Tipo};
use crate::paso::{self, aviso_de_margen, nota_del_arnes};
use crate::plan::{self, PLAN_ESCRIBIR};
use crate::proveedor::{Aprobacion, Ejecutor, Evento, Pedido, Persona, Proveedor};

/// Steps without the plan written again before the AI is reminded of it.
const RECORDAR_PLAN: usize = 3;

/// How a turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fin {
    /// The AI answered; its words are the conversation's last.
    Respuesta,
    /// The person pressed Stop.
    Parada,
}

/// Runs a turn with `texto` from the person under the standing instruction
/// `sistema`. When the AI fails, the conversation is left as it was before
/// the turn - what the tools did stays done, and is the editor's to undo -
/// and the failure is said in a sentence.
pub fn turno(
    proveedor: &mut dyn Proveedor,
    ejecutor: &mut dyn Ejecutor,
    persona: &mut dyn Persona,
    conversacion: &mut Conversacion,
    sistema: &str,
    texto: &str,
    limites: Limites,
) -> Result<Fin, String> {
    let inicio = Instant::now();
    let antes = conversacion.mensajes.len();
    conversacion.mensajes.push(Mensaje::del_usuario(texto));
    let mut todas = ejecutor.herramientas();
    todas.extend([plan::herramienta(), catalogo::herramienta()]);
    let sistema = format!("{sistema}\n\n{}", catalogo::indice(&todas));
    let mut vuelta = Vuelta {
        ejecutor,
        persona,
        conversacion,
        guardia: Guardia::new(limites),
        todas,
        sin_plan: 0,
    };
    let resultado = vuelta.pasos(proveedor, &sistema);
    let pasos = vuelta.conversacion.mensajes[antes..].iter().filter(|m| m.rol == Rol::Modelo).count();
    log::info!("asistente: turn {} ms, {pasos} steps, {resultado:?}", inicio.elapsed().as_millis());
    if resultado.is_err() {
        vuelta.conversacion.mensajes.truncate(antes);
    }
    resultado
}

/// A turn under way.
struct Vuelta<'a> {
    ejecutor: &'a mut dyn Ejecutor,
    persona: &'a mut dyn Persona,
    conversacion: &'a mut Conversacion,
    guardia: Guardia,
    /// Every tool there is, the harness's own among them.
    todas: Vec<Herramienta>,
    /// Steps since the plan was last written.
    sin_plan: usize,
}

/// What the calls of one answer gave.
struct Ronda {
    resultados: Vec<Parte>,
    notas: Vec<String>,
    /// Every call was run and did what it was asked.
    todo_bien: bool,
    /// Some call only read: the AI has yet to see what it found.
    leyo: bool,
}

impl Vuelta<'_> {
    fn pasos(&mut self, proveedor: &mut dyn Proveedor, sistema: &str) -> Result<Fin, String> {
        for n in 0.. {
            if self.persona.parar() {
                return Ok(Fin::Parada);
            }
            if self.guardia.agotado(n) {
                break;
            }
            let paso = paso::preparar(self.conversacion, &self.todas, &self.guardia, n);
            self.persona.avisar(Evento::Pensando);
            let respuesta = proveedor.responder(&Pedido {
                sistema,
                mensajes: &paso.mensajes,
                herramientas: &paso.herramientas,
                modo: paso.modo,
            })?;
            let llamadas: Vec<(String, String, Value)> = respuesta
                .llamadas()
                .map(|(id, nombre, args)| (id.to_owned(), nombre.to_owned(), args.clone()))
                .collect();
            let dicho = respuesta.texto();
            self.conversacion.mensajes.push(respuesta);
            if llamadas.is_empty() {
                return Ok(Fin::Respuesta);
            }
            let mut ronda = self.ejecutar(&llamadas);
            self.sin_plan += 1;
            ronda.notas.extend(self.avisos(n + 1));
            let parado = self.persona.parar();
            let hecho = !dicho.trim().is_empty() && ronda.todo_bien && !ronda.leyo;
            self.responder(ronda);
            if parado {
                return Ok(Fin::Parada);
            }
            if hecho {
                return Ok(Fin::Respuesta);
            }
            if !dicho.trim().is_empty() {
                self.persona.avisar(Evento::Texto(dicho));
            }
        }
        Err(format!(
            "La IA siguió pidiendo herramientas sin contestar; se ha parado tras {} pasos.",
            self.conversacion.mensajes.iter().filter(|m| m.rol == Rol::Modelo).count()
        ))
    }

    /// Hands the AI the results of its calls, the harness's notes inside
    /// the last.
    fn responder(&mut self, ronda: Ronda) {
        let mut resultados = ronda.resultados;
        if !ronda.notas.is_empty()
            && let Some(Parte::Resultado { valor, .. }) = resultados.last_mut()
        {
            valor["nota_komunta"] = json!(nota_del_arnes(&ronda.notas.join(" ")));
        }
        self.conversacion.mensajes.push(Mensaje {
            rol: Rol::Usuario,
            partes: resultados,
            crudo: None,
        });
    }

    /// What the AI should be told before step `siguiente`: its plan when it
    /// has not written it for a while, and how much budget is left.
    fn avisos(&mut self, siguiente: usize) -> Vec<String> {
        let mut avisos = Vec::new();
        let recuerdo = plan::recordatorio(&self.conversacion.plan);
        if !recuerdo.is_empty() && self.sin_plan >= RECORDAR_PLAN {
            avisos.push(recuerdo);
            self.sin_plan = 0;
        }
        avisos.extend(aviso_de_margen(self.guardia.margen(siguiente)).map(str::to_owned));
        avisos
    }

    /// Runs the calls of one answer, in order.
    fn ejecutar(&mut self, llamadas: &[(String, String, Value)]) -> Ronda {
        let mut ronda = Ronda {
            resultados: Vec::with_capacity(llamadas.len()),
            notas: Vec::new(),
            todo_bien: true,
            leyo: false,
        };
        for (id, nombre, argumentos) in llamadas {
            let salida = self.llamar(nombre, argumentos, &mut ronda);
            ronda.todo_bien &= salida.ok();
            ronda.resultados.push(Parte::Resultado {
                id: id.clone(),
                nombre: nombre.clone(),
                valor: salida.para_la_ia(),
                error: !salida.ok(),
                imagen: salida.imagen,
            });
        }
        ronda
    }

    /// One call, through the gate.
    fn llamar(&mut self, nombre: &str, argumentos: &Value, ronda: &mut Ronda) -> Salida {
        if self.persona.parar() {
            return Salida::mal(Tipo::Cancelado, "La persona pulsó Parar antes de que se ejecutara.");
        }
        match nombre {
            PLAN_ESCRIBIR => return self.escribir_plan(argumentos),
            catalogo::BUSCAR => return self.buscar(argumentos),
            _ => {}
        }
        let Some(herramienta) = self.todas.iter().find(|h| h.nombre == nombre).cloned() else {
            return Salida::mal(Tipo::Error, format!("No existe la herramienta {nombre}."))
                .con_sugerencia("Mira el índice de herramientas o usa herramientas_buscar.");
        };
        ronda.leyo |= herramienta.clase.solo_lee();
        // A tool of a family not asked for yet is the AI knowing the index
        // well: it is offered from now on.
        self.conversacion.familias.insert(herramienta.familia.clone());
        if self.guardia.bloqueadas.contains(nombre) {
            return Salida::mal(Tipo::Bloqueado, format!("{nombre} está bloqueada en este turno."))
                .con_sugerencia("Sigue de otra forma o explica a la persona qué falta.");
        }
        let salida = self.ejecutar_aprobada(&herramienta, argumentos);
        ronda
            .notas
            .extend(self.guardia.registrar(nombre, argumentos, !salida.ok()));
        salida
    }

    /// Checks, asks the person when the tool's kind says so, and runs.
    fn ejecutar_aprobada(&mut self, herramienta: &Herramienta, argumentos: &Value) -> Salida {
        let nombre = herramienta.nombre.as_str();
        if let Err(motivo) = self.ejecutor.comprobar(nombre, argumentos) {
            return Salida::mal(Tipo::Error, motivo);
        }
        if self.ejecutor.pide_aprobacion(herramienta, argumentos) {
            let aprobacion = Aprobacion {
                herramienta: nombre.to_owned(),
                clase: herramienta.clase,
                detalle: self.ejecutor.detalle(nombre, argumentos),
            };
            if !self.persona.aprobar(&aprobacion) {
                return Salida::mal(Tipo::Rechazado, "La persona no lo aprobó.")
                    .con_sugerencia("Pregúntale qué prefiere antes de intentarlo de otra forma.");
            }
            if self.persona.parar() {
                return Salida::mal(Tipo::Cancelado, "La persona pulsó Parar.");
            }
        }
        self.persona.avisar(Evento::Herramienta(nombre.to_owned()));
        let inicio = Instant::now();
        let salida = self.ejecutor.ejecutar(nombre, argumentos.clone());
        log::info!(
            "asistente: tool {nombre} {} ms{}",
            inicio.elapsed().as_millis(),
            if salida.ok() { "" } else { ", failed" }
        );
        salida
    }

    fn escribir_plan(&mut self, argumentos: &Value) -> Salida {
        match plan::leer(argumentos) {
            Ok(tareas) => {
                self.conversacion.plan = tareas.clone();
                self.sin_plan = 0;
                self.persona.avisar(Evento::Plan(tareas));
                Salida::bien(json!("plan guardado"))
            }
            Err(motivo) => Salida::mal(Tipo::Error, motivo),
        }
    }

    fn buscar(&mut self, argumentos: &Value) -> Salida {
        let consulta = argumentos["consulta"].as_str().unwrap_or_default();
        let familias = catalogo::buscar(&self.todas, consulta);
        if familias.is_empty() {
            return Salida::mal(Tipo::Vacio, format!("Ninguna familia coincide con «{consulta}»."))
                .con_sugerencia("Usa un nombre de familia del índice de herramientas.");
        }
        self.conversacion.familias.extend(familias.iter().cloned());
        Salida::bien(catalogo::respuesta(&self.todas, &familias))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use serde_json::json;

    use super::*;
    use crate::herramienta::{Clase, NUCLEO};
    use crate::plan::Tarea;
    use crate::proveedor::Modo;

    /// An AI that answers from a script, and remembers what it was shown.
    struct Guionizado {
        respuestas: VecDeque<Result<Mensaje, String>>,
        vistos: Vec<usize>,
        herramientas: Vec<Vec<String>>,
        modos: Vec<Modo>,
    }

    impl Proveedor for Guionizado {
        fn nombre(&self) -> String {
            "guion".into()
        }

        fn responder(&mut self, pedido: &Pedido<'_>) -> Result<Mensaje, String> {
            self.vistos.push(pedido.mensajes.len());
            self.herramientas
                .push(pedido.herramientas.iter().map(|h| h.nombre.clone()).collect());
            self.modos.push(pedido.modo.clone());
            self.respuestas.pop_front().expect("one answer more than scripted was asked for")
        }
    }

    /// Tools that write down what they were asked.
    #[derive(Default)]
    struct Apuntador {
        llamadas: Vec<(String, Value)>,
        /// Every tool fails.
        falla: bool,
        /// `comprobar` refuses every call.
        rechaza: bool,
    }

    impl Ejecutor for Apuntador {
        fn herramientas(&self) -> Vec<Herramienta> {
            vec![
                Herramienta::nueva("timeline_read", "Lee", json!({}), Clase::Lectura, NUCLEO),
                Herramienta::nueva("clips_split", "Corta", json!({}), Clase::Edicion, NUCLEO),
                Herramienta::nueva("clips_remove", "Quita", json!({}), Clase::Destructiva, "clips"),
                Herramienta::nueva("zoom_en", "Acerca la cámara", json!({}), Clase::Edicion, "animacion"),
            ]
        }

        fn comprobar(&mut self, _: &str, _: &Value) -> Result<(), String> {
            if self.rechaza { Err("no existe el clip c7".into()) } else { Ok(()) }
        }

        fn ejecutar(&mut self, nombre: &str, argumentos: Value) -> Salida {
            self.llamadas.push((nombre.to_owned(), argumentos));
            if self.falla { Salida::mal(Tipo::Error, "falló") } else { Salida::bien(json!({"hecho": true})) }
        }
    }

    /// The person: says yes or no to everything, may have pressed Stop,
    /// and keeps what it was told.
    #[derive(Default)]
    struct Alguien {
        dice_no: bool,
        parar_tras: Option<usize>,
        eventos: Vec<Evento>,
        aprobaciones: Vec<Aprobacion>,
    }

    impl Persona for Alguien {
        fn avisar(&mut self, evento: Evento) {
            self.eventos.push(evento);
        }

        fn aprobar(&mut self, aprobacion: &Aprobacion) -> bool {
            self.aprobaciones.push(aprobacion.clone());
            !self.dice_no
        }

        fn parar(&self) -> bool {
            self.parar_tras
                .is_some_and(|n| self.eventos.iter().filter(|e| matches!(e, Evento::Herramienta(_))).count() >= n)
        }
    }

    fn palabras(texto: &str) -> Result<Mensaje, String> {
        Ok(Mensaje {
            rol: Rol::Modelo,
            partes: vec![Parte::Texto(texto.into())],
            crudo: None,
        })
    }

    fn llamadas(nombres: &[(&str, Value)]) -> Result<Mensaje, String> {
        Ok(Mensaje {
            rol: Rol::Modelo,
            partes: nombres
                .iter()
                .enumerate()
                .map(|(i, (nombre, args))| Parte::Llamada {
                    id: format!("c{i}"),
                    nombre: (*nombre).into(),
                    argumentos: args.clone(),
                })
                .collect(),
            crudo: None,
        })
    }

    fn llama(nombre: &str) -> Result<Mensaje, String> {
        llamadas(&[(nombre, json!({"time": 1.5}))])
    }

    fn llama_y_dice(nombre: &str, texto: &str) -> Result<Mensaje, String> {
        let mut mensaje = llama(nombre)?;
        mensaje.partes.push(Parte::Texto(texto.into()));
        Ok(mensaje)
    }

    fn guion(respuestas: Vec<Result<Mensaje, String>>) -> Guionizado {
        Guionizado {
            respuestas: respuestas.into(),
            vistos: Vec::new(),
            herramientas: Vec::new(),
            modos: Vec::new(),
        }
    }

    struct Prueba {
        ia: Guionizado,
        herramientas: Apuntador,
        persona: Alguien,
        conversacion: Conversacion,
    }

    impl Prueba {
        fn new(respuestas: Vec<Result<Mensaje, String>>) -> Self {
            Self {
                ia: guion(respuestas),
                herramientas: Apuntador::default(),
                persona: Alguien::default(),
                conversacion: Conversacion::default(),
            }
        }

        fn turno(&mut self, texto: &str) -> Result<Fin, String> {
            self.turno_con(texto, Limites::default())
        }

        fn turno_con(&mut self, texto: &str, limites: Limites) -> Result<Fin, String> {
            turno(
                &mut self.ia,
                &mut self.herramientas,
                &mut self.persona,
                &mut self.conversacion,
                "",
                texto,
                limites,
            )
        }

        /// The results the AI was handed, in order.
        fn resultados(&self) -> Vec<Value> {
            self.conversacion
                .mensajes
                .iter()
                .flat_map(|m| &m.partes)
                .filter_map(|p| match p {
                    Parte::Resultado { valor, .. } => Some(valor.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    #[test]
    fn a_plain_answer_is_one_step() {
        let mut prueba = Prueba::new(vec![palabras("Hola")]);
        assert_eq!(prueba.turno("hola"), Ok(Fin::Respuesta));
        assert_eq!(prueba.conversacion.mensajes.len(), 2);
        assert_eq!(prueba.conversacion.respuesta(), "Hola");
    }

    #[test]
    fn the_tools_asked_for_run_and_their_results_go_back_before_the_answer() {
        let mut prueba = Prueba::new(vec![llama("clips_split"), palabras("Cortado.")]);
        prueba.turno("corta").unwrap();
        assert_eq!(prueba.herramientas.llamadas, [("clips_split".to_owned(), json!({"time": 1.5}))]);
        assert_eq!(prueba.ia.vistos, [1, 3]);
        assert_eq!(prueba.resultados(), [json!({"ok": true, "datos": {"hecho": true}})]);
        assert_eq!(prueba.conversacion.respuesta(), "Cortado.");
        assert!(prueba.persona.eventos.contains(&Evento::Herramienta("clips_split".into())));
    }

    #[test]
    fn words_beside_edits_that_succeed_end_the_turn_but_not_beside_reads() {
        let mut prueba = Prueba::new(vec![llama_y_dice("clips_split", "Cortado.")]);
        prueba.turno("corta").unwrap();
        assert_eq!(prueba.ia.vistos, [1], "one ask");

        let mut prueba = Prueba::new(vec![llama_y_dice("timeline_read", "Miro..."), palabras("Hay 3 clips.")]);
        prueba.turno("¿qué hay?").unwrap();
        assert_eq!(prueba.ia.vistos.len(), 2, "what was read is seen before answering");
        assert_eq!(prueba.conversacion.respuesta(), "Hay 3 clips.");
    }

    #[test]
    fn the_tool_list_only_grows_and_a_family_is_offered_once_asked_for() {
        let mut prueba = Prueba::new(vec![
            llamadas(&[("herramientas_buscar", json!({"consulta": "animacion"}))]),
            llama("zoom_en"),
            palabras("Hecho."),
        ]);
        prueba.turno("haz zoom").unwrap();
        assert!(!prueba.ia.herramientas[0].contains(&"zoom_en".to_owned()));
        assert!(prueba.ia.herramientas[1].contains(&"zoom_en".to_owned()));
        assert_eq!(prueba.ia.herramientas[1], prueba.ia.herramientas[2]);
        assert_eq!(prueba.herramientas.llamadas.len(), 1);
    }

    #[test]
    fn a_call_that_fails_its_check_is_not_run_nor_put_to_the_person() {
        let mut prueba = Prueba::new(vec![llama("clips_remove"), palabras("No existe.")]);
        prueba.herramientas.rechaza = true;
        prueba.turno("borra c7").unwrap();
        assert!(prueba.herramientas.llamadas.is_empty());
        assert!(prueba.persona.aprobaciones.is_empty());
        assert_eq!(prueba.resultados()[0]["error"], "no existe el clip c7");
    }

    #[test]
    fn what_cannot_be_undone_in_place_waits_for_the_person_and_a_no_is_said_to_the_ai() {
        let mut prueba = Prueba::new(vec![llama("clips_remove"), palabras("Vale, no lo borro.")]);
        prueba.persona.dice_no = true;
        prueba.turno("borra").unwrap();
        assert_eq!(prueba.persona.aprobaciones[0].herramienta, "clips_remove");
        assert!(prueba.herramientas.llamadas.is_empty());
        assert_eq!(prueba.resultados()[0]["tipo"], "rechazado");
    }

    #[test]
    fn stop_answers_every_call_left_and_ends_the_turn() {
        let mut prueba = Prueba::new(vec![llamadas(&[
            ("clips_split", json!({"time": 1})),
            ("clips_split", json!({"time": 2})),
        ])]);
        prueba.persona.parar_tras = Some(1);
        assert_eq!(prueba.turno("corta dos veces"), Ok(Fin::Parada));
        assert_eq!(prueba.herramientas.llamadas.len(), 1);
        let resultados = prueba.resultados();
        assert_eq!(resultados.len(), 2, "every call answered");
        assert_eq!(resultados[1]["tipo"], "cancelado");
    }

    #[test]
    fn the_plan_is_kept_shown_and_reminded_inside_a_result_when_stale() {
        let plan = json!({"tareas": [{"texto": "Zoom", "estado": "en_curso"}]});
        let mut prueba = Prueba::new(vec![
            llamadas(&[("plan_escribir", plan)]),
            llama("timeline_read"),
            llamadas(&[("timeline_read", json!({"a": 1}))]),
            llamadas(&[("timeline_read", json!({"a": 2}))]),
            palabras("Listo."),
        ]);
        prueba.turno("zoom").unwrap();
        assert_eq!(
            prueba.conversacion.plan,
            [Tarea {
                texto: "Zoom".into(),
                estado: crate::plan::Estado::EnCurso
            }]
        );
        assert!(prueba.persona.eventos.iter().any(|e| matches!(e, Evento::Plan(_))));
        let notas: Vec<String> = prueba
            .resultados()
            .iter()
            .filter_map(|r| r["nota_komunta"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(notas.len(), 1, "{notas:?}");
        assert!(notas[0].starts_with("[Nota de Komunta") && notas[0].contains("[>] Zoom"));
        // Never as words of the person's.
        let del_usuario = prueba.conversacion.mensajes.iter().filter(|m| !m.texto().is_empty() && m.rol == Rol::Usuario);
        assert_eq!(del_usuario.count(), 1);
    }

    #[test]
    fn a_call_repeated_is_blocked_and_the_turn_goes_on() {
        let mut respuestas: Vec<_> = (0..6).map(|_| llama("clips_split")).collect();
        respuestas.push(palabras("No avanzo."));
        let mut prueba = Prueba::new(respuestas);
        prueba.turno("corta").unwrap();
        assert_eq!(prueba.herramientas.llamadas.len(), 5);
        assert_eq!(prueba.resultados()[5]["tipo"], "bloqueado");
        assert_eq!(prueba.conversacion.respuesta(), "No avanzo.");
    }

    #[test]
    fn the_last_steps_read_then_say_and_a_failed_turn_leaves_no_trace() {
        let mut prueba = Prueba::new(vec![
            llama("clips_split"),
            llamadas(&[("clips_split", json!({"time": 2}))]),
            llama("timeline_read"),
            llamadas(&[("timeline_read", json!({"a": 1}))]),
            palabras("Hecho."),
        ]);
        let limites = Limites {
            pasos: 5,
            tiempo: std::time::Duration::from_secs(300),
        };
        prueba.turno_con("corta", limites).unwrap();
        assert_eq!(prueba.ia.modos[..2], [Modo::Libre, Modo::Libre]);
        assert!(matches!(&prueba.ia.modos[2], Modo::SoloLeer(n) if n.contains(&"timeline_read".to_owned())));
        assert_eq!(prueba.ia.modos[4], Modo::Ninguna);
        let avisos: Vec<Value> = prueba.resultados().into_iter().filter(|r| r.get("nota_komunta").is_some()).collect();
        assert!(avisos[0]["nota_komunta"].as_str().unwrap().contains("Quedan pocos pasos"));

        let mut prueba = Prueba::new(vec![llama("clips_split"), Err("Gemini no respondió".into())]);
        assert_eq!(prueba.turno("corta").unwrap_err(), "Gemini no respondió");
        assert!(prueba.conversacion.mensajes.is_empty());
        assert_eq!(prueba.herramientas.llamadas.len(), 1, "what the tool did is the editor's to undo");
    }
}
