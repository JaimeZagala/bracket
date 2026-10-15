[English](../en/plan.md) · **Español**

# Plan del proyecto

Documento de referencia con el alcance, las restricciones y las fases de Bracket. Cualquier cambio de alcance se refleja aquí en los dos idiomas.

## Objetivo

Editor de código nativo para Windows 10/11 x64, en un único `.exe`, a medio camino entre Notepad++ y VS Code. No es un IDE completo. Debe permitir:

- Abrir, editar y guardar ficheros con pestañas, sesión recuperable y recarga automática si cambian en disco.
- Abrir carpetas como proyecto: panel lateral con vista previa, Ctrl+P (fichero), Ctrl+T (símbolo en el proyecto), Ctrl+Shift+O (símbolo en el fichero), paleta de comandos, buscar y reemplazar con regex en fichero y carpeta.
- Navegar por símbolos: outline, ir a definición y referencias por nombre (aproximadas, sin compilador).
- Resaltar y navegar C#, Razor, CSS, T-SQL, JSON y XML/XSIG, integrados en el ejecutable. Lenguajes nuevos se añaden copiando una carpeta, sin recompilar.
- Instalarse con un instalador por usuario o usarse como zip portable, con "Abrir con", asociaciones de ficheros y registro como candidato a editor predeterminado.
- Detectar versiones nuevas y actualizarse solo cuando el usuario lo indique, con canal estable (por defecto) y canal preliminar opcional.
- Interfaz y documentación en español (predeterminado) e inglés.

## Presupuestos de rendimiento

Se miden, no se suponen. Si alguno no es alcanzable, se informa con datos y alternativas.

| Medida | Presupuesto |
|---|---|
| Ejecutable release (con lenguajes integrados y actualizador) | < 15 MB |
| Arranque en frío hasta poder escribir | < 150 ms |
| RAM en reposo con 10 pestañas | < 80 MB |
| Fichero de 100 MB | contenido visible en < 1 s sin bloquear la interfaz |
| Ctrl+P en un proyecto de 50.000 ficheros | < 50 ms por pulsación |

## Restricciones

1. Rust estable, target `x86_64-pc-windows-msvc`. Sin nightly.
2. El núcleo (todo menos la interfaz y la integración con Windows) no depende del framework gráfico ni de APIs de Windows.
3. Sin telemetría. Única conexión de red: comprobación de actualizaciones, desactivable, como máximo una vez al día, sin datos identificativos y sin descargar ni instalar nada sin confirmación. El editor nunca ejecuta lo que abre.
4. Sin pérdida de datos: guardado atómico (temporal + renombrado), conservación exacta de codificación, BOM y saltos de línea. En XML/XSIG no hay limpieza automática: se guardan los bytes tal cual.
5. El hilo de interfaz solo pinta y gestiona eventos. Indexado, búsqueda, carga de carpetas, resaltado pesado y red van en hilos de trabajo con cancelación.
6. Cada dependencia nueva se justifica: mantenimiento, licencia e impacto medido en tamaño y arranque. Versiones y APIs se comprueban con cargo y docs.rs.
7. Sin `unsafe` salvo necesidad demostrada y documentada con un comentario `SAFETY`.
8. No se avanza de fase sin aprobación del autor. Sin refactors fuera del alcance.
9. Repositorio público: ningún dato de empresas ni de clientes, ningún secreto. Identidad de Git personal configurada a nivel de repositorio. Revisión del diff antes de cada push.
10. Nada se crea ni publica en GitHub sin confirmación explícita del autor.

## Stack

- **Interfaz:** egui + eframe (backend glow). Decisión provisional, validada en la Fase 0 (ver `decisions/`). La UI queda detrás de una capa fina.
- **Buffer:** ropey. Selección múltiple desde el inicio. Deshacer/rehacer propio por transacciones (un multicursor o un reemplazo masivo es un solo paso).
- **Resaltado:** syntect con la feature `default-fancy` (sin dependencias de C), gramáticas `.sublime-syntax`/`.tmLanguage`.
- **Símbolos:** tree-sitter para C#, CSS, JSON y XML. T-SQL por reglas regex (CREATE/ALTER de PROCEDURE, FUNCTION, VIEW, TABLE, TRIGGER; `GO` como separador). Razor en tres capas: resaltado TextMate; símbolos de directivas (`@page`, `@inject`, `@using`, `@inherits`) y bloques `@code` pasados al extractor de C#; navegación de `<MiComponente />` a `MiComponente.razor` por nombre de fichero.
- **Proyecto y búsqueda:** ignore, regex + grep-searcher, nucleo-matcher.
- **Otros:** notify, similar, encoding_rs + chardetng, serde + toml, anyhow + thiserror + tracing, crossbeam-channel.
- **Distribución:** winreg, winresource o embed-resource, ureq, semver, sha2 + ed25519-dalek, self-replace, zip. Instalador con Inno Setup. Publicación con GitHub Actions en runner de Windows.
- **Pruebas:** cargo test, proptest, criterion.

La estructura de crates está en [architecture.md](architecture.md).

## Lenguajes

C#, Razor, CSS, T-SQL, JSON y XML van dentro del ejecutable. Una carpeta `languages/` opcional añade lenguajes o sustituye uno integrado. Orden de búsqueda: `%APPDATA%\Bracket\languages`, `languages/` junto al `.exe` y, por último, lo integrado. En modo portable solo cuenta la carpeta junto al `.exe`.

```text
languages/<id>/language.toml          format_version, nombre, extensiones, comentarios, tabulación, pares
languages/<id>/syntax.sublime-syntax  (o .tmLanguage)
languages/<id>/symbols.toml           opcional: reglas regex de símbolos (nombre, patrón, tipo)
```

`format_version` permite rechazar con un mensaje claro un paquete incompatible. `.xsig`, `.xml`, `.xsd`, `.config` y `.csproj` se asocian a XML. Origen y licencia de cada gramática en `THIRD_PARTY.md`.

## Versionado y publicación

- SemVer 2.0.0. Versión única en `[workspace.package]`, visible con `--version`, en Acerca de y en los metadatos del `.exe`.
- Antes de la 1.0.0: `0.y.z`, una versión menor por fase.
- Antes de cada versión MAJOR: `X.0.0-prerelease.N`, luego `X.0.0-rc.N` (con punto) y después `X.0.0`. En prerelease y RC no entra funcionalidad nueva; se trabaja en la rama `release/X.0.0` con commits `fix:`, `docs:` y `chore:`.
- `CHANGELOG.md` y `CHANGELOG.en.md` en formato Keep a Changelog.
- Etiquetas `vX.Y.Z` y `vX.Y.Z-rc.1`. Toda etiqueta con guion se publica como pre-release. `main` siempre compila y pasa los tests. Conventional Commits.

## Instalación, modo portable y actualizaciones (Fase 5)

- Mismo ejecutable en dos formas: instalador por usuario (Inno Setup, en `%LOCALAPPDATA%`, sin administrador) y zip portable con `portable.toml` junto al `.exe`.
- El instalador registra extensiones, "Abrir con", "Editar con Bracket", "Abrir como proyecto en Bracket", menú Inicio y desinstalador.
- En modo portable, configuración, sesión y lenguajes viven junto al ejecutable y no se escribe en `%APPDATA%` ni en el registro.
- Comandos "Registrar en Windows" y "Quitar registro" (solo HKCU).
- Editor predeterminado: Bracket se registra como candidato y abre la página de aplicaciones predeterminadas de Windows. Sin trucos para saltarse esa protección.
- Firma de código antes de la 1.0.0; si no hay certificado, se explica en el README y se publican las sumas de comprobación.
- Actualizador: canales estable y preliminar, API pública de GitHub sin autenticar, como máximo una vez al día, desactivable. Avisa, muestra el changelog y solo descarga e instala cuando el usuario lo indica.
- Seguridad: cada release publica `SHA256SUMS` y una firma ed25519 hecha con una clave privada que no está en el repositorio ni en GitHub. El editor lleva la clave pública y no instala nada sin verificar. Procedimiento de rotación documentado.
- Aplicación: el exe en uso se renombra a `.old`, se coloca el nuevo y se reinicia; si el nuevo no arranca, se restaura el anterior. Copia de la configuración antes de actualizar.
- Reportar errores: un botón abre un issue de GitHub con versión y sistema rellenados. No se envía nada automáticamente.

## Documentación

README, CHANGELOG, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY y THIRD_PARTY en español (`X.md`) e inglés (`X.en.md`). `docs/es/` y `docs/en/` con `user-guide.md`, `languages.md`, `architecture.md`, `release.md`, `plan.md` y `decisions/`. Licencias solo en inglés; `CLAUDE.md` solo en español. Las ventajas que afirme el README solo pueden ser las respaldadas por mediciones reproducibles del propio repositorio.

## Idiomas

- Código y ficheros en inglés.
- Documentación en español (referencia) e inglés, actualizadas en el mismo commit. `scripts/check-doc-pairs.ps1` falla si falta una pareja.
- Notas de release: primero español y después inglés.
- Interfaz: el usuario elige español (predeterminado) o inglés. Los textos visibles van en catálogos de traducción, uno por idioma, cargados por un único módulo. La solución de traducción se decide en la Fase 1 midiendo su impacto en el tamaño.

## Fases

Cada fase termina con una versión usable. Criterios de cierre: tests y clippy en verde, benchmarks sin regresión, CHANGELOG, README y documentación afectada actualizados en los dos idiomas, y una nota en `decisions/` si cambió la arquitectura.

| Fase | Versión | Alcance |
|---|---|---|
| 0 | — | Cimientos y prototipo del editor. |
| 1 | 0.1.0 | Editor base: pestañas, sesión, codificación y saltos de línea, recarga por cambios en disco, guardado atómico, resaltado con los seis lenguajes integrados y paquetes externos, temas, configuración, barra de estado, instancia única, modo portable, interfaz en español e inglés con selector de idioma. |
| 2 | 0.2.0 | Proyectos: panel de carpetas con `.gitignore`, vista previa, recientes, Ctrl+P, paleta de comandos, buscar y reemplazar en fichero y carpeta. |
| 3 | 0.3.0 | Símbolos y navegación: extractores, índice incremental, outline, Ctrl+T, ir a definición, referencias, ruta XPath y JSON bajo el cursor, `languages.md`. |
| 4 | 0.4.0 | Edición avanzada: multicursor completo, plegado, diff, paréntesis y palabras iguales, mover/duplicar/comentar líneas, modo para ficheros grandes. |
| 5 | 0.5.0 → 1.0.0 | Distribución y actualizaciones: instalador en español e inglés, zip portable, registro en Windows, actualizador con canales, firma, publicación automatizada, documentación completa y ciclo prerelease/RC de la 1.0.0. |
| 6 | 1.1.0 | LSP opcional, desactivado por defecto. |

### Fase 0 en detalle

1. Workspace con las ocho crates, toolchain fijado, perfil de release.
2. Documentación bilingüe inicial y scripts de comprobación.
3. Hola mundo con eframe como línea base de tamaño, arranque y RAM.
4. `bracket-core`: buffer sobre ropey con selecciones múltiples.
5. Inserción y borrado en todas las selecciones.
6. Deshacer/rehacer por transacciones con tests de propiedades.
7. Movimiento de cursor independiente de la UI.
8. Vista del editor que pinta solo las líneas visibles, con scroll.
9. Selección con teclado y ratón, portapapeles, deshacer/rehacer.
10. IME y tabulaciones.
11. Abrir (argumento o arrastrar) y guardar (temporal + renombrado; solo UTF-8 en esta fase).
12. Benchmarks con criterion: apertura, inserción en medio, pintado del viewport.
13. Prototipo comparable con iced fuera del workspace y mediciones.
14. Decisión de interfaz (`decisions/0001-ui.md`) e informe final.

## Forma de trabajar

1. Antes de escribir código: plan, dudas y riesgos; se espera aprobación.
2. Pasos pequeños: cada paso compila, tiene tests y lleva su propio commit.
3. Antes de cerrar un paso: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
4. Ante un bug: causa raíz primero y un test que lo reproduzca. Tras dos intentos sin progreso, parar y proponer opciones.
5. No inventar APIs: consultar docs.rs o el código de la dependencia.
