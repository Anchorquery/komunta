# El agente de Komunta

El asistente de la ventana pasa de ser un chat que llama herramientas a ser
un agente: planea, edita, mira el resultado, se corrige y pide permiso antes
de lo que no tiene vuelta atrás. Este documento fija el arnés que lo hace
posible y las herramientas de animación que se le dan. Las ideas vienen del
arnés de agentes de Komunikia (`komunikia-v4/app/lib/ai`, AI SDK) y del
estándar actual (ver «Contraste con el estándar»), llevadas a Rust, a
Gemini y a un editor de escritorio.

## Piezas

```
crates/asistente            el arnés, sin ventana (se prueba sin ella)
  herramienta.rs            Herramienta con metadatos; Resultado uniforme
  bucle.rs                  el turno: pasos, guardias, aprobaciones, parada
  paso.rs                   preparar cada paso: imágenes por lotes,
                            herramientas declaradas y Modo de llamada
  guardia.rs                anti-bucle por firma y presupuesto de pasos/tiempo
  plan.rs                   plan_escribir: la lista de tareas viva
  catalogo.rs               familias de herramientas y herramientas_buscar
  gemini.rs                 proveedor: modos, imagen en la respuesta, firmas,
                            reintento a ritmo de chat
  generico.rs               (feature genai) Claude, GPT, Ollama, Groq... por
                            los adaptadores de genai
  externo/                  (feature acp) el agente de otro - Gemini CLI,
                            Claude Code, Codex - por Agent Client Protocol
crates/animacion            recetas de animación puras: Clip/llaves → Command
crates/concat/src/panes/
  asistente_vivo.rs         el Ejecutor: todas las de concat-mcp + las nuevas
  asistente.rs              el panel: plan, pasos, tarjeta de aprobación, Parar
  asistente_ia.rs           qué IA: la del montaje si va por API y tiene clave
  asistente_externo.rs      qué agente externo y cómo se arranca
crates/concat/src/en_vivo.rs   el modo en vivo del lado de la ventana
crates/concat-api/src/live.rs  el modo en vivo del lado de la API
```

## Frameworks: qué se usa y por qué (noviembre de 2026)

El bucle, las guardias, el catálogo y las aprobaciones son propios: son lo
que hace al agente de un editor (una edición = un Ctrl+Z, ver el fotograma,
preguntar antes de lo que no se deshace en el sitio) y ningún framework
de Rust los da hechos sin imponer su forma. Lo demás va por estándares:

- **Modelos: `genai` 0.7** detrás de `Proveedor`. Más de 35 adaptadores;
  el agente habla con la IA con la que planifica el montaje (`ia.cerebro`:
  anthropic, openai) si tiene su clave, y con Gemini si no. Gemini sigue
  con su adaptador propio: respuestas de función con imagen, `VALIDATED`,
  `allowedFunctionNames`, `thinkingLevel` y la cuota. Como genai solo
  admite texto en un resultado de herramienta, la imagen de `ver_fotograma`
  va en un mensaje de usuario detrás de los resultados.
- **Herramientas: MCP.** `komunta --mcp` ya servía la API a cualquier agente;
  el **modo en vivo** (#337, Ajustes > Automatización) le deja editar el
  proyecto que está en pantalla: la petición que nombra la carpeta abierta
  se hace en la sesión de la ventana, una llamada = un paso de deshacer, el
  fotograma sale del monitor de la ventana. Guardar, cerrar y exportar
  siguen siendo de la persona. Lo que no se deshace en el sitio (cambiar el
  tamaño del vídeo, importar, quitar más de 3 clips) se pregunta en el
  panel, llame quien llame.
- **Agentes: ACP** (`agent-client-protocol` 5.2, protocolo v1, sin tokio).
  El panel puede tener a otro agente en vez del propio (Ajustes >
  Automatización > Agente del asistente): Gemini CLI (`gemini --acp`),
  Claude Code (`npx @agentclientprotocol/claude-agent-acp`) o Codex
  (`npx @agentclientprotocol/codex-acp`), cada uno con su IA y su cuenta. Se le
  da el servidor MCP de Komunta en modo ventana, así que necesita el modo en
  vivo. Sus mensajes, herramientas, plan y peticiones de permiso se ven como
  los del propio; Parar es `session/cancel`; a un permiso se contesta
  «permitir una vez» o «rechazar», nunca «siempre». El proceso y la sesión
  duran lo que la conversación. Gemini CLI recibe la clave de Google de
  Komunta (`GEMINI_API_KEY`) y se le pide `authenticate` con
  `gemini-api-key`: su acceso con cuenta de Google para particulares ya no
  existe. Probado: sesión abierta en ~10 s con Gemini CLI 0.65.
  Lo que cada agente deja elegir de su sesión (modelo, nivel de
  razonamiento, modo) llega como `config_options` de ACP (categorías
  `model`, `thought_level`, `mode`) y se cambia con
  `session/set_config_option`; un agente que aún habla los `modes`
  antiguos (Gemini CLI 0.65: Default, Auto Edit, YOLO, Plan) los ofrece
  como su modo, con `session/set_mode`. Su lista de `models` antigua ya no
  está en el esquema y no se lee. Las opciones salen al abrir la sesión,
  es decir, tras el primer mensaje.
- Descartados: adk-rust (MSRV 1.95, 49 crates, 3.0 a medio romper), rig
  (otro bucle encima del nuestro), A2A (sin SDK oficial en Rust).

## El turno

1. Llega el texto de la persona. Es el único mensaje de usuario con texto
   del turno: Gemini 3 decide dónde empieza el turno buscando el último, y
   una nota del arnés como texto de usuario partiría la tarea en dos.
2. Cada paso empieza en `paso::preparar`, el único punto de control, que
   no toca el prefijo de la petición (lo que Gemini tiene en caché):
   - la lista de herramientas solo crece: núcleo + familias activadas
     (`herramientas_buscar`, o llamar a una del índice la activa);
   - qué se puede llamar lo dice `Modo`: libre (`VALIDATED`), solo lectura
     (`VALIDATED` + `allowedFunctionNames`) o ninguna (`NONE`), sin quitar
     declaraciones;
   - los fotogramas se limpian por lotes: al pasar de 4 quedan 1; los
     demás pasan a una línea dentro de su resultado.
3. El modelo contesta. Sus llamadas pasan, en orden, por la misma puerta:
   herramientas del arnés (plan, búsqueda) → guardias → `comprobar` (ids y
   tiempos contra la línea de tiempo: lo que va a fallar ni se aprueba ni
   se ejecuta) → aprobación de la persona si la clase lo pide → Parar →
   ejecutar.
4. Toda llamada recibe respuesta, se ejecute o no. Forma única:
   `{ok, datos}` o `{ok: false, error, tipo, sugerencia?}` con `tipo` =
   `error | bloqueado | rechazado | cancelado | vacio | parcial`. Las notas
   del arnés (plan rancio, guardias, margen) viajan dentro del último
   resultado como `nota_komunta`, marcadas «[Nota de Komunta, no escrita por
   la persona]». `ver_fotograma` lleva la imagen dentro de su
   `functionResponse` (Gemini 3+); a Gemini 2 va como parte aparte, tras
   todas las respuestas del paso.
5. El turno acaba cuando el modelo responde sin llamadas (o con palabras
   junto a ediciones que salieron bien y nada que leer), cuando la persona
   pulsa Parar (las llamadas pendientes vuelven `cancelado`) o al agotar
   el presupuesto. Todo lo editado en el turno es un solo Ctrl+Z.

## Clases de efecto y aprobación

| Clase | Ejemplos | Aprobación | MCP |
|---|---|---|---|
| `lectura` | `timeline_read`, `ver_fotograma`, `effects_list` | nunca | `readOnlyHint` |
| `edicion` | recortar, mover, keyframes, efectos, títulos | nunca: se deshace con Ctrl+Z | — |
| `destructiva` | quitar varios clips, cambiar fotograma del proyecto | siempre | `destructiveHint` |
| `gasto` | sticker con Nano Banana, B-roll con IA | siempre, con el coste estimado | `openWorldHint` |
| `archivo` | importar medios | siempre | — |

Cada herramienta dice además si es `idempotente` (repetirla es seguro).
La política vive en el arnés, no en el prompt: el prompt solo la explica.

## Guardias

- **Anti-bucle**: la misma llamada con los mismos argumentos 3 veces avisa;
  5 la bloquea el resto del turno (se queda declarada y responde
  `bloqueado`: quitarla rompería la caché). Dos fallos seguidos de una
  herramienta avisan; tres la bloquean.
- **Pasos**: hasta 30 por turno; quedan 3 → solo lectura; queda 1 → sin
  herramientas, para cerrar.
- **Tiempo**: al 70 % de 5 minutos, solo lectura; al 85 %, sin herramientas.
- **Parar**: una bandera que la ventana levanta; se mira antes de cada paso,
  de cada herramienta y tras cada aprobación.

## Proveedor (Gemini)

- Reintento a ritmo de conversación (`Ritmo::CONVERSACION`): un modelo
  saturado se repite tras 1 s y 3 s y luego pasa el siguiente; el límite
  por minuto respeta el `retryDelay` de Google.
- Una respuesta `MALFORMED_FUNCTION_CALL` se pide otra vez, una vez.
- Un 400 que nombra `function_declarations[N]` se repite sin esa
  herramienta: red de seguridad, el esquema hay que arreglarlo.
- Firmas: las partes de Gemini vuelven intactas; la primera llamada de un
  paso sin firma (la hizo un modelo 2.x) lleva
  `skip_thought_signature_validator` al ir a un modelo 3+.
- Sin `temperature`/`top_p`/`top_k` (3.6+ los ignora, los siguientes darán
  400). Pensamiento según el razonamiento elegido en el panel (bajo por
  defecto): `thinkingBudget` 0/128, 2048 u 8192 en 2.x; `thinkingLevel`
  `low`, `medium` o `high` en 3+. Por genai, `ReasoningEffort` igual.
- El registro anota `cachedContentTokenCount` de cada paso: si baja, algo
  cambió el prefijo.

## Contraste con el estándar (noviembre de 2026)

Revisado contra Anthropic (agentes, herramientas, ingeniería de contexto,
edición de contexto), OpenAI Agents SDK (aprobaciones, `max_turns`), Google
(llamadas a funciones, firmas, caché, novedades de 3.6, ADK) y MCP
(anotaciones). Cambió respecto al primer diseño: notas dentro de resultados,
imagen dentro de `functionResponse`, lista de herramientas estable con
`Modo`, limpieza de imágenes por lotes, firmas ficticias, `rechazado` /
`cancelado` / `sugerencia`, respuesta a todas las llamadas al parar,
`VALIDATED`, sin temperatura. Cada turno deja una línea en
`<datos>/asistente/trazas.jsonl` (`asistente::traza`): el pedido, los pasos
con llamadas y resultados recortados, sin imágenes, cómo acabó y cuánto
tardó. Queda pendiente: resumen de la sesión al pasar un umbral de tokens
y tareas doradas del asistente (evals sobre el estado de la línea de
tiempo), que se pueden sacar de las trazas.

## El panel

Como los paneles de agente de Zed, VS Code y Claude: el campo abajo del
todo, una tarjeta que crece con el texto (Intro envía, Mayús+Intro es otra
línea) y debajo de las palabras las opciones del turno siguiente y el
botón de enviar, que es Parar mientras corre un turno. Arriba, en vez del
título, qué agente es. Para el propio: el modelo (Automático = el orden de
siempre; uno elegido va primero y los demás quedan detrás de respaldo) y
el razonamiento (bajo, medio, alto), guardados en `asistente` de los
ajustes. Para uno externo: lo que ofrezca su sesión. Las palabras de la
persona van en una tarjeta a la derecha; las del asistente, en texto
llano a lo ancho; cada herramienta, una fila discreta con su marca.

## Plan visible

`plan_escribir({tareas: [{texto, estado}]})` reemplaza la lista entera. La
ventana la enseña encima del chat. Para encargos de más de dos pasos, el
prompt pide escribir el plan primero.

## Herramientas

**Núcleo, siempre visibles**: `timeline_read`, `ver_fotograma`,
`plan_escribir`, `herramientas_buscar`, `edit_undo`, `edit_redo`.

**Familias, bajo demanda**:

- `clips`: `clip_add`, `clips_add_sequence`, `clip_trim`, `clips_split`,
  `clips_move`, `clips_remove`, `clip_update`, `clip_speed`, `track_add`.
- `encuadre`: `clip_transform`, `clip_frame`, `clip_mask`,
  `clip_stabilize`, `timeline_background`.
- `texto`: `title_add`, `captions_add`, `titulo_animado`.
- `animacion`: `keyframe_set`, `keyframe_clear`, `zoom_en`,
  `texto_detras`, `seguir_cara`.
- `efectos`: `effects_list`, `effect_layer_add`, `effect_key_set`.
- `medios`: `media_import`, `media_probe`, `foto_parallax`,
  `sticker_poner`, `sticker_generar`.
- `proyecto`: `project_set_video`, `edit_apply` (comando crudo).
- `guion` (fase F3 de `asistente-ia.md`): `guion_leer`, `guion_palabras`,
  `guion_toma_quitar`, `guion_toma_mover`, `guion_subtitulo_cambiar`,
  `guion_subtitulo_quitar`, `guion_recurso_quitar`, `guion_descartar`,
  `guion_versiones`, `guion_volver`, `guion_aplicar`.
- `montaje` (fase F4): `montaje_estado`, `montaje_opciones` (recetas y kits
  de marca), `montaje_lanzar` (gasto: rehace el montaje o lo hace con un
  material), `montaje_cancelar`.

Las herramientas del guion trabajan sobre el plan que tiene el panel Guion
en memoria: lo editado se ve en el panel al momento, no entra en el Ctrl+Z
del turno y `guion_descartar` lo tira. `guion_volver` y `guion_aplicar`
piden permiso. Aplicar guarda el proyecto y lo abre otra vez, así que se
hace al acabar el turno; la conversación sigue tras volver a abrirlo.
`montaje_lanzar` igual: rellena la hoja del panel Montaje como la dejó el
último montaje, cambia lo que se le pide, comprueba material y modelos, y
el montaje arranca al acabar el turno.

`foto_parallax` y los stickers son de la fase 2 y aún no están;
`seguir_cara` ya sí (YuNet sobre el fotograma compuesto, cada 0,2 s). Abrir, guardar o crear proyectos, las plantillas (hacen proyectos
nuevos) y exportar no se dan al agente: son de la persona.

## Recetas de animación (`crates/animacion`)

Funciones puras que reciben la línea de tiempo y devuelven comandos del
editor, usadas por el agente y por el plan del montaje:

- **`zoom_en`**: punch-in de 1,0 a 1,12–1,25 en una frase, centrado en la
  cara si se conoce, con salida suave o corte seco.
- **`texto_detras`**: un duplicado del clip de vídeo con el recorte de
  persona, en una pista por encima del título.
- **`titulo_animado`**: estilos `aparecer`, `subir`, `escala`, `palabras`
  (revelado por palabra con el efecto `revelar-palabras`), `maquina`.
- **`seguir_cara`**: llaves de posición del título siguiendo la cara
  detectada (YuNet) a 5 por segundo, suavizadas.
- **`foto_parallax`**: la profundidad de una foto (Depth Anything V2
  Small, ONNX) parte la imagen en planos con alfa; cada plano se anima
  con llaves de escala y posición distintas.
- **Stickers**: biblioteca local (`<datos>/stickers`), con entrada «pop».
  `sticker_generar` crea uno nuevo con Nano Banana sobre verde y lo recorta.

En el montaje automático, el plan puede pedir `zooms`, subtítulos `clave`
con `detras: true` y `stickers` por palabra; el prompt dice cuándo.
