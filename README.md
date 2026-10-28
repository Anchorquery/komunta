# Komunta

Editor de vídeo interno del equipo de comunicación de Komunikia: montaje automático con IA y edición a mano en la misma app.

> **Nombre e icono provisionales.** Komunta es un fork de [Concat](https://github.com/jub0t/concat) (AGPL-3.0-or-later). La marca «Concat» no se usa en las builds modificadas, como pide su [política de marca](TRADEMARK.md). El README original está en [README.concat.md](README.concat.md).

## Estado

- **Fase 0 (esta):** fork de Concat v0.2.8 con otro nombre, icono y color de acento (azul Komunikia), carpeta de ajustes propia (`app.komunta.editor`), instaladores de Windows con identificadores propios y build en GitHub Actions.
- **Siguientes:** pantalla «Nuevo montaje», motor de montaje portado a Rust (`crates/montaje`), agente de IA dentro del editor, subtítulos es/eu.

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
| CI solo Windows, sin disparadores automáticos en los demás workflows | `.github/workflows/` |

Los modelos (Whisper, voces, recorte) se siguen descargando de las publicaciones de Concat.

## Licencia

AGPL-3.0-or-later, igual que Concat. Mientras el uso sea interno no obliga a publicar nada. **Si la app se entrega a terceros** (clientes, ayuntamientos), hay que darles también el código fuente.
