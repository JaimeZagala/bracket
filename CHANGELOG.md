[English](CHANGELOG.en.md) · **Español**

# Registro de cambios

Todos los cambios visibles de Bracket se anotan aquí.

El formato sigue [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y el proyecto usa [Versionado Semántico](https://semver.org/lang/es/). Antes de la 1.0.0 puede haber cambios incompatibles entre versiones menores; se indican en cada versión.

## [Unreleased]

### Added

- Abrir una carpeta como proyecto: Archivo → Abrir carpeta, `bracket <carpeta>`, arrastrar la carpeta a la ventana o pasársela a un Bracket abierto. El título de la ventana muestra el proyecto y la sesión lo recuerda; Archivo → Cerrar carpeta lo cierra sin tocar las pestañas.
- Recientes: Archivo → Recientes lista los últimos ficheros (15) y carpetas (10) abiertos, sin los que ya no existen, y permite borrar la lista. Se guardan en `recent.toml`.
- Recorrido del proyecto (`bracket-index`): lista los ficheros de una carpeta respetando `.gitignore` (también fuera de un repositorio Git), `.ignore` y la nueva clave `[project] exclude` de `config.toml`; incluye los ficheros ocultos y nunca `.git`, `.svn` ni `.hg`. Va en varios hilos y se puede cancelar: 110 ms con 50.000 ficheros.
- Panel del proyecto: árbol de carpetas a la izquierda con las mismas reglas de exclusión. Cada carpeta se lista en un hilo al abrirla; insignia de lenguaje en los ficheros; redimensionable; Ctrl+B lo oculta y Ctrl+Mayús+E lleva el teclado al árbol (flechas, Inicio/Fin, RePág/AvPág, Intro, Esc). La sesión recuerda su ancho, si está visible y las carpetas abiertas.
- Pestaña de vista previa: un clic en un fichero del árbol lo abre en una pestaña provisional, con el título en cursiva, que el siguiente fichero que se mire reemplaza. Editarla, hacer doble clic en ella o en el fichero, o abrirlo con Intro la convierte en una pestaña normal. La sesión la recuerda.
- Rust como lenguaje integrado (`.rs`, insignia `RS`), resaltado con la gramática de syntect que ya iba en el ejecutable, así que no cambia su tamaño. Las comillas simples no se cierran en pareja, por los *lifetimes* (`'a`).

## [0.1.0] - 2026-10-18

Primera versión del editor base (Fase 1). Sin publicar: solo existe la etiqueta local `v0.1.0`.

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
- Recarga por cambios en disco: si otro programa modifica un fichero abierto sin cambios en Bracket, se recarga solo (deshacer vuelve al texto anterior) y la barra de estado lo indica. Si tenía cambios sin guardar, un aviso ofrece recargar del disco (deshacer recupera los cambios) o mantenerlos. Si el fichero se borra o se mueve, su pestaña queda como modificada para no perder el texto.
- Instancia única: si Bracket ya está abierto, `bracket.exe fichero` abre el fichero en esa ventana y la trae al frente (también si estaba minimizada). Cada carpeta de datos (instalación o copia portable) tiene su propia instancia.
- Línea de comandos: `fichero:línea` y `fichero:línea:columna` abren el fichero con el cursor en esa posición; `--help` y `--version` (la ayuda, en el idioma de la interfaz) se muestran en la consola desde la que se lanzó Bracket, o en un mensaje si no hay consola.
- Corregido: al cerrar la ventana minimizada se guardaba una ventana diminuta en (0, 0) y la sesión no se guardaba.
- Corregido: al abrir Bracket con la ventana maximizada se veía la ventana en negro un momento antes de la interfaz. Además, «Cargando…» solo aparece si la carga dura más de 200 ms, para que no destelle con ficheros normales.
- Al restaurar la sesión solo se lee la pestaña activa (y las que tienen cambios sin guardar); las demás se leen al activarlas. Con dos ficheros grandes en pestañas inactivas, el arranque pasa de 226 MB a 47 MB de memoria.
- Resaltado de sintaxis para C#, Razor, CSS, T-SQL, JSON y XML (syntect, en un hilo por documento). Al escribir no parpadean las demás líneas y, si la edición no cambia la estructura del código, solo se recalcula lo necesario. Los ficheros de más de 16 MB y las líneas de más de 20.000 caracteres se muestran sin colores.
- Paquetes de lenguaje externos: una carpeta `languages/<id>/` con `language.toml` (`format_version = 1`) añade un lenguaje o sustituye uno integrado. Puede traer su gramática (`syntax.sublime-syntax`) o usar una de las que ya incluye Bracket (`scope = "source.python"`), y fijar su insignia y su tabulación. Un paquete con problemas se ignora y la barra de estado lo indica.
- Temas: menú Ver con la apariencia de la interfaz (según Windows, clara u oscura) y el tema de colores del editor (automático según la apariencia, o uno de los siete incluidos: InspiredGitHub, Solarized claro y oscuro, base16 Ocean claro y oscuro, Eighties y Mocha). Se aplican al momento y se guardan en la configuración (`appearance` y `theme`).
- Barra de estado completa, a la derecha: posición (o número de selecciones) con los caracteres seleccionados, salto de línea, codificación, lenguaje del documento e idioma de la interfaz; a la izquierda, los mensajes. Desde el lenguaje se puede cambiar el de la pestaña a mano (también a texto sin formato) y se recuerda en la sesión.
- Icono propio (`[|]` sobre azul) en el ejecutable, la ventana y la barra de tareas, e información de versión en las propiedades del fichero. Manifiesto de Windows: DPI por monitor desde el arranque, rutas largas, Windows 10/11 y aspecto actual de los diálogos del sistema.
