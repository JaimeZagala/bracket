[English](CHANGELOG.en.md) · **Español**

# Registro de cambios

Todos los cambios visibles de Bracket se anotan aquí.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/). Antes de la 1.0.0 puede haber cambios incompatibles entre versiones menores; se indican en cada versión.

## [Unreleased]

### Added

- Workspace de Cargo con ocho crates (`bracket-core`, `bracket-syntax`, `bracket-index`, `bracket-search`, `bracket-session`, `bracket-update`, `bracket-ui`, `bracket-app`), toolchain fijado y perfil de release optimizado.
- Documentación inicial en español e inglés y comprobación de parejas de idioma.
- Doble licencia MIT o Apache-2.0.
- Ventana principal de `bracket.exe` (egui/eframe con backend OpenGL) y script de medición de tamaño, arranque y memoria.
- Prototipo del editor: abrir un fichero UTF-8 (argumento o arrastrar a la ventana), editar con selecciones múltiples, deshacer/rehacer, portapapeles, IME, tabulaciones y guardado atómico que conserva BOM y saltos de línea. Los ficheros que no son UTF-8 se abren en solo lectura.
- Configuración en `config.toml` (idioma de la interfaz, tamaño de letra, ancho de tabulación y salto de línea de los ficheros nuevos) con avisos en la barra de estado si algún valor no es válido.
- Modo portable: con `portable.toml` junto al exe, todos los datos se guardan en esa carpeta.
- Guía de usuario en `docs/es/user-guide.md`.
- Selector de idioma en la barra de estado: cambia la interfaz al momento y lo guarda en la configuración. Los textos de la interfaz están en catálogos, uno por idioma (`es.toml` y `en.toml`).
- Fuentes del sistema: la fuente del editor es configurable (`font_family`, Consolas por defecto) y se cargan bajo demanda fuentes de respaldo para símbolos, chino, japonés y coreano. El ejecutable ya no incluye fuentes (1,36 MB menos).
- Codificaciones: se detectan UTF-8, UTF-16 (con o sin BOM) y codificaciones heredadas (windows-1252, Shift_JIS, GBK...), y el fichero se guarda en la misma codificación. Si no se puede conservar byte a byte, se abre en solo lectura. Menús en la barra de estado para reabrir o guardar con otra codificación y para convertir los saltos de línea (en un solo paso deshacible). Ctrl+S funciona aunque el foco esté en un menú.
- Menú Archivo con Nuevo (Ctrl+N), Abrir (Ctrl+O), Guardar (Ctrl+S), Guardar como (Ctrl+Mayús+S) y Salir, con los diálogos nativos de Windows. Guardar un documento sin título abre Guardar como.
- Al guardar sobre un fichero existente se conservan sus atributos, permisos y fecha de creación (`ReplaceFileW`). Deshacer hasta el texto guardado deja el documento como no modificado.
- Pestañas: cada fichero se abre en su pestaña, con una insignia de lenguaje (C#, Razor, CSS, T-SQL, JSON, XML) y un punto mientras tiene cambios sin guardar. Ctrl+W o clic central cierran, Ctrl+Tab y Ctrl+RePág/AvPág cambian de pestaña. Abrir un fichero ya abierto lleva a su pestaña. Se pueden pasar varios ficheros por argumento.
- Sesión recuperable: al cerrar Bracket se guardan las pestañas abiertas, la posición del cursor y del scroll, y el texto de los documentos con cambios sin guardar (también los sin título); al abrirlo vuelve todo como estaba, con esos cambios aún pendientes de guardar. La sesión se guarda también unos segundos después de cada cambio, así que un cierre inesperado pierde como mucho esos segundos. Salir ya no pregunta por los cambios sin guardar.
- La ventana recuerda su posición, su tamaño y si estaba maximizada; si su monitor ya no está conectado, se abre en uno visible. La primera vez se abre centrada (antes podía quedar en parte por debajo de la barra de tareas).
