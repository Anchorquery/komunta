# Asistente de IA en Komunta: evaluación y propuesta

Estado: evaluación, sin código. Fecha: 28-10-2026.
Alcance: un asistente dentro de Komunta que ayude en todo el flujo (crear montajes, editar el «Guion de montaje», corregir hablando, recetas y kits de marca, y edición normal en la línea de tiempo), en castellano y en euskera.

---

## 1. Recomendación en corto

**Opción (d), híbrida, con el chat propio como núcleo:**

1. **Un chat lateral dentro de Komunta** que llama a la API de Anthropic con *tool use* desde Rust (HTTP directo con `ureq`, que ya está en el árbol). Sus herramientas son las **mismas** que ya expone `concat-mcp` (generadas de los tipos de `concat-api`), más herramientas nuevas para el plan, las recetas y los kits.
2. **Antes que nada, el «modo en vivo»**: que esas herramientas actúen sobre el proyecto abierto en la ventana (hoy no pueden). Es el bloqueo principal; upstream ya lo aprobó como *opt-in* en jub0t/Concat#337 y sigue sin asignar, así que lo hacemos nosotros con el diseño que fijó el mantenedor.
3. **Modelo por defecto: Claude Sonnet 5.5** para el chat; **Claude Opus 5.5** para generar el plan de un montaje nuevo (lo que ya hace el motor Python); Haiku 5.5 solo para tareas internas baratas.
4. **Modo suscripción opcional** (Claude Code CLI conectado a `komunta --mcp window`), como hace hoy el motor, para quien no quiera clave de API. No es la vía principal: dependencias por equipo, peor control de las confirmaciones y límites de uso de la suscripción.
5. **MCP externo** (Claude Desktop o Claude Code al lado) sigue disponible y gana las herramientas nuevas sin coste extra, porque comparten catálogo.

Coste estimado: unos **0,03-0,05 $ por mensaje** con Sonnet 5.5 (≈1-2 $ por una sesión larga de retoques) y **≈1 $ por plan nuevo** con Opus 5.5. Esfuerzo: **≈8-10 semanas** de una persona hasta tener todo el flujo, con algo útil (chat que edita la línea de tiempo) en ≈3-4.

---

## 2. Qué hay ya en el repo (y qué falta)

### 2.1 Piezas que se reutilizan

| Pieza | Dónde | Qué aporta al asistente |
| --- | --- | --- |
| Catálogo de herramientas MCP | `src/crates/concat-mcp/src/tools.rs` (`catalogue()`) | 42 herramientas con nombre, descripción y marcas `read_only` / `destructive` / `idempotent`. Las marcas sirven tal cual para decidir qué se confirma. |
| Esquemas generados | `src/crates/concat-mcp/src/schema.rs` | Esquemas JSON de entrada sacados de los tipos de `concat-api`. Son el `input_schema` de las herramientas de la API de Anthropic sin reescribir nada. |
| `Backend` | `src/crates/concat-mcp/src/backend.rs` | Rasgo `call(Request) -> Value` con dos implementaciones (`Window`, `Headless`). Su propio comentario prevé una tercera: «la ventana con su proyecto abierto (live mode, #337)». |
| API única | `src/crates/concat-api` (`Request`: `project.*`, `edit.apply`, `edit.undo/redo`, `media.*`, `catalogue.list`, `template.*`, `export.*`, `preview.frame`) | Toda edición es `edit.apply` de un `concat_project::Command`. |
| Comandos | `src/crates/concat-project/src/commands/mod.rs` | 54 variantes de `Command`; `Batch { commands }` agrupa varias en **un paso de deshacer**. |
| Gestos | `Editor::apply_within(gesture, command)` (`concat-project/src/editor.rs`) | Varias órdenes con el mismo nombre de gesto se pliegan en un solo paso de deshacer, y cada una devuelve su `created_id`. Ideal para «un turno del asistente = un Ctrl+Z». |
| Ventana | `Studio::apply`, `undo`, `redo`, `after_change` (`src/crates/concat/src/studio.rs`) | Punto de entrada de las ediciones en la ventana: refresca vista, autoguardado, audio y vista previa. |
| Ver el vídeo | herramienta `preview_frame` → `preview.frame` | PNG del fotograma compuesto real (títulos, efectos incluidos), por defecto ajustado a 1280 px. |
| Trabajo en segundo plano | `concat/src/host.rs` (`spawn`, `spawn_in_project`, `spawn_detached`) | Hilos de trabajo con entrega al bucle de eventos de Slint: ahí corre la llamada HTTP del chat sin bloquear la interfaz. |
| Red y carpetas | `concat-host` (`ureq = "2"`, `AppDirs` con `config` en `%APPDATA%` y `data` en `%LOCALAPPDATA%`) | Cliente HTTP ya enlazado; carpetas de configuración donde guardar ajustes del asistente. |
| Voz local | `src/crates/concat-speech` (whisper.cpp en proceso, modelos bajo demanda) | Dictado al chat sin enviar audio fuera («corregir hablando» de verdad). |
| Montaje | `src/crates/montaje` (`Trabajo`, `Plan { tomas, subtitulos, recursos, musica }`, `construir_linea`, `construir_proyecto`) | El plan ya es un tipo Rust con índices de palabra: las operaciones del «Guion de montaje» son funciones puras sobre él. |
| Paneles | `ui/workspace/pane-switcher.slint` (`PaneKind`: media, preview, inspector, timeline, keyframes) y `seat.slint` (`if root.kind == PaneKind.x: …`) | Añadir un panel es una variante más, su etiqueta, su icono y una rama en `seat.slint`. |

### 2.2 Qué falta

- **Modo en vivo.** El servidor de la ventana (`concat-server`, `Hub`) tiene **su propia** `Api`: no ve la sesión de `Studio`. `docs/MCP.md` lo dice: «el agente edita proyectos propios; no edita el proyecto abierto en la ventana». Sin esto, el asistente no puede tocar lo que el usuario está viendo.
- **Estado de la interfaz** (qué clips hay seleccionados, dónde está el cabezal, qué línea de tiempo está activa, idioma): no está en la API. El asistente lo necesita para entender «este subtítulo» o «aquí».
- **Herramientas del plan, recetas y kits**: no existen todavía en ninguna capa.
- **Guardado de claves**: no hay nada (ni `keyring` ni similar) en el árbol.

### 2.3 Upstream (jub0t/Concat)

- **#337 (abierto)**: el mantenedor aceptó el modo en vivo el 18-10-2026 con estas decisiones, que conviene seguir para no divergir:
  - interruptor *opt-in* en Ajustes > Automatización («Let scripts edit the project that's open»), apagado por defecto;
  - con él encendido, una petición que nombra el proyecto abierto se ejecuta **en el bucle de eventos de la ventana, sobre su propia sesión**, con el mismo `Command`, ediciones visibles al momento y lecturas respondidas desde el estado de la ventana; las exportaciones van por la ranura compartida;
  - **un paso de deshacer por llamada a la API** (`batch` ya agrupa).
- El MCP (`concat --mcp`) ya entró en 0.2.8 (lo tenemos). El modo en vivo está **sin asignar** («still open, if you want it»).
- **No hay agente integrado** ni chat en upstream, ni planes publicados para ello. Lo que hagamos de chat es solo nuestro; el modo en vivo sí podría devolverse a upstream (AGPL, mismo diseño) y así reducir el coste de mantener el fork.

### 2.4 Motor Python de referencia (`automatizacion_videos/motor`)

- `cerebros/clis.py`: `claude -p --output-format json --allowedTools Read Glob --permission-mode dontAsk`, con `--resume` para la segunda pasada. Usa la suscripción del equipo.
- `cerebros/apis.py`: Anthropic (por defecto `claude-opus-5-5`, *streaming*, caché del pedido, respaldo si el modelo rechaza), OpenAI y Gemini.
- `cuota.py`: cuotas diarias y cambio de modelo cuando Gemini gratuito se agota.
- `edicion.py`: «corregir hablando» = pedido original + plan actual + **mapa del vídeo con segundos reales** + historial de cambios → plan completo nuevo con un campo `cambios`; cada versión se guarda en `ia/versiones/`.
- `ajustes.py`: claves en `.env` (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`), validadas con una llamada barata, nunca devueltas enteras a la interfaz; catálogo de modelos con coste orientativo.

Lo que se aprovecha: el **mapa del vídeo** (es justo el contexto que necesita el asistente), el **versionado del plan**, la validación de claves y la idea de respaldos. Lo que cambia: en vez de «devuelve el plan entero en JSON», el asistente llama a **operaciones pequeñas** sobre el plan, que se validan una a una y se pueden deshacer sueltas.

---

## 3. Opciones comparadas

| | (a) Chat propio + API de Anthropic | (b) Claude Code CLI / Agent SDK con suscripción + MCP | (c) Solo agente externo vía MCP | (d) Híbrido (recomendado) |
| --- | --- | --- | --- | --- |
| Experiencia | Panel dentro de la app, tarjetas de confirmación, botón «Deshacer» en cada respuesta, chips del guion | Panel dentro de la app, pero el bucle lo lleva la CLI: confirmaciones y progreso más toscos | Otra ventana (Claude Desktop/Code); el equipo tiene que saber configurarlo | La de (a), con (b) y (c) como alternativas |
| Coste | Por uso (≈0,03-0,05 $/mensaje con Sonnet 5.5) | Incluido en la suscripción, con sus límites de uso por ventana de tiempo | Incluido en la suscripción de quien lo use | Por uso por defecto; suscripción si se elige |
| Instalación por equipo | Nada: una clave de API | Node + `claude` instalado y con sesión iniciada en cada equipo | Cliente MCP configurado a mano | Nada por defecto |
| Control | Total: qué se envía, qué se confirma, qué modelo, cuánto gasta | Parcial: hay que pasar por `--mcp-config`, `--allowedTools`, `--permission-prompt-tool` y leer `stream-json` | Ninguno sobre la conversación | Total en el núcleo |
| Latencia | Baja (*streaming* directo) | Arranque de proceso por sesión; aceptable si se mantiene viva con `--input-format stream-json` | Depende del cliente | Baja |
| Sin conexión | No (ninguna opción tiene IA sin red) | No | No | No; la edición manual sigue igual |
| Esfuerzo | Alto (cliente HTTP+SSE, bucle, panel, herramientas) | Medio (panel + proceso hijo + permisos) | Bajo (modo en vivo + herramientas nuevas en MCP) | Alto, pero (b) y (c) salen casi gratis encima |
| Riesgos | Gasto si no se pone tope; mantener el bucle | Condiciones de uso de la suscripción en apps de terceros; cambios de formato de la CLI; límites de uso a mitad de trabajo | El equipo no técnico no lo usará | Más superficie que mantener |

Notas sobre (b):
- La CLI admite servidores MCP por stdio (`--mcp-config` con `komunta --mcp window`), filtrar herramientas (`--allowedTools "mcp__komunta__*"`) y continuar sesiones (`--resume`). Técnicamente encaja.
- Anthropic no permite que productos de terceros ofrezcan el inicio de sesión de claude.ai a través del Agent SDK; para productos piden clave de API. Lanzar la CLI propia del usuario en su propio equipo es lo que ya hace el motor, pero hay que **confirmarlo con las condiciones vigentes del plan del equipo** antes de convertirlo en la vía principal.
- El consumo cuenta contra los límites de la suscripción de cada persona; un montaje largo puede cortarse a medias.

Por eso (b) queda como opción («Usar mi suscripción de Claude») y no como núcleo.

---

## 4. Arquitectura propuesta

```
┌──────────────────────── Komunta (ventana) ─────────────────────────┐
│  Panel «Asistente» (Slint)  ◄──── modelos Slint ────►  panes/asistente.rs
│        │ mensajes, tarjetas, «Deshacer»                    │
│        ▼                                                   ▼
│  crate `asistente`  ── bucle de herramientas ──►  Herramientas (Herramienta)
│   · cliente Messages API (ureq + SSE)            ├─ Edición: catálogo de concat-mcp
│   · contexto (sistema, estado, mapa)             │     └─► Backend «Vivo» ─► Studio (sesión abierta)
│   · política de confirmación                     ├─ Guion: operaciones sobre montaje::Plan
│   · registro de uso y tope de gasto              ├─ Recetas y kits (archivos de datos)
│        │                                         └─ Interfaz: cabezal, selección, ir a…
│        ▼
│   Proveedor: AnthropicApi (por defecto) │ ClaudeCodeCli (opcional)
└─────────────────────────────────────────────────────────────────────┘
          ▲                                    ▲
          │ komunta --mcp window (en vivo)     │ mismas herramientas
   Claude Desktop / Claude Code (opción c)  ───┘
```

### 4.1 Modo en vivo (requisito previo)

- Una tercera implementación de `Backend`: **`Vivo`**, que lleva cada `Request` al bucle de eventos de la ventana (`slint::invoke_from_event_loop`) y la ejecuta contra la sesión de `Studio`; la respuesta vuelve por un canal. Lecturas (`project.get`, `preview.frame`) desde el estado de la ventana.
- El servidor de la ventana usa ese mismo `Vivo` cuando la petición nombra el proyecto abierto y el interruptor de Ajustes está encendido (diseño de #337). Así `komunta --mcp window` también edita en vivo.
- El chat integrado llama a `Vivo` **directamente**, sin socket ni token: está en el mismo proceso.
- Las ediciones del asistente entran por `Studio::apply_within("asistente:<turno>", …)` para que todo un turno sea **un único paso de deshacer** y que cada orden devuelva su `created_id` a tiempo para la siguiente.

### 4.2 Crate `asistente` (nuevo)

- **Cliente HTTP**: Rust no tiene SDK oficial de Anthropic, así que se habla con `POST /v1/messages` en HTTP con `ureq` (ya enlazado), con *streaming* SSE leído línea a línea en un hilo de `host::spawn`. Cancelación con una bandera (botón «Parar»).
- **Bucle**: mientras `stop_reason == "tool_use"`, ejecutar las herramientas pedidas (en paralelo las de lectura), devolver **todos** los `tool_result` en un solo mensaje y seguir. Errores de herramienta como `tool_result` con `is_error: true` (el texto de `CommandError` le sirve al modelo para corregirse).
- **Particularidades de los modelos 5.5** que hay que respetar:
  - `tool_choice` forzado (`any`/`tool`) da error 400 en Opus 5.5 y Sonnet 5.5: usar `auto` + `strict: true` en las herramientas.
  - El pensamiento no se puede desactivar en Opus 5.5; se regula con `output_config.effort` (por defecto `medium` en Opus 5.5, `high` en Sonnet 5.5). Para el chat, `low`/`medium`.
  - Historial **solo de añadir**: no reescribir mensajes anteriores (los bloques de pensamiento quedan invalidados). Cuando el usuario deshace algo a mano o cambia la selección, se **añade** un mensaje de sistema intermedio («el usuario ha deshecho el último cambio»; «cabezal en 12,4 s»), que además no rompe la caché.
  - Comprobar `stop_reason == "refusal"` antes de leer la respuesta; activar el respaldo de servidor (`fallbacks: "default"` con la cabecera beta correspondiente) en Opus/Sonnet 5.5.
- **Caché de prompt**: orden estable `herramientas → sistema → historial`; el catálogo de herramientas y el prompt de sistema (con el glosario es/eu) en caché. Las lecturas en caché cuestan una fracción de la entrada normal y son la mayor parte del contexto en cada turno.
- **Contexto que se envía en cada turno** (pequeño y textual por defecto):
  - estado de la interfaz: proyecto, línea de tiempo activa, cabezal, selección, idioma;
  - `timeline_read` (resumen compacto que ya existe en `concat-mcp/src/summary.rs`);
  - si hay montaje: el **mapa del vídeo** al estilo de `edicion.py::mapa_video` (tomas, subtítulos, recursos y música con segundos reales);
  - transcripciones **solo bajo demanda** (herramienta), por tramos.
- **Proveedor** como rasgo (`Proveedor`): `AnthropicApi` (por defecto) y `ClaudeCodeCli` (opcional, lanza `claude -p --input-format stream-json --output-format stream-json --mcp-config … --allowedTools "mcp__komunta__*"`). OpenAI/Gemini no en la primera versión: el *tool use* de cada proveedor es distinto y duplicaría pruebas; la generación del plan (llamada única, JSON) sí puede conservar los respaldos del motor.

### 4.3 Panel de chat en la interfaz

- Nueva variante `PaneKind.assistant` en `ui/workspace/pane-switcher.slint`, con su texto en `Panes.label` (`paneSwitcher.assistant`), su `id` («assistant») y su icono; nueva rama `if root.kind == PaneKind.assistant: AssistantPane { … }` en `ui/workspace/seat.slint`; componente en `ui/workspace/assistant-pane.slint`; lógica en `src/crates/concat/src/panes/asistente.rs`, como los demás paneles de `src/panes/`.
- Al ser un panel más, el usuario lo coloca en cualquier asiento (por ejemplo, en lugar del inspector), como el resto.
- Elementos: lista de mensajes con *streaming*; **tarjetas de cambio propuesto** (qué va a hacer, con botones «Aplicar» / «Descartar»); en cada respuesta que editó, **«Deshacer este cambio»**; chips clicables que llevan el cabezal a una toma, subtítulo o clip; botón de micrófono (dictado local); indicador de coste de la sesión.
- Textos nuevos en `locales/es.json`, `eu.json` y `en.json`.

### 4.4 Cómo ve el vídeo

Por orden de coste:
1. **Texto primero**: transcripción con índices de palabra, mapa del vídeo, resumen de la línea de tiempo, análisis del material (`analisis.json`: encuadres, BPM, voz/música). Resuelve la mayoría de pedidos («quita la muletilla del principio», «este subtítulo más corto»).
2. **`preview_frame`** cuando lo que se pregunta es visual («¿se lee el título?», «¿tapa la cara?»). Pedirlo a **768 px de lado** en vez de 1280 (≈4 veces menos *tokens* por imagen) salvo que haga falta detalle.
3. **Hojas de contacto** (rejilla de miniaturas de un clip o de un tramo), como las que ya genera el motor para el plan: una imagen sustituye a diez fotogramas.

El vídeo entero nunca se envía.

### 4.5 Deshacer

- **Línea de tiempo**: un turno del asistente = un gesto = **un Ctrl+Z**, compartido con la historia normal de la ventana. El botón «Deshacer este cambio» del chat solo se activa si ese paso es todavía el último (si no, explica que hay cambios posteriores).
- **Guion de montaje**: cada operación crea una versión del plan (como `ia/versiones/` del motor); «volver a la versión N» es una herramienta y un botón.
- **Aplicar el guion**: cada «Aplicar» crea una **línea de tiempo nueva** (`AddTimeline`), así que nunca se pierde la anterior; deshacer es volver a la pestaña anterior.

### 4.6 Confirmar cambios

Política por tipo de herramienta (las marcas ya existen en el catálogo MCP):

| Tipo | Ejemplos | Comportamiento |
| --- | --- | --- |
| Lectura | `timeline_read`, `preview_frame`, `plan_leer` | Sin preguntar |
| Interfaz | mover cabezal, seleccionar | Sin preguntar |
| Edición reversible | recortar, mover, subtítulos, volumen | Se aplica y se muestra con «Deshacer» (modo «Aplicar y avisar»); o tarjeta previa si el usuario elige «Preguntar antes» |
| Guion | operaciones del plan | Siempre como propuesta en el guion (tachado, chips nuevos resaltados) hasta «Aplicar» |
| Destructiva o externa | `clips_remove` masivo, borrar línea de tiempo, `project_save`, exportar sobre un archivo existente, borrar recetas o kits | Siempre con confirmación explícita |

Límite de seguridad: si un turno quiere tocar más de N clips (p. ej. 20), pide confirmación aunque el modo sea «Aplicar y avisar».

### 4.7 Castellano y euskera

- Responder en el idioma de la interfaz (`I18n`) o en el que escriba la persona si es otro.
- Prompt de sistema con el **glosario del fork** (denbora-lerroa, klipa, azpitituluak, esportatu…, ver la nota `euskera-revision.md` de la sesión que tradujo la interfaz) para que el asistente use los mismos términos que los menús.
- Los nombres y descripciones de herramientas pueden quedarse en inglés (los del catálogo MCP) o en castellano; al modelo le da igual y no se muestran.
- Los subtítulos que escriba el asistente en euskera: revisión humana, como cualquier texto de cliente. Preparar un juego de pruebas en euskera (sección 7).
- Dictado: whisper local de `concat-speech`. El motor usa para euskera `xezpeleta/whisper-large-v3-eu-ct2` (formato CTranslate2); para whisper.cpp hace falta su versión GGML o convertirla. Comprobar calidad antes de prometerlo.

---

## 5. Herramientas del asistente

Nombres orientativos. Las marcadas con ★ ya existen en `concat-mcp` y solo hay que conectarlas al `Backend` en vivo.

### 5.1 Contexto e interfaz
- `estado` — proyecto, línea de tiempo activa, cabezal, selección, idioma, si hay montaje y receta.
- `timeline_read` ★, `project_get` ★ (solo si hace falta todo), `concat_version` ★.
- `media_probe` ★, `media_list` (medios del proyecto con duración y tipo).
- `transcripcion_leer(clip, desde, hasta)` — palabras con índice y tiempo.
- `preview_frame` ★ (con tamaño por defecto 768 px), `hoja_contacto(clip | tramo)`.
- `ir_a(segundo | toma | subtítulo | clip)` y `seleccionar(clips)` — mueven la vista para enseñar algo al usuario.
- `effects_list` ★, `template_list` ★.

### 5.2 Edición en la línea de tiempo
- `clip_add` ★, `clips_add_sequence` ★, `clip_trim` ★, `clips_split` ★, `clips_move` ★, `clips_remove` ★ (destructiva), `clip_update` ★, `clip_transform` ★, `clip_speed` ★, `clip_frame` ★, `track_add` ★.
- `title_add` ★, `captions_add` ★, `subtitulo_editar(clip, texto, estilo)` (sobre `clip_update`), `subtitulos_desde_voz(clip)` (transcripción local → `captions_add`).
- `keyframe_set` ★, `keyframe_clear` ★, `effect_layer_add` ★, `effect_key_set` ★, `clip_mask` ★, `timeline_background` ★.
- `musica_poner(archivo | biblioteca, volumen, desde)` y `volumen_bajo_voz(clip, nivel)` (sobre `clip_add` + `clip_update`/`keyframe_set`).
- `edit_apply` ★ (cualquier `Command` o `Batch`), `edit_undo` ★, `edit_redo` ★.
- `export_start` ★, `export_status` ★, `export_wait` ★, `export_cancel` ★, `project_save` ★ (con confirmación).

### 5.3 Guion de montaje (sobre `montaje::Plan`)
- `plan_leer` y `plan_mapa` (mapa con segundos reales, como `mapa_video`).
- `toma_mover(toma, posición)`, `toma_quitar(toma)`, `toma_restaurar(toma)`.
- `palabras_quitar(toma, desde, hasta)` y `palabras_restaurar(toma, desde, hasta)` (el «texto tachable»).
- `recurso_cambiar(recurso, clip, dur, desde_seg)`, `recurso_añadir(clip, ancla)`, `recurso_quitar(recurso)`, `garabato_poner(recurso, tipo, posición)`.
- `subtitulo_texto(n, texto)`, `subtitulo_estilo(n, estilo, posición)`, `subtitulo_añadir(toma, desde, hasta, texto)`, `subtitulo_quitar(n)`.
- `musica_elegir(clip, volumen, desde_seg)`.
- `plan_versiones`, `plan_volver(versión)`.
- `plan_aplicar` — monta el plan en una **línea de tiempo nueva** (`construir_linea` + `construir_proyecto`); siempre con confirmación.

Cada operación valida índices (los errores de `montaje::Error` ya están en castellano: «el plan usa la toma 7, que no existe») y devuelve el plan resultante resumido.

### 5.4 Crear montajes
- `recetas_listar`, `receta_leer(id)`.
- `montaje_redactar(receta, notas)` — propone el texto del «¿Qué quieres?» para que la persona lo edite; no lanza nada.
- `montaje_crear(receta, que_quieres, material[], guion?, subtitulos?)` — lanza análisis + plan (Opus 5.5) como trabajo largo; con confirmación porque cuesta dinero y tiempo.
- `montaje_estado(trabajo)`.

### 5.5 Recetas y kits de marca
- `receta_crear`, `receta_editar(id, cambios)`, `receta_duplicar`, `receta_borrar` (confirmación). Los campos son los del formato de receta acordado; el asistente debe pasar por la misma validación que el editor de recetas, no escribir el archivo a mano.
- `kits_listar`, `kit_leer`, `kit_editar(colores, fuentes, logo, música, cierre)`, `kit_vista_previa` (fotograma de un título o del cierre con ese kit).

Total ≈ 60 herramientas. Es un catálogo grande pero manejable con caché; si en la práctica empeora la elección, se pueden cargar por grupos según el panel activo (o con la herramienta de búsqueda de herramientas de la API).

---

## 6. Modelos y coste

Precios de la API de Anthropic (por millón de *tokens*, tarifa propia de Anthropic; consultados el 28-10-2026):

| Modelo | Entrada | Salida | Lectura de caché | Contexto | Uso propuesto |
| --- | --- | --- | --- | --- | --- |
| Claude Opus 5.5 (`claude-opus-5-5`) | 4 $ | 20 $ | 0,20 $ | 1M | Generar el plan de un montaje nuevo; replanteos grandes («hazlo en 30 s y más dinámico») |
| Claude Sonnet 5.5 (`claude-sonnet-5-5`) | 2 $ | 10 $ | 0,20 $ | 1M | **Chat por defecto**: edición, guion, recetas, kits |
| Claude Haiku 5.5 (`claude-haiku-5-5`) | 0,10 $ | 0,50 $ | — | 1M (precio mayor por encima de 100K) | Tareas internas: título de la conversación, clasificar el pedido, resumir historial |

Todos aceptan imágenes y *tool use*.

Estimación por mensaje con Sonnet 5.5 (supuestos: ≈12K *tokens* en caché —herramientas, sistema, historial—, ≈8K nuevos —estado, mapa, resultados de herramientas—, ≈1,5K de salida):
12K × 0,20 $ + 8K × 2 $ + 1,5K × 10 $ (por millón) ≈ **0,035 $**. Con un fotograma de 768 px se suman unos cientos de *tokens* (céntimos de céntimo). Una sesión de 30 mensajes ≈ **1 $**. Con Opus 5.5, aproximadamente el doble. Un plan nuevo con Opus 5.5: ≈1 $ (estimación ya usada en `ajustes.py`).

Medidas de gasto: tope mensual por equipo (aviso al 80 %, corte al 100 %) guardado en la configuración; contador por sesión visible en el panel; límite de gasto también en la consola de Anthropic (clave por *workspace*).

---

## 7. Privacidad (material de clientes)

- **Qué sale del equipo**: texto (transcripciones, plan, resumen de la línea de tiempo, nombres de archivo) y, solo cuando hace falta, fotogramas sueltos o hojas de contacto. Nunca el vídeo ni el audio: el dictado y la transcripción son locales (whisper).
- **API de Anthropic**: con condiciones comerciales los datos de la API no se usan para entrenar; hay retención limitada y se puede pedir retención cero. **Suscripciones**: los planes de empresa (Team/Enterprise) tienen condiciones comerciales; los planes personales tienen un ajuste de entrenamiento que controla cada usuario. Comprobar el plan y el contrato vigentes antes de usar la vía (b) con material de clientes.
- **En la app**:
  - interruptor «Permitir que el asistente vea fotogramas» (encendido por defecto, apagable por proyecto);
  - opción por proyecto «Proyecto confidencial: sin asistente»;
  - aviso la primera vez que se activa, explicando qué se envía;
  - registro local de cada petición (qué herramientas, qué imágenes, cuánto costó) en la carpeta de datos, para auditoría.
- Las claves nunca se escriben en el proyecto, ni en registros, ni se devuelven enteras a la interfaz (igual que `ajustes.py`).

---

## 8. Sin conexión

Ninguna opción da IA sin red: los modelos útiles para esto son remotos, y un modelo local pequeño no sostiene bien el *tool use* largo ni el euskera. Sin conexión:
- el panel lo dice claramente y deja escribir; el mensaje se puede enviar al volver la red;
- todo lo demás funciona igual: el guion se edita a mano (tachar, arrastrar chips), la línea de tiempo también, y la transcripción y los subtítulos automáticos son locales.

No se recomienda invertir en un modelo local ahora.

---

## 9. Dónde guardar las claves

Orden de búsqueda propuesto:
1. Variable de entorno `ANTHROPIC_API_KEY` (para pruebas y CI; compatible con el motor).
2. **Almacén de credenciales del sistema** (Administrador de credenciales de Windows; Llavero en macOS; Secret Service en Linux) mediante el crate `keyring`. Es la opción recomendada: cifrada por el sistema y ligada al usuario de Windows.
3. Si no hay almacén disponible: archivo en `%APPDATA%\Komunta\` (`AppDirs.config`) con permisos solo para el usuario, como ya hace `automation.json` del servidor.

En Ajustes > Asistente: pegar clave, «Probar» (llamada barata a la lista de modelos, como `probar_y_listar`), mostrar solo los últimos 4 caracteres, «Borrar». Recomendado: **una clave por equipo**, creada en un *workspace* propio de la consola con límite de gasto, en vez de claves personales.

Con la vía (b) no hay clave: usa la sesión de `claude` del usuario (`claude auth login`).

---

## 10. Fases

| Fase | Contenido | Esfuerzo (1 persona) | Resultado |
| --- | --- | --- | --- |
| **F0. Modo en vivo** | `Backend::Vivo` sobre `Studio`; interruptor en Ajustes > Automatización; un paso de deshacer por llamada o turno (`apply_within`); estado de la interfaz en la API; pruebas. Proponerlo a upstream como PR para #337. | 1,5-2 semanas | `komunta --mcp window` edita lo que se ve. Ya sirve la opción (c). |
| **F1. Chat básico** | Crate `asistente`: cliente HTTP+SSE, bucle de herramientas, caché, respaldo ante rechazos, registro de uso y tope; panel `PaneKind.assistant`; herramientas ★ de lectura y edición; `preview_frame`; claves; es/eu. | 2-3 semanas | Pedidos como «baja la música bajo la voz», «añade subtítulos», «exporta en vertical». |
| **F2. Guion de montaje** | Operaciones del plan (5.3) con versiones; propuestas visibles en el guion; `plan_aplicar` → nueva línea de tiempo; «corregir hablando» sustituye a `edicion.py`. | 2 semanas | El flujo de revisión entero desde el chat. |
| **F3. Crear montajes, recetas y kits** | `montaje_redactar`/`crear` con Opus 5.5; herramientas de recetas y kits con la misma validación que sus editores; vista previa de kit. | 1,5-2 semanas | Del material al primer montaje hablando con el asistente. |
| **F4. Voz, suscripción y MCP** | Dictado local (es/eu); proveedor `ClaudeCodeCli` opcional; las herramientas nuevas también en `concat-mcp`. | 1-1,5 semanas | Las tres vías funcionando con el mismo catálogo. |
| **F5. Evaluación continua** | 30-50 pedidos reales (mitad en euskera) con resultado esperado; medir acierto, nº de turnos y coste por tarea; ajustar *effort*, descripciones de herramientas y prompt. | 1 semana y luego continuo | Saber si un cambio de modelo o de prompt mejora o empeora. |

Orden: F0 es imprescindible y desbloquea todo lo demás; F2 depende del panel del «Guion de montaje» que se está diseñando aparte; F3 depende del formato de recetas y kits.

---

## 11. Riesgos y preguntas abiertas

- **Divergencia con upstream** en el modo en vivo: seguir al pie de la letra las decisiones de #337 y ofrecer el PR; si upstream lo implementa antes, adoptar el suyo.
- **Ediciones simultáneas**: el usuario arrastra un clip mientras el asistente edita. El bucle de eventos serializa las órdenes, pero los identificadores pueden quedar obsoletos; el asistente debe volver a leer (`timeline_read`) tras un error `NotFound` y no reintentar a ciegas.
- **Calidad en euskera** de respuestas y subtítulos: medir en F5 con un hablante nativo.
- **Condiciones de la suscripción** para la vía (b): confirmar antes de ofrecerla al equipo.
- **Tamaño del catálogo** (≈60 herramientas): vigilar si el modelo elige peor; plan B, cargar grupos según el panel.
- **Licencia**: el fork es AGPL; el asistente también lo será. Para uso interno no cambia nada; si algún día se distribuye fuera, hay que publicar el código (nunca las claves).
