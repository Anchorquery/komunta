# Komunta

Editor de vídeo interno del equipo de comunicación de Komunikia: montaje automático con IA y edición a mano en la misma app.

> **Nombre e icono provisionales.** Komunta es un fork de [Concat](https://github.com/jub0t/concat) (AGPL-3.0-or-later). La marca «Concat» no se usa en las builds modificadas, como pide su [política de marca](TRADEMARK.md). El README original está en [README.concat.md](README.concat.md).

## Estado

- **Fase 0 (esta):** fork de Concat v0.2.8 con otro nombre, icono y color de acento (azul Komunikia), carpeta de ajustes propia (`app.komunta.editor`), instaladores de Windows con identificadores propios y build en GitHub Actions.
- **Fase 1 (hecha):** `crates/montaje` convierte un trabajo del motor de montaje (`ia/plan.json`, `analisis.json`, `inventario.json`) en un proyecto que Komunta abre y se puede editar. La línea de tiempo da los mismos números que el motor en Python (prueba dorada con trabajos reales).
- **Fase 2 (hecha):** recetas editables en lugar de plantillas fijas (subtítulos del documento, de la voz, solo frases clave o ninguno), kit de marca (fuentes, colores, cierre, destello), música con ducking bajo la voz, subtítulos apilados, garabatos y una línea de tiempo por formato (vertical, 16:9, 1:1).
- **Fase 3 (hecha):** pantallas dentro de la app: «Nuevo montaje» en la pantalla de inicio, panel «Guion de montaje» y Ajustes › Montaje e IA. Por debajo sigue trabajando el motor en Python.
- **Siguientes:** el resto del motor en Rust (análisis, transcripción, plan con IA), «Corregir hablando» y agente de IA dentro del editor, aparición palabra a palabra.

### Montaje desde la app

1. **Ajustes › Montaje e IA**: carpeta del motor (por defecto `Escritorio/apps/automatizacion_videos`, o la variable `KOMUNTA_MOTOR`), Python (vacío = el `.venv` del motor) y la IA que planifica. Las recetas propias del equipo van en la carpeta que enseña esa página.
2. **Nuevo montaje** en la pantalla de inicio: carpeta del material, nombre y lugar del proyecto; tipo de vídeo (receta), «¿Qué quieres?» con tus palabras, formatos y marca; subtítulos (del guion, de la voz, frases clave o ninguno), guion opcional e idioma. El motor tarda unos minutos y el proyecto se abre solo.
3. **Guion de montaje** (sale donde estaba la Biblioteca): pulsa una palabra para cortarla, sube, baja o quita tomas, corrige subtítulos y quita recursos. **Aplicar al vídeo** guarda el plan como versión nueva en el trabajo y añade el montaje como líneas de tiempo nuevas («Montaje 2 · Vertical»…), sin tocar las que ya editaste.

El proyecto guarda en `montaje.json` qué trabajo, receta, marca y formatos lo hicieron.

### Montaje desde la línea de órdenes

```sh
cd src
cargo run -p montaje -- nuevo <material> <carpeta del proyecto> --motor <carpeta del motor> [--receta <id>] [--peticion "<texto>"] [--guion <archivo>] [--idioma es|eu] [--marca <id>] [--formatos vertical,16x9] [--ia <cerebro>]
```

### Montaje desde un trabajo ya hecho del motor

```sh
cd src
cargo run -p montaje -- <carpeta del trabajo> <carpeta del proyecto> [opciones]
```

| Opción | Qué hace | Por defecto |
|---|---|---|
| `--receta <id>` | `mensaje_a_camara`, `voz_en_off`, `entrevista`, `resumen_acto` o una propia | la plantilla del trabajo |
| `--recetas <carpeta>` | recetas propias del equipo (JSON) | — |
| `--formatos vertical,16x9,1x1` | una línea de tiempo por formato; el primero es el principal | los de la receta |
| `--marca <id>` | kit de `marcas/<id>/kit.yaml` | la marca del trabajo |
| `--marcas <carpeta>` | carpeta `marcas` del motor | dos carpetas por encima del trabajo |

Luego, en Komunta, **Abrir proyecto** y elegir la carpeta del proyecto (o `concat.exe --project <carpeta>`). Cada formato es una línea de tiempo del mismo proyecto. Pistas, de abajo arriba: voz y cierre, destello, recursos, garabatos, subtítulos, palabra gigante de los énfasis, música. Los garabatos se dibujan como PNG en `<proyecto>/montaje/garabatos`. Aún no pasa al proyecto: la aparición palabra a palabra (necesita soporte en el motor de texto).

## Descargar la build de Windows

Pestaña **Actions** → workflow **Komunta (Windows)** → última ejecución en verde → artefacto **Komunta-windows-x86_64**. Dentro van el instalador `Komunta-<versión>-windows-x86_64-setup.exe` y el `.msi`.

La build no está firmada: si Windows SmartScreen la para, **Más información** → **Ejecutar de todas formas**.

Para lanzar una build a mano: Actions → Komunta (Windows) → **Run workflow**.

## Compilar en local

Igual que el original (ver [README.concat.md](README.concat.md#build-from-source)): Rust 1.93+, FFmpeg 7+ (BtbN `win64-gpl-shared` con `FFMPEG_DIR`), cmake y Visual Studio 2022 Build Tools. Ocupa entre 20 y 30 GB.

```sh
cd src
cargo run -p concat
```

Los crates internos conservan el nombre `concat-*` a propósito, para poder traer mejoras del original sin conflictos.

## Traer mejoras de Concat

```sh
git fetch upstream --tags
git merge v0.2.9   # o la versión que toque
```

El remoto `upstream` apunta a `jub0t/concat` y tiene el push desactivado.

## Cambios respecto a Concat

| Qué | Dónde |
|---|---|
| Nombre en ventana, textos (25 idiomas), carpeta de proyectos por defecto | `src/crates/concat/ui/app.slint`, `locales/*.json`, `panes/start.rs`, `platform.rs` |
| Carpeta de ajustes y modelos: `app.komunta.editor` | `src/crates/concat-host/src/dirs.rs` |
| Actualizaciones y enlaces apuntan a este repo | `concat-host/src/updates.rs`, `panes/start.rs` |
| Castellano por defecto (hasta que se elija otro idioma en Ajustes) | `src/crates/concat/src/i18n.rs` (`DEFAULT`), `studio.rs`, `panes/settings.rs` |
| Inicio sin tarjeta de Discord; «Informar de un problema» abre una incidencia aquí; «Novedades» lleva a este README. La documentación sigue siendo la guía de Concat (es el mismo editor) | `ui/start.slint`, `panes/start.rs` |
| «Komunta» en el diálogo de error al arrancar, la información del sistema, el User-Agent y las instrucciones del servidor MCP | `platform.rs`, `sysinfo.rs`, `concat-host/src/machine.rs`, `concat-mcp/src/` |
| Acento por defecto azul Komunikia | `ui/theme/light.slint`, `ui/theme/dark.slint` |
| Icono | `assets/icons/`, `assets/concat-*.png` |
| Instaladores con nombre e identificadores propios | `assets/windows/concat.iss`, `concat.wxs` |
| Montaje: trabajo del motor → proyecto editable | `src/crates/montaje` |
| CI solo Windows, sin disparadores automáticos en los demás workflows | `.github/workflows/` |

Los modelos (Whisper, voces, recorte) se siguen descargando de las publicaciones de Concat.

## Licencia

AGPL-3.0-or-later, igual que Concat. Mientras el uso sea interno no obliga a publicar nada. **Si la app se entrega a terceros** (clientes, ayuntamientos), hay que darles también el código fuente.
