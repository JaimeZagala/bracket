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

- **Interfaz:** egui + eframe (backend glow). Decisión aceptada en la Fase 0 tras medirla frente a iced ([decisions/0001-ui.md](decisions/0001-ui.md)). La UI queda detrás de una capa fina.
- **Buffer:** ropey. Selección múltiple desde el inicio. Deshacer/rehacer propio por transacciones (un multicursor o un reemplazo masivo es un solo paso).
- **Resaltado:** syntect con la feature `default-fancy` (sin dependencias de C), gramáticas `.sublime-syntax`/`.tmLanguage`.
- **Símbolos:** tree-sitter para C#, CSS, JSON y XML. T-SQL por reglas regex (CREATE/ALTER de PROCEDURE, FUNCTION, VIEW, TABLE, TRIGGER; `GO` como separador). Razor en tres capas: resaltado TextMate; símbolos de directivas (`@page`, `@inject`, `@using`, `@inherits`) y bloques `@code` pasados al extractor de C#; navegación de `<MiComponente />` a `MiComponente.razor` por nombre de fichero.
- **Proyecto y búsqueda:** ignore, regex + grep-searcher, emparejador difuso propio, trash (Papelera de reciclaje).
- **Otros:** notify, similar, encoding_rs + chardetng, toml_edit (configuración: conserva comentarios al guardar y lee clave a clave; sustituye a serde + toml desde la Fase 1), anyhow + thiserror + tracing, crossbeam-channel.
- **Distribución:** winreg, winresource o embed-resource, ureq, semver, sha2 + ed25519-dalek, self-replace, zip. Instalador con Inno Setup. Publicación con GitHub Actions en runner de Windows.
- **Pruebas:** cargo test, proptest, criterion.

La estructura de crates está en [architecture.md](architecture.md).

## Lenguajes

C#, Razor, CSS, T-SQL, JSON y XML van dentro del ejecutable. Una carpeta `languages/` opcional añade lenguajes o sustituye uno integrado. Orden de búsqueda: `%APPDATA%\Bracket\languages`, `languages/` junto al `.exe` y, por último, lo integrado. En modo portable solo cuenta la carpeta junto al `.exe`.

```text
languages/<id>/language.toml          format_version, nombre, extensiones, insignia, comentarios, tabulación, pares
languages/<id>/syntax.sublime-syntax  opcional; o `scope` en language.toml para usar una gramática incluida
languages/<id>/symbols.toml           opcional (Fase 2): reglas regex de símbolos (nombre, patrón, tipo)
```

`format_version` permite rechazar con un mensaje claro un paquete incompatible. Las gramáticas son `.sublime-syntax` versión 1: syntect no carga `.tmLanguage` ni las gramáticas de Sublime Text 4 con `extends`. `.xsig`, `.xml`, `.xsd`, `.config` y `.csproj` se asocian a XML. Origen y licencia de cada gramática en `THIRD_PARTY.md`.

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

### Fase 1 en detalle

Aprobada el 2026-10-15 y completada el 2026-10-18 (0.1.0, etiqueta local `v0.1.0`). Pasos ordenados por dependencias; cada uno compila, tiene tests y lleva su propio commit.

1. Arranque: perfilar eframe (ventana, contexto OpenGL, fuentes, primer frame), probar opciones de `NativeOptions` midiendo cada cambio y añadir un control del presupuesto de arranque.
2. Configuración en TOML y modo portable (`bracket-session`): `portable.toml` junto al exe o `%APPDATA%\Bracket`.
3. Traducciones: elegir la librería midiendo su tamaño, selector de idioma en la configuración y migración de los textos.
4. Fuentes del sistema: familia configurable por nombre (Consolas por defecto, Cascadia Mono como alternativa, o cualquier fuente instalada, también las instaladas solo para el usuario) y una fuente CJK del sistema como respaldo. No se incluye ninguna fuente en el exe.
5. Codificaciones y saltos de línea (encoding_rs + chardetng): detección, solo lectura si codificar de nuevo no reproduce los bytes exactos, y órdenes para reabrir o guardar con otra codificación y cambiar el salto de línea.
6. Ficheros: Nuevo, Abrir, Guardar y Guardar como, con diálogos nativos.
7. Guardado robusto: `ReplaceFileW` desde la capa de Windows (conserva atributos y ACL) y estado "modificado" exacto al deshacer.
8. Pestañas: varios documentos, pregunta al cerrar con cambios, e insignia de lenguaje en cada pestaña (`C#`, `@`, `#`, `SQL`, `{}`, `<>`). El registro de lenguajes (identificador, nombre, extensiones e insignia) se crea aquí en `bracket-syntax`.
9. Sesión recuperable: pestañas, posiciones y cambios sin guardar.
10. Recarga por cambios en disco (notify).
11. Instancia única y línea de comandos: `bracket fichero:línea`, `--version` y ayuda en los dos idiomas.
12. Resaltado con syntect para los seis lenguajes integrados, en un hilo, con caché por línea y carga diferida de gramáticas.
13. Paquetes de lenguaje externos (`languages/`, `format_version`) y asociación de `.xsig`, `.config` y `.csproj` a XML.
14. Temas: tema del editor e interfaz clara u oscura.
15. Barra de estado completa: lenguaje, codificación, salto de línea, posición y número de selecciones.
16. Integración en `bracket-app`: manifiesto de Windows (DPI, rutas largas), versión e icono en los metadatos del exe.
17. Cierre: mediciones frente a los presupuestos (incluida la RAM con 10 pestañas), CHANGELOG, README y documentación, y etiqueta local `v0.1.0`.

Valores por defecto: ficheros nuevos en UTF-8 sin BOM con CRLF; tabulación de ancho 4. Ambos configurables.

### Fase 2 en detalle

Aprobada el 2026-10-18. Pasos ordenados por dependencias; cada uno compila, tiene tests y lleva su propio commit.

1. Cancelación y generaciones: token de cancelación y número de generación comunes en `bracket-core` para las tareas largas (recorrido, índice, búsquedas).
2. Abrir carpeta como proyecto: menú, línea de comandos (`bracket <carpeta>`), arrastrar una carpeta e instancia única. La sesión recuerda el proyecto y el título muestra la carpeta.
3. Recientes: ficheros y carpetas en Archivo → Recientes, persistentes, sin los que ya no existen.
4. Recorrido del proyecto en `bracket-index` con `ignore` (`.gitignore`, `.ignore`, ocultos y `[project] exclude`), en hilos y cancelable. Generador de un proyecto sintético de 50.000 ficheros y benchmarks.
5. Panel lateral con el árbol: carga perezosa por carpeta en un hilo, redimensionable, Ctrl+B para ocultarlo, teclado, insignia de lenguaje; ancho, visibilidad y carpetas abiertas en la sesión.
6. Pestaña de vista previa: un clic abre una pestaña provisional (cursiva) que reemplaza el siguiente clic; doble clic o editar la fija.
7. Vigilancia del proyecto: `notify` recursivo sobre la raíz actualiza el árbol y el índice de forma incremental, con antirrebote.
8. Acciones del árbol: nuevo fichero, nueva carpeta, renombrar, eliminar a la Papelera de reciclaje (`trash`), copiar ruta y mostrar en el Explorador.
9. Ctrl+P: búsqueda difusa propia sobre el índice, en un hilo, con las letras coincidentes resaltadas, recientes con la consulta vacía, `fichero:línea` y `:línea`. Benchmark frente al presupuesto (< 50 ms por pulsación con 50.000 ficheros).
10. Registro de comandos y paleta (Ctrl+Mayús+P): comandos con id, texto traducido, atajo y disponibilidad; menús y atajos usan el registro; búsqueda difusa; Ir a línea (Ctrl+G).
11. Buscar en el fichero (Ctrl+F): mayúsculas, palabra completa y regex, coincidencias marcadas, contador, F3/Mayús+F3; en ficheros grandes, en un hilo.
12. Reemplazar en el fichero (Ctrl+H): uno a uno o todos (un solo paso de deshacer), capturas `$1`, respetando los saltos de línea.
13. Buscar en la carpeta (Ctrl+Mayús+F): consulta, opciones y filtros de inclusión y exclusión; resultados en streaming por fichero con la línea de contexto; clic para abrir en la coincidencia; cancelable; se saltan binarios y ficheros enormes; respeta la codificación de cada fichero.
14. Reemplazar en la carpeta: vista previa con casillas; en las pestañas abiertas el cambio va al buffer (deshacible, sin guardar); los ficheros cerrados se reescriben con guardado atómico conservando codificación, BOM y saltos de línea; se saltan los de solo lectura o codificación inexacta; resumen al final.
15. Cierre: mediciones (Ctrl+P con 50.000 ficheros, recorrido, velocidad de búsqueda en carpeta, RAM y arranque con proyecto), documentación, CHANGELOG y etiqueta local `v0.2.0`.

Dependencias previstas, cada una medida antes de entrar: `ignore` y `grep-searcher` + `grep-regex` (Unlicense OR MIT) y `trash` (MIT). El emparejador difuso es propio, para no añadir otra licencia (`nucleo-matcher` es MPL-2.0); si no cumpliera el presupuesto, se mediría `nucleo-matcher`.

## Mejoras futuras sin fase asignada

- Iconos SVG opcionales para los lenguajes, definidos en `language.toml` como alternativa a la insignia de texto. El formato de los paquetes se diseña desde la Fase 1 para admitirlos sin romper los existentes.

## Forma de trabajar

1. Antes de escribir código: plan, dudas y riesgos; se espera aprobación.
2. Pasos pequeños: cada paso compila, tiene tests y lleva su propio commit.
3. Antes de cerrar un paso: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
4. Ante un bug: causa raíz primero y un test que lo reproduzca. Tras dos intentos sin progreso, parar y proponer opciones.
5. No inventar APIs: consultar docs.rs o el código de la dependencia.
