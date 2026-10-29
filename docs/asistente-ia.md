# Asistente de IA en Komunta: evaluación y propuesta

Estado: evaluación, sin código. Revisado el 29-10-2026 (varios proveedores; el montaje es parte del proyecto).
Alcance: un asistente dentro de Komunta que ayude en todo el flujo (montar con IA un proyecto, editar el «Guion de montaje», corregir hablando, recetas y kits de marca, y edición normal en la línea de tiempo), en castellano y en euskera, y que funcione con **varios proveedores de IA**, como el motor Python.

---

## 1. Recomendación en corto

**Opción (d), híbrida: un chat propio que habla con varios proveedores, con las mismas herramientas para todos.**

1. **Primero, el «modo en vivo»**: que las herramientas actúen sobre el proyecto abierto en la ventana (hoy no pueden). Upstream lo aprobó como *opt-in* en jub0t/Concat#337 y sigue sin asignar; lo hacemos nosotros con el diseño que fijó el mantenedor. En esa capa van también **las confirmaciones**, para que valgan igual sea cual sea el proveedor o el cliente.
2. **Un chat lateral dentro de Komunta** con un bucle de herramientas propio en Rust y **adaptadores por proveedor**, los mismos siete modos que ya tiene el motor (`ia.modo`) y que la app ya ofrece en el panel Montaje (`CEREBROS` en `concat/src/panes/montaje.rs`):

   | Modo | Cómo funciona el asistente |
   | --- | --- |
   | `anthropic` | API de Anthropic con *tool use*, bucle en la app |
   | `openai` | API de OpenAI (Responses) con *function calling*, bucle en la app |
   | `google` | API de Gemini con *function calling*, bucle en la app (sirve con la clave gratuita, con sus límites) |
   | `claude_code` | Claude Code CLI con la suscripción, conectado al MCP de Komunta |
   | `codex` | Codex CLI con la sesión de ChatGPT, conectado al MCP de Komunta |
   | `gemini_cli` | Gemini CLI con la cuenta de Google, conectado al MCP de Komunta |
   | `manual` | Ningún chat en la app: un agente externo (Claude Desktop, etc.) usa `komunta --mcp window` |

3. **Una sola preferencia de proveedor** para el montaje y el asistente (con la opción de usar otro para el chat), **cadena de respaldo** como la de `cuota.py` (si uno se queda sin cuota o está saturado, sigue el siguiente) y claves de los tres proveedores en el almacén de credenciales del sistema.
4. **El montaje no es un flujo aparte**: el asistente monta con IA **el proyecto abierto** (lo mismo que «Montar con IA» y «Rehacer…» del panel Montaje) o crea un proyecto nuevo con la sección «Con IA»; el resultado entra como líneas de tiempo nuevas por `guion::graft`.

Proveedor recomendado por defecto: **Anthropic** (Sonnet 5.5 para el chat, Opus 5.5 para el plan), con **Gemini gratuito** como respaldo barato y **Claude Code** para quien prefiera la suscripción. Pero ningún proveedor es obligatorio, y el juego de pruebas de la fase F6 decide con datos cuál va mejor en cada tarea y en euskera.

Esfuerzo: **≈10-13 semanas** de una persona hasta tener todo el flujo con los siete modos; algo útil (chat con Anthropic que edita la línea de tiempo) en ≈3-4.

---

## 2. Qué hay ya en el repo (y qué falta)

### 2.1 Piezas que se reutilizan

| Pieza | Dónde | Qué aporta al asistente |
| --- | --- | --- |
| Catálogo de herramientas MCP | `src/crates/concat-mcp/src/tools.rs` (`catalogue()`) | 42 herramientas con nombre, descripción y marcas `read_only` / `destructive` / `idempotent`. Las marcas sirven para decidir qué se confirma. |
| Esquemas generados | `src/crates/concat-mcp/src/schema.rs` | Esquemas JSON de entrada sacados de los tipos de `concat-api`; con un pequeño adaptador por proveedor son las declaraciones de herramientas de Anthropic, OpenAI y Gemini. |
| `Backend` | `src/crates/concat-mcp/src/backend.rs` | Rasgo `call(Request) -> Value` con `Window` y `Headless`; su comentario ya prevé un tercero para el modo en vivo (#337). |
| API única | `src/crates/concat-api` (`Request`: `project.*`, `edit.apply`, `edit.undo/redo`, `media.*`, `catalogue.list`, `template.*`, `export.*`, `preview.frame`) | Toda edición es `edit.apply` de un `concat_project::Command`. |
| Comandos | `src/crates/concat-project/src/commands/mod.rs` | 54 variantes de `Command`; `Batch` agrupa varias en un paso de deshacer. |
| Gestos | `Editor::apply_within(gesture, command)` (`concat-project/src/editor.rs`) | Varias órdenes con el mismo gesto se pliegan en un solo paso de deshacer y cada una devuelve su `created_id`: «un turno del asistente = un Ctrl+Z». |
| Ventana | `Studio::apply`, `undo`, `redo`, `after_change` (`concat/src/studio.rs`) | Entrada de las ediciones en la ventana: refresca vista, autoguardado, audio y vista previa. |
| Ver el vídeo | `preview_frame` → `preview.frame` | PNG del fotograma compuesto real. |
| Hilos | `concat/src/host.rs` (`spawn`, `spawn_in_project`, `spawn_detached`) | Llamadas HTTP y procesos hijo fuera del hilo de la interfaz. |
| Red y carpetas | `concat-host` (`ureq = "2"`, `AppDirs`) | Cliente HTTP ya enlazado; `%APPDATA%\Komunta` y `%LOCALAPPDATA%\Komunta`. |
| Voz local | `src/crates/concat-speech` (whisper.cpp) | Dictado al chat sin enviar audio fuera. |
| Montaje dentro del proyecto | `concat/src/panes/montaje.rs` (panel Montaje: «Montar con IA», fases y progreso, «Rehacer…», selector `CEREBROS`), sección «Con IA» de Nuevo proyecto, tipo «Automático» (la IA elige receta) | El asistente lanza exactamente esto, no un flujo propio. |
| Motor | `src/crates/montaje/src/motor.rs` (`python -m motor analizar`, avance en líneas `[fase  45%] mensaje`) | Análisis, transcripción y plan siguen en Python por ahora; el asistente lo usa a través de esta misma pieza. |
| Guion | `src/crates/montaje/src/guion.rs` (`Guion`, `PlanCrudo` con `alternar_palabra`, `quitar_toma`, `mover_toma`, `cambiar_subtitulo`, `quitar_subtitulo`, `quitar_recurso`, `guardar(trabajo, nota)` con versiones en `ia/versiones`) y `concat/src/panes/guion.rs` (`PaneKind.guion`, `graft`) | La mitad de las herramientas del guion ya existen como métodos; solo hay que exponerlas. |
| Recetas y kits | `src/crates/montaje/src/receta.rs` (`cargar`, `guardar`, `de_serie`, `automatica`), `kit.rs` (`Kit::cargar`) | Formatos ya en Rust: las herramientas los usan en vez de escribir archivos a mano. Para editar kits falta un `Kit::guardar`. |
| Paneles | `ui/workspace/pane-switcher.slint` (`PaneKind`: media, preview, inspector, timeline, keyframes, guion) y `seat.slint` | Añadir el chat es una variante más. |

### 2.2 Qué falta

- **Modo en vivo.** El servidor de la ventana (`concat-server`, `Hub`) tiene su propia `Api` y no ve la sesión de `Studio`; `docs/MCP.md` lo dice («el agente edita proyectos propios»).
- **Estado de la interfaz** en la API (selección, cabezal, línea de tiempo activa, idioma, si el proyecto tiene montaje).
- **Herramientas** del guion, del montaje, de recetas y de kits en el catálogo.
- **Clientes de OpenAI y Gemini en Rust** (hoy solo existen en el motor Python) y el de Anthropic.
- **Guardado de claves** en la app (el motor las tiene en su `.env`).

### 2.3 Upstream (jub0t/Concat)

- **#337 (abierto)**: el mantenedor aceptó el modo en vivo el 18-10-2026:
  - interruptor *opt-in* en Ajustes > Automatización, apagado por defecto;
  - con él encendido, la petición que nombra el proyecto abierto se ejecuta en el bucle de eventos de la ventana, sobre su sesión, con el mismo `Command`, ediciones visibles al momento, lecturas desde el estado de la ventana y exportaciones por la ranura compartida;
  - un paso de deshacer por llamada a la API.
- El MCP (`concat --mcp`) está desde 0.2.8. El modo en vivo está **sin asignar**.
- **No hay agente ni chat** en upstream. El modo en vivo se puede devolver a upstream (mismo diseño, AGPL) para reducir el coste de mantener el fork; el chat y los adaptadores son solo nuestros.

### 2.4 Motor Python de referencia (`automatizacion_videos/motor`)

- `cerebros/__init__.py`: siete modos intercambiables (`manual`, `claude_code`, `codex`, `gemini_cli`, `anthropic`, `openai`, `google`) con la misma validación del plan y una ronda de corrección.
- `cerebros/clis.py`: `claude -p` (solo lectura, `--resume`), `codex exec` (*sandbox* de solo lectura, imágenes con `-i`), `gemini -p` (modo plan, adjuntos con `@ruta`). Quitan del entorno la clave del proveedor para forzar la suscripción.
- `cerebros/apis.py`: Anthropic (por defecto `claude-opus-5-5`, *streaming*, caché, respaldo si el modelo rechaza), OpenAI (`gpt-5.5`, Responses) y Google (`google-genai`, con `cuota.con_reintentos`).
- `cuota.py`: cuenta llamadas por modelo y día, aprende el límite del mensaje de error, marca agotado hasta la renovación y pasa al siguiente modelo de la lista.
- `edicion.py`: «corregir hablando» con el **mapa del vídeo** en segundos reales; versiones en `ia/versiones/`.
- `ajustes.py`: claves en `.env` (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`), validadas con una llamada barata, nunca devueltas enteras; catálogo de modelos recomendados con coste orientativo.

Se aprovecha casi todo: los siete modos, la cadena de respaldo y la cuota, el mapa del vídeo, el versionado y la validación de claves. Lo que cambia: en vez de «devuelve el plan entero en JSON», el asistente llama a **operaciones pequeñas** que se validan una a una y se deshacen sueltas.

---

## 3. Opciones comparadas

| | (a) Chat propio con API (un proveedor) | (b) Solo CLIs con suscripción + MCP | (c) Solo agente externo vía MCP | (d) Híbrido multiproveedor (recomendado) |
| --- | --- | --- | --- | --- |
| Experiencia | Panel en la app, tarjetas, «Deshacer» por respuesta, chips | Panel en la app que muestra lo que dice la CLI; confirmaciones vía MCP | Otra ventana; el equipo lo tiene que configurar | La de (a) con cualquier proveedor; (b) y (c) como modos |
| Proveedores | Uno | Claude Code, Codex, Gemini CLI | Cualquier cliente MCP | Los siete modos del motor |
| Coste | Por uso | Incluido en cada suscripción, con sus límites | Incluido en la suscripción de quien lo use | El del modo elegido; respaldo gratuito con Gemini |
| Instalación | Una clave | Node y la CLI con sesión iniciada en cada equipo | Cliente MCP a mano | Nada por defecto |
| Control | Total | Parcial: depende de las opciones de cada CLI | Ninguno sobre la conversación | Total en los modos API; las confirmaciones, en todos (van en la capa en vivo) |
| Riesgo de dependencia | Alto (un proveedor) | Formatos de salida de tres CLIs que cambian | Bajo | Bajo: si un proveedor falla o encarece, se cambia en Ajustes |
| Esfuerzo | Alto | Medio | Bajo | El más alto, pero cada adaptador es pequeño una vez hecho el núcleo |

Notas sobre las CLIs con suscripción:
- Las tres admiten servidores MCP por stdio: Claude Code con `--mcp-config` y `--allowedTools "mcp__komunta__*"`; Codex con `[mcp_servers]` en su `config.toml` (o `-c` en la línea de órdenes); Gemini CLI con `mcpServers` en su `settings.json`. La app genera esa configuración apuntando a `komunta --mcp window` y la pasa al lanzar el proceso, sin tocar la configuración global del usuario.
- El consumo cuenta contra los límites de cada suscripción; un montaje largo se puede cortar a medias.
- Anthropic no permite que productos de terceros ofrezcan el inicio de sesión de claude.ai a través del Agent SDK. Lanzar la CLI propia del usuario en su equipo es lo que ya hace el motor; aun así, **conviene confirmar las condiciones vigentes de cada plan** (Claude, ChatGPT, Google) antes de recomendarlo al equipo.

---

## 4. Arquitectura propuesta

```
┌────────────────────────────── Komunta (ventana) ──────────────────────────────┐
│ Panel «Asistente»  ◄── modelos Slint ──►  panes/asistente.rs                    │
│      │                                                                          │
│      ▼                                                                          │
│ crate `asistente`                                                               │
│  · historial neutro (texto, imagen, llamada, resultado)                         │
│  · bucle de herramientas · contexto (estado, mapa) · uso y tope de gasto        │
│  · cadena de respaldo + cuota (port de cuota.py)                                │
│      │                                                                          │
│      ├─► Proveedor API ── Anthropic │ OpenAI │ Google  (HTTP con ureq)          │
│      │        └─ herramientas: catálogo común ─┐                                │
│      └─► Proveedor CLI ── claude │ codex │ gemini (proceso hijo)                │
│               └─ MCP: komunta --mcp window ────┤                                │
│                                                ▼                                │
│                            Herramientas (mismo catálogo para todos)             │
│                             ├─ Edición: concat-mcp → Backend «Vivo» → Studio    │
│                             ├─ Montaje y guion: montaje::{motor, guion}, graft  │
│                             ├─ Recetas y kits: montaje::{receta, kit}           │
│                             └─ Interfaz: cabezal, selección, ir a…              │
│                            Política de confirmación (aquí, para todos)          │
└─────────────────────────────────────────────────────────────────────────────────┘
          ▲
          └── modo `manual`: Claude Desktop u otro cliente MCP externo
```

### 4.1 Modo en vivo y confirmaciones (requisito previo)

- Una tercera implementación de `Backend`, **`Vivo`**: lleva cada `Request` al bucle de eventos de la ventana (`slint::invoke_from_event_loop`) y la ejecuta contra la sesión de `Studio`; la respuesta vuelve por un canal. Lecturas desde el estado de la ventana.
- El servidor de la ventana usa ese `Vivo` cuando la petición nombra el proyecto abierto y el interruptor está encendido (diseño de #337). Así `komunta --mcp window` edita en vivo, y con él las tres CLIs y el modo `manual`.
- El chat con proveedores API llama a `Vivo` directamente, en el mismo proceso.
- **Un turno del asistente = un paso de deshacer**: `Studio::apply_within("asistente:<turno>", …)`. Para las CLIs y los clientes externos, el turno se cierra cuando el cliente pasa un rato sin llamar a herramientas o cuando el usuario escribe.
- **Las confirmaciones viven aquí, no en el proveedor**: cuando una herramienta necesita permiso (ver 4.6), `Vivo` deja la llamada en espera, muestra la tarjeta en el panel y responde «aplicado» o «el usuario lo ha rechazado». Funciona igual con Anthropic, con Codex o con Claude Desktop, sin depender de los permisos de cada CLI.

### 4.2 Crate `asistente` y adaptadores

**Núcleo común**
- Historial **neutro**: mensajes con partes de texto, imagen, llamada a herramienta y resultado. Cada adaptador lo traduce al formato de su proveedor y de vuelta. Esto permite cambiar de proveedor a mitad de conversación (respaldo) sin perder el hilo: se descarta lo que es propio de un proveedor (por ejemplo, los bloques de pensamiento de Claude).
- Catálogo de herramientas común (nombre, descripción, esquema JSON), sacado de `concat-mcp` más las nuevas.
- Bucle: pedir → ejecutar las llamadas (en paralelo las de lectura) → devolver todos los resultados juntos → seguir hasta que no haya llamadas. Los errores de herramienta vuelven como resultado de error (el texto de `CommandError` sirve al modelo para corregirse).
- Contexto en cada turno, pequeño y textual: estado de la interfaz, resumen de la línea de tiempo (`concat-mcp/src/summary.rs`), y si el proyecto tiene montaje, el mapa del vídeo al estilo de `edicion.py::mapa_video`. Transcripciones bajo demanda.
- *Streaming* en un hilo de `host::spawn`; botón «Parar».

**Adaptadores API** (Rust no tiene SDK oficial de ninguno de los tres; HTTP con `ureq`, que ya está):

| | Anthropic | OpenAI | Google |
| --- | --- | --- | --- |
| Llamada | `POST /v1/messages`, `tools` con `input_schema` | Responses, herramientas `function` | `generateContent`, `functionDeclarations` |
| Esquemas | El JSON Schema tal cual; `strict: true` | En modo estricto exige todas las propiedades en `required` y `additionalProperties: false`: se transforma o se usa sin modo estricto | Admite un subconjunto de OpenAPI: el adaptador limpia las palabras clave que no acepta |
| Imágenes de herramientas | Dentro del resultado de la herramienta | Si el resultado no admite imagen, se añade como entrada del usuario justo después | Igual: parte de imagen en el turno siguiente |
| Particularidades | Opus/Sonnet 5.5: `tool_choice` forzado da error (usar `auto`); el pensamiento no se apaga en Opus 5.5, se regula con `effort`; historial solo de añadir; atender `stop_reason: "refusal"` y activar el respaldo de servidor; caché de prompt | El que tenga la cuenta; reutilizar la caché automática del proveedor manteniendo el prefijo estable | Clave gratuita con cuotas diarias bajas: la cuota (4.2, más abajo) es imprescindible |

Los detalles concretos de cada API (campos exactos, límites de imágenes, caché) se comprueban contra su documentación vigente al implementar cada adaptador; la tabla solo marca dónde difieren.

**Adaptadores CLI**
- `claude_code`: `claude -p --input-format stream-json --output-format stream-json --mcp-config <archivo> --allowedTools "mcp__komunta__*"`, sesión viva durante la conversación y `--resume` si se reinicia.
- `codex`: `codex exec` con `[mcp_servers.komunta]` pasado por `-c`, y una ejecución por mensaje que retoma la sesión, o el modo interactivo que tenga la versión instalada.
- `gemini_cli`: `gemini` con `mcpServers` en un `settings.json` propio de la app.
- Las tres: el texto que escriben va al chat; las herramientas las ejecutan por MCP contra `Vivo` (con sus confirmaciones); el proceso se mata con «Parar». Igual que el motor, se quita del entorno la clave de API del proveedor para que use la suscripción.
- Con `manual` no se lanza nada: el panel muestra cómo conectar un cliente externo y, mientras ese cliente edita, enseña lo que va haciendo (las mismas tarjetas de cambios).

**Respaldo y cuota**
- Cadena configurable, por ejemplo `anthropic → google → openai`, como `ia.google.respaldo` del motor pero entre proveedores.
- Port de `cuota.py` a Rust: llamadas por modelo y día, límite aprendido del error, «agotado hasta» la renovación; guardado en `AppDirs.data`. Con 429 o saturación se pasa al siguiente y se avisa en el chat («Gemini se ha quedado sin cuota hasta mañana; sigo con Claude»).
- Las CLIs no entran en la cadena automática (no se sabe su cuota); si una falla, se avisa y se ofrece cambiar.

**Un mismo proveedor para montaje y chat**
- El panel Montaje ya elige `cerebro` (`studio.prefs.montaje.cerebro`). El asistente usa esa misma preferencia por defecto, con la opción «Otro proveedor para el chat» (por ejemplo, plan con Opus y chat con Gemini gratuito).

### 4.3 Panel de chat en la interfaz

- Nueva variante `PaneKind.asistente` en `ui/workspace/pane-switcher.slint` (etiqueta, `id` e icono) y rama `if root.kind == PaneKind.asistente: AsistentePane { … }` en `ui/workspace/seat.slint`; componente en `ui/workspace/asistente-pane.slint`; lógica en `concat/src/panes/asistente.rs`, como `guion.rs` y `montaje.rs`.
- Elementos: mensajes con *streaming*; selector de proveedor y modelo visible (con el respaldo activo); tarjetas de cambio propuesto («Aplicar» / «Descartar»); «Deshacer este cambio» en cada respuesta que editó; chips que llevan a una toma, subtítulo o clip; micrófono (dictado local); coste de la sesión cuando el proveedor lo informa.
- Textos en `locales/es.json`, `eu.json` y `en.json`.

### 4.4 Cómo ve el vídeo

Por orden de coste, igual para todos los proveedores:
1. **Texto**: transcripción con índices de palabra, mapa del vídeo, resumen de la línea de tiempo, `analisis.json` (encuadres, BPM, voz/música).
2. **`preview_frame`** cuando la duda es visual, a **768 px de lado** por defecto.
3. **Hojas de contacto** de un clip o tramo, como las del motor.

El vídeo entero nunca se envía. Si el modelo configurado no acepta imágenes, las herramientas de ver lo dicen y el asistente trabaja solo con texto.

### 4.5 Deshacer

- **Línea de tiempo**: un turno = un Ctrl+Z, compartido con la historia de la ventana. «Deshacer este cambio» solo si ese paso sigue siendo el último.
- **Guion**: cada cambio pasa por `PlanCrudo::guardar(trabajo, nota)`, que ya guarda la versión anterior en `ia/versiones`; «volver a la versión N» es una herramienta y un botón.
- **Aplicar el guion y montar**: el resultado entra como **líneas de tiempo nuevas** (`guion::graft`), así que nunca se pierde la anterior.
- Historial del chat solo de añadir: si el usuario deshace a mano, se añade una nota («el usuario ha deshecho el último cambio») en vez de reescribir mensajes.

### 4.6 Confirmar cambios

Política aplicada en `Vivo` (válida para todos los modos):

| Tipo | Ejemplos | Comportamiento |
| --- | --- | --- |
| Lectura | `timeline_read`, `preview_frame`, `guion_leer` | Sin preguntar |
| Interfaz | mover cabezal, seleccionar | Sin preguntar |
| Edición reversible | recortar, mover, subtítulos, volumen | Se aplica con «Deshacer» («Aplicar y avisar»), o tarjeta previa si se elige «Preguntar antes» |
| Guion | tachar palabras, mover tomas, cambiar recursos | Se ve en el guion como cambio pendiente hasta «Aplicar al vídeo» |
| Coste o tiempo | `montar` / `rehacer` (análisis y plan con IA) | Siempre con confirmación, diciendo proveedor y coste estimado |
| Destructiva o externa | borrar muchos clips, borrar línea de tiempo, `project_save`, exportar sobre un archivo existente, borrar recetas o kits | Siempre con confirmación |

Más de N clips (p. ej. 20) en un turno: confirmación aunque el modo sea «Aplicar y avisar».

### 4.7 Castellano y euskera

- Responder en el idioma de la interfaz o en el que escriba la persona.
- Prompt de sistema con el glosario del fork (denbora-lerroa, klipa, azpitituluak, esportatu…; nota `euskera-revision.md` de la sesión que tradujo la interfaz), común a todos los proveedores.
- La calidad del euskera **varía entre proveedores y modelos**: el juego de pruebas (F6) lleva la mitad de los pedidos en euskera y se pasa con cada proveedor.
- Dictado: whisper local (`concat-speech`). El motor usa para euskera `xezpeleta/whisper-large-v3-eu-ct2` (CTranslate2); para whisper.cpp hace falta una versión GGML o convertirla.

---

## 5. Herramientas del asistente

Mismo catálogo para todos los modos. ★ = ya existe en `concat-mcp`; ◆ = ya existe como función en Rust y solo hay que exponerla.

### 5.1 Contexto e interfaz
- `estado`: proyecto, línea de tiempo activa, cabezal, selección, idioma; si el proyecto tiene montaje, su receta, material y petición (de `montaje.json`).
- `timeline_read` ★, `project_get` ★, `concat_version` ★, `media_probe` ★, `media_list`.
- `transcripcion_leer(clip, desde, hasta)`: palabras con índice y tiempo.
- `preview_frame` ★ (768 px por defecto), `hoja_contacto(clip | tramo)`.
- `ir_a(segundo | toma | subtítulo | clip)`, `seleccionar(clips)`, `abrir_panel(guion | montaje | …)`.
- `effects_list` ★, `template_list` ★.

### 5.2 Edición en la línea de tiempo
- `clip_add` ★, `clips_add_sequence` ★, `clip_trim` ★, `clips_split` ★, `clips_move` ★, `clips_remove` ★, `clip_update` ★, `clip_transform` ★, `clip_speed` ★, `clip_frame` ★, `track_add` ★.
- `title_add` ★, `captions_add` ★, `subtitulo_editar(clip, texto, estilo)`, `subtitulos_desde_voz(clip)` (transcripción local → `captions_add`).
- `keyframe_set` ★, `keyframe_clear` ★, `effect_layer_add` ★, `effect_key_set` ★, `clip_mask` ★, `timeline_background` ★.
- `musica_poner(archivo | biblioteca, volumen, desde)`, `volumen_bajo_voz(clip, nivel)`.
- `edit_apply` ★, `edit_undo` ★, `edit_redo` ★.
- `export_start` ★, `export_status` ★, `export_wait` ★, `export_cancel` ★, `project_save` ★.

### 5.3 Montaje con IA (siempre sobre un proyecto)
No hay «crear montaje» aparte: el montaje es una capacidad del proyecto.
- `proyecto_nuevo(nombre, carpeta, con_ia?)`: con `con_ia` hace lo mismo que la sección «Con IA» de Nuevo proyecto (material, tipo, «¿Qué quieres?», subtítulos, guion); el proyecto se abre y el montaje corre dentro.
- `montar(tipo, que_quieres, material[], subtitulos?, guion?)`: «Montar con IA» del panel Montaje sobre el proyecto abierto. `tipo` por defecto «Automático» (la IA elige receta).
- `rehacer(cambios)`: «Rehacer…» con los pasos de la última vez y lo que se cambie.
- `montaje_estado` y `montaje_cancelar`: fases y progreso (`Avance` de `montaje::motor`).
- `que_quieres_redactar(notas)`: propone el texto del «¿Qué quieres?» para que la persona lo edite; no lanza nada.
- `recetas_listar`, `receta_leer` (para explicar o recomendar el tipo).

El análisis y el plan los hace el motor con el `cerebro` configurado, así que el montaje también es multiproveedor.

### 5.4 Guion de montaje (sobre `montaje::guion::PlanCrudo`)
- `guion_leer` y `guion_mapa` (segundos reales).
- `palabra_alternar(toma, i)` ◆ y `palabras_quitar(toma, desde, hasta)` / `palabras_restaurar(...)` (varias `alternar_palabra`).
- `toma_quitar(toma)` ◆, `toma_mover(toma, hacia)` ◆, `toma_restaurar(toma)`.
- `subtitulo_cambiar(k, texto)` ◆, `subtitulo_quitar(k)` ◆, `subtitulo_estilo(k, estilo, posición)`, `subtitulo_añadir(toma, desde, hasta, texto)`.
- `recurso_quitar(k)` ◆, `recurso_cambiar(k, clip, dur, desde_seg)`, `recurso_añadir(clip, ancla)`, `garabato_poner(k, tipo, posición)`.
- `musica_elegir(clip, volumen, desde_seg)`.
- `guion_versiones`, `guion_volver(versión)`.
- `guion_aplicar`: «Aplicar al vídeo» → líneas de tiempo nuevas por `guion::graft`.

Cada operación termina en `PlanCrudo::guardar(trabajo, nota)` ◆ con una nota corta («quitar la muletilla del principio») que sirve de historial y de versión.

### 5.5 Recetas y kits de marca
- `recetas_listar` / `receta_leer` sobre `receta::cargar` y `de_serie` ◆; `receta_crear`, `receta_editar(id, cambios)`, `receta_duplicar` sobre `receta::guardar` ◆ (el tipo `Receta` hace de validación); `receta_borrar` (confirmación).
- `kits_listar`, `kit_leer` sobre `Kit::cargar` ◆; `kit_editar(colores, fuentes, logo, música, cierre)` y `kit_vista_previa` necesitan antes un `Kit::guardar` que hoy no existe.

Total ≈ 60 herramientas. Los modelos pequeños o gratuitos pueden elegir peor con tantas; plan B: cargar solo los grupos del panel activo.

---

## 6. Proveedores, modelos y coste

### 6.1 Anthropic (precios consultados el 28-10-2026, por millón de *tokens*)

| Modelo | Entrada | Salida | Lectura de caché | Contexto | Uso propuesto |
| --- | --- | --- | --- | --- | --- |
| Claude Opus 5.5 (`claude-opus-5-5`) | 4 $ | 20 $ | 0,20 $ | 1M | Plan de un montaje y replanteos grandes |
| Claude Sonnet 5.5 (`claude-sonnet-5-5`) | 2 $ | 10 $ | 0,20 $ | 1M | Chat por defecto |
| Claude Haiku 5.5 (`claude-haiku-5-5`) | 0,10 $ | 0,50 $ | — | 1M (más caro por encima de 100K) | Tareas internas: título de la conversación, resumir historial |

Mensaje típico con Sonnet 5.5 (≈12K *tokens* en caché, ≈8K nuevos, ≈1,5K de salida): ≈**0,035 $**; sesión de 30 mensajes ≈**1 $**; Opus 5.5 ≈ el doble. Plan nuevo con Opus 5.5 ≈ 1 $ (estimación de `ajustes.py`).

### 6.2 OpenAI y Google

- **OpenAI**: el motor usa `gpt-5.5`; coste según la tarifa vigente de OpenAI.
- **Google**: `gemini-flash-latest` gratis dentro de los límites del plan gratuito (cuotas diarias bajas: un día de mucho trabajo las agota); `gemini-pro-latest` más capaz y de pago por encima de esos límites.
- No se dan cifras aquí para no quedar desfasadas: el panel muestra el coste que informe cada API (uso de *tokens* × tarifa configurada en Ajustes, como el catálogo `RECOMENDADOS` de `ajustes.py`).

### 6.3 CLIs con suscripción

Sin coste por mensaje; consumen los límites de cada suscripción (Claude, ChatGPT o Google).

### 6.4 Elección por tarea

| Tarea | Por defecto | Alternativas |
| --- | --- | --- |
| Plan del montaje | Opus 5.5 (o el `cerebro` del panel Montaje) | `claude_code`, `gpt-5.5`, Gemini |
| Chat y edición | Sonnet 5.5 | Gemini flash (gratis), `gpt-5.5`, cualquier CLI |
| Tareas internas | Haiku 5.5 | Gemini flash |

La tabla es el punto de partida; el juego de pruebas (F6) la confirma o la cambia.

Control de gasto: tope mensual por proveedor (aviso al 80 %, corte al 100 %), contador por sesión y límite también en la consola de cada proveedor.

---

## 7. Privacidad (material de clientes)

- **Qué sale del equipo**: texto (transcripciones, plan, resumen de la línea de tiempo, nombres de archivo) y, cuando hace falta, fotogramas sueltos u hojas de contacto. Nunca el vídeo ni el audio (dictado y transcripción locales).
- **Cada proveedor tiene sus condiciones**, y cambian según el tipo de cuenta:
  - APIs de pago (Anthropic, OpenAI, Google de pago): condiciones comerciales; por norma no entrenan con los datos de la API y tienen retención limitada.
  - **Gemini gratuito**: Google puede usar los datos de su plan gratuito para mejorar sus productos. **No usarlo con material de clientes** salvo que el contrato lo permita.
  - Suscripciones (CLIs): depende del plan (empresa o personal) y de los ajustes de cada cuenta.
  - Comprobar las condiciones vigentes de cada uno antes de activarlo para el equipo.
- **En la app**:
  - cada proveedor con una etiqueta en Ajustes («apto para material de clientes» sí/no), que decide el equipo;
  - opción por proyecto «Proyecto confidencial»: solo proveedores marcados como aptos, o ninguno; la cadena de respaldo **nunca** salta a un proveedor no apto;
  - interruptor «Permitir que el asistente vea fotogramas»;
  - aviso la primera vez, explicando qué se envía y a quién;
  - registro local de cada petición (proveedor, herramientas, imágenes, coste) en la carpeta de datos.
- Las claves nunca se escriben en el proyecto ni en registros, ni se devuelven enteras a la interfaz.

---

## 8. Sin conexión

Ningún modo da IA sin red. Sin conexión, el panel lo dice y guarda el mensaje para enviarlo luego; el guion, la línea de tiempo, la transcripción y los subtítulos automáticos (locales) funcionan igual.

Un modelo local (por ejemplo, vía Ollama) cabe después como un adaptador más del mismo rasgo, pero hoy los modelos locales pequeños no sostienen bien el *tool use* largo ni el euskera. No se recomienda invertir en ello ahora.

---

## 9. Dónde guardar las claves

Mismas variables que el motor: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`.

Orden de búsqueda:
1. Variable de entorno (pruebas y CI).
2. **Almacén de credenciales del sistema** (Administrador de credenciales de Windows; Llavero en macOS; Secret Service en Linux) con el crate `keyring`, una entrada por proveedor. Recomendado.
3. Si no hay almacén: archivo en `%APPDATA%\Komunta\` con permisos solo para el usuario, como `automation.json`.

En Ajustes > Asistente: por proveedor, pegar clave, «Probar» (llamada barata a la lista de modelos, como `probar_y_listar`), últimos 4 caracteres, «Borrar»; botón **«Importar del motor»** que lee el `.env` de la carpeta del motor ya configurada en Ajustes. Recomendado: una clave por equipo y proveedor, con límite de gasto.

Las CLIs no usan clave: usan la sesión iniciada de cada una (`claude auth login`, `codex login`, `gemini` con la cuenta de Google). La app comprueba si están instaladas y con sesión, y lo enseña en Ajustes.

---

## 10. Fases

| Fase | Contenido | Esfuerzo (1 persona) | Resultado |
| --- | --- | --- | --- |
| **F0. Modo en vivo** | `Backend::Vivo` sobre `Studio`; interruptor en Ajustes > Automatización; un paso de deshacer por turno; estado de la interfaz en la API; **política de confirmación en `Vivo`**. Proponerlo a upstream para #337. | 1,5-2 semanas | `komunta --mcp window` edita lo que se ve, con confirmaciones. Ya sirve el modo `manual`. |
| **F1. Núcleo y Anthropic** | Crate `asistente`: historial neutro, bucle, catálogo común, contexto; adaptador Anthropic; panel `PaneKind.asistente`; herramientas ★; `preview_frame`; claves. | 2-3 semanas | Chat que edita la línea de tiempo con Claude. |
| **F2. OpenAI, Google y respaldo** | Adaptadores OpenAI y Gemini (esquemas, imágenes); cadena de respaldo y port de `cuota.py`; etiqueta «apto para clientes»; preferencia compartida con el panel Montaje. | 1,5-2 semanas | Los tres proveedores API intercambiables. |
| **F3. Guion** | Herramientas del guion (5.4) sobre `PlanCrudo`, con versiones; cambios visibles en el panel Guion; `guion_aplicar` con `graft`; «corregir hablando» sustituye a `edicion.py`. | 1,5-2 semanas | Revisión entera desde el chat. |
| **F4. Montaje, recetas y kits** | `proyecto_nuevo` con IA, `montar`, `rehacer`, estado y cancelar; herramientas de recetas y kits con `montaje::receta` y `kit` (añadiendo `Kit::guardar`). | 1-1,5 semanas | Del material al montaje hablando con el asistente. |
| **F5. CLIs y voz** | Adaptadores `claude_code`, `codex` y `gemini_cli` vía MCP; comprobación de instalación y sesión; dictado local es/eu; las herramientas nuevas también en `concat-mcp`. | 1,5-2 semanas | Los siete modos funcionando. |
| **F6. Evaluación continua** | 30-50 pedidos reales (mitad en euskera) pasados con cada proveedor; acierto, nº de turnos y coste por tarea; ajustar la tabla 6.4, descripciones de herramientas y prompt. | 1 semana y luego continuo | Elegir proveedor por defecto con datos. |

F0 desbloquea todo. F3 y F4 se apoyan en lo que ya hay en `montaje` y en los paneles Montaje y Guion.

---

## 11. Riesgos y preguntas abiertas

- **Calidad desigual entre proveedores** en *tool use* largo y en euskera: lo mide F6; mientras tanto, Anthropic por defecto.
- **Formatos de las CLIs** (salida `stream-json`, opciones de MCP) cambian entre versiones: fijar versión mínima y probar en cada actualización.
- **Divergencia con upstream** en el modo en vivo: seguir las decisiones de #337 y ofrecer el PR.
- **Ediciones simultáneas** (el usuario y el asistente a la vez): el bucle de eventos serializa, pero los identificadores pueden quedar obsoletos; tras `NotFound`, volver a leer y no reintentar a ciegas.
- **Condiciones de uso** de las suscripciones en las CLIs y de los planes gratuitos con material de clientes: confirmar con cada proveedor.
- **Tamaño del catálogo** (≈60 herramientas) con modelos pequeños: cargar por grupos si hace falta.
- **Licencia**: el fork es AGPL y el asistente también; para uso interno no cambia nada.
