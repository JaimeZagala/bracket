[English](../en/architecture.md) · **Español**

# Arquitectura

Estado: Fase 1 completada (0.1.0). Este documento describe la estructura acordada; cada sección indica qué está implementado.

## Crates del workspace

| Crate | Responsabilidad | Depende de (workspace) | Estado |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), selecciones múltiples, deshacer/rehacer por transacciones, movimiento de cursor, codificación, saltos de línea, modelo de documento. Sin UI ni APIs de Windows. | — | Fase 1: buffer, selecciones, historial con estado guardado exacto, movimiento, codificaciones, saltos de línea, documento con guardado atómico, recarga y sellos de disco |
| `bracket-syntax` | Carga de lenguajes integrados y externos, resaltado incremental por líneas con caché, temas. | `bracket-core` | Fase 1: registro de lenguajes (los seis integrados, extensiones, insignia y gramática) y resaltado con syntect. Fase 2: Rust como séptimo lenguaje integrado |
| `bracket-index` | Recorrido de proyecto, extracción de símbolos (tree-sitter y regex), índice incremental, búsqueda difusa, referencias por nombre. | `bracket-core` | Fase 2: recorrido del proyecto con `ignore` (índice de ficheros y listado de una carpeta) |
| `bracket-search` | Buscar y reemplazar con regex en fichero y carpeta, cancelable, con resultados en streaming. | `bracket-core` (previsto) | Vacía |
| `bracket-session` | Configuración TOML, modo portable, sesión recuperable, recientes, vigilancia de ficheros, registro de comandos. | `bracket-core` | Fase 1: rutas (modo portable o `%APPDATA%`), configuración TOML, sesión recuperable y vigilancia de ficheros |
| `bracket-update` | Comprobación de versiones por canal, descarga, verificación de suma y firma, aplicación y vuelta atrás. | — | Vacía |
| `bracket-ui` | Interfaz egui: vista del editor, pestañas, árbol de proyecto, paleta, barra de estado. | `bracket-core`, `bracket-index`, `bracket-session`, `bracket-syntax` | Fase 1: pestañas, menú Archivo, barra de estado, fuentes del sistema, catálogos de textos. Fase 2: árbol del proyecto en el panel lateral |
| `bracket-app` | Binario `bracket.exe`: arranque, instancia única, argumentos, manifiesto de Windows, registro en HKCU. | `bracket-ui`, `bracket-session`, `bracket-syntax`, `bracket-update` | Fase 0: arranque y fichero por argumento. Fase 1: resuelve rutas, carga la configuración y los paquetes de lenguaje, instancia única y línea de comandos |

## Reglas de dependencias

```text
bracket-app ──► bracket-ui ──► bracket-syntax ─┐
     │               ├──────► bracket-index  ──┤
     │               ├──────► bracket-search ──┼──► bracket-core
     │               └──────► bracket-session ─┘
     └──► bracket-update   (aislada)
```

- Solo `bracket-app` depende de `bracket-ui` y de `bracket-update`.
- `bracket-core` y `bracket-update` no dependen de ninguna otra crate del workspace.
- Las flechas siempre van hacia abajo; no hay ciclos.
- Nada por debajo de `bracket-ui` usa egui ni APIs de Windows. Así la interfaz se puede sustituir sin tocar el núcleo.
- Las dependencias entre crates se añaden cuando se usan, no antes.

## Hilos y comunicación

- El hilo de interfaz solo pinta y gestiona eventos.
- Indexado, búsqueda, carga de carpetas, resaltado pesado y red van en hilos de trabajo.
- La interfaz envía comandos por canales y recibe eventos. Cada tarea larga lleva un token de cancelación y un número de generación; el resultado de una generación ya superada se descarta.

Los hilos de trabajo usan `std::thread` y `std::sync::mpsc`. `bracket-core` (`task.rs`) da a cada tarea larga un `Task`: un `CancelToken` que la tarea consulta entre unidades de trabajo y un número de generación que devuelve con sus resultados. `Generations` lleva la tarea actual de cada tipo: empezar otra cancela la anterior, y los resultados de una generación que ya no es la actual se descartan aunque lleguen tarde.

## Modelo del núcleo (`bracket-core`)

- **Posiciones:** índices de carácter (valores escalares Unicode), la unidad nativa de ropey 1.x. No puede haber posiciones a mitad de un carácter UTF-8. tree-sitter y regex trabajan en bytes; la conversión (O(log n)) se hará en el borde.
- **Saltos de línea:** solo LF, CRLF y CR parten líneas, como en Notepad++ y VS Code (U+2028 no). El texto nunca se normaliza; el salto de línea del documento (el primero que aparece, CRLF para ficheros nuevos) solo se usa para el texto nuevo: Intro y pegado.
- **CRLF como unidad:** el cursor nunca se queda entre `\r` y `\n`, y Retroceso/Supr borran el par completo. Los clústeres de grafemas (p. ej. `e` + acento combinante) todavía no se tratan como unidad.
- **Selecciones:** conjunto no vacío, ordenado y sin solapes, con una selección principal. Cada selección tiene ancla, cabeza y columna objetivo para moverse en vertical.
- **Edición:** toda edición se planifica por selección contra el texto anterior y se aplica como una sola transacción (lista de cambios con el texto borrado, invertible).
- **Deshacer:** escribir caracteres seguidos o borrar carácter a carácter forma un solo paso; pegar, Intro, borrar una selección y las ediciones multicursor son pasos propios. Mover el cursor corta el grupo.
- **Estado modificado:** cada paso del historial tiene un identificador único y la versión del buffer es la del último paso aplicado; deshacer y rehacer recuperan identificadores anteriores, así que volver al texto guardado lo deja como no modificado.
- **Reemplazo de ficheros:** el núcleo define el trait `FileReplacer` (por defecto, renombrar). `bracket-app` usa `ReplaceFileW` (`file_replace.rs`), que conserva atributos, ACL y fecha de creación. Junto con `console.rs` (ver abajo), son los únicos bloques `unsafe` del proyecto, cada uno con su comentario `SAFETY`.
- **Documento:** buffer + ruta + codificación + salto de línea + versión guardada. Guardado atómico: fichero temporal en la misma carpeta, `sync_all` y renombrado sobre el destino.
- **Codificaciones (`encoding.rs`):** detección por BOM, UTF-16 sin BOM (patrón de bytes cero), UTF-8 válido y, si no, `chardetng`. Un fichero solo es editable si codificar de nuevo su texto reproduce exactamente sus bytes; si no, solo lectura. Al guardar, un carácter que no existe en la codificación hace fallar el guardado (encoding_rs lo sustituiría por `&#NNNN;`). UTF-16 se codifica a mano porque encoding_rs solo escribe UTF-8 para él. El UTF-8 válido se decodifica sin copiar los bytes.

## Ficheros y atajos (Fase 1)

- `dialogs.rs` envuelve los diálogos nativos de `rfd` con la ventana de Bracket como padre: son modales y se ejecutan en el hilo de interfaz, como en cualquier editor.
- Los atajos de la aplicación (Ctrl+N/O/S, Ctrl+Mayús+S) se consumen en la app antes de pintar el editor, así que funcionan esté donde esté el foco. Guardar un documento sin título pasa por Guardar como.

## Pestañas (Fase 1)

- `tabs.rs` define `Tab` (documento, vista, lenguaje, carga y guardado en curso) y dibuja la barra de pestañas. La app guarda un `Vec<Tab>` que nunca está vacío y el índice de la activa; la barra de estado, el título y los comandos actúan sobre la activa.
- Cada pestaña tiene un identificador único durante la ejecución y su `EditorView` deriva de él su `Id` de egui, así que el foco y el desplazamiento de cada pestaña son independientes.
- La carga y el guardado van por pestaña en hilos de trabajo; el resultado llega a su pestaña aunque ya no sea la activa. Si falla la carga de un fichero abierto en una pestaña nueva, la pestaña se cierra.
- El lenguaje de cada pestaña sale de `LanguageRegistry::for_path` (`bracket-syntax`) al abrir y al guardar como. Las insignias son texto con color; el registro admite añadir iconos SVG más adelante.

## Sesión (Fase 1)

- `bracket-session` (`session.rs`) define el formato: `session.toml` con las pestañas (ruta, codificación forzada, selección principal, primera línea visible) y un fichero UTF-8 por documento modificado en `backups/`. Guardar escribe primero las copias y después `session.toml`, ambos de forma atómica, así que la lista nunca apunta a una copia a medias.
- Solo se borran copias que la sesión actual ya no usa y que son "propias": las restauradas al arrancar o las escritas en esta ejecución. Los nombres llevan la hora de arranque y el PID, así que una ejecución nunca sobrescribe las copias de otra. Una sesión dañada se aparta como `session.damaged.toml` y sus copias no se tocan.
- `bracket-ui` (`session.rs`) restaura cada pestaña en un hilo: lee el fichero y aplica encima el texto guardado como un único paso deshacible (`Document::restore_unsaved`), así que deshacer vuelve al texto del disco. `SessionKeeper` guarda en un hilo 2 s después del último cambio (detectado con un hash del estado) y reescribe una copia solo si su texto cambió. Al cerrar la ventana guarda en el hilo de interfaz; si falla, vuelve a preguntar por los cambios sin guardar.
- Carga diferida: al arrancar solo se restauran la pestaña activa y las que tienen texto sin guardar (así cerrar y «Guardar todo» siguen viendo sus cambios). Las demás quedan como `Tab::unloaded` con su entrada de la sesión, que se vuelve a guardar tal cual hasta que la pestaña se activa por primera vez.
- `bracket-app` lee la sesión al arrancar (30 µs con 10 pestañas).
- La ventana (posición, tamaño, maximizada) la guarda eframe con su feature `persistence` en `window.ron`, en la carpeta de datos (también en modo portable). eframe la devuelve a un monitor conectado si el suyo ya no está y tiene en cuenta el escalado de cada pantalla. Solo se guarda la ventana, no el estado de los widgets de egui. eframe aplica `centered` después de la posición guardada, así que solo se activa cuando aún no existe `window.ron`. Coste: 547 KB (serde y ron).
- La sesión guarda también la carpeta del proyecto (`project`); si ya no existe, no se reabre. La tabla `[sidebar]` guarda si el panel del proyecto está visible, su ancho y las carpetas abiertas del árbol (rutas absolutas); las que el árbol ya no encuentra no se vuelven a guardar. Cada pestaña de vista previa lleva `preview = true`.
- La pestaña de vista previa es un indicador de `Tab` (`preview`). Mirar otro fichero crea una pestaña nueva en el sitio de la de vista previa (si no tiene cambios), así que la carga, el seguimiento del disco y la sesión no necesitan nada especial. Deja de serlo al detectarse cambios sin guardar (comprobado en cada `logic`), con doble clic, con Guardar como o al abrir el fichero de forma normal.
- Los recientes (`bracket-session`, `recent.rs`) van en `recent.toml`, aparte de la sesión porque son historial. Un único hilo escribe el fichero con la versión más nueva de la lista, así que dos cambios seguidos nunca escriben a la vez. Las entradas que ya no existen se ocultan al mostrar el menú y se quitan si fallan al abrirse.
- El recorrido del proyecto (`bracket-index`, `walk.rs`) usa la crate `ignore` de ripgrep con las mismas reglas para el índice completo y para listar una carpeta del árbol: `.gitignore` aunque no haya repositorio (`require_git(false)`), `.ignore`, ocultos incluidos y las carpetas de control de versiones fuera. Las exclusiones de `[project] exclude` entran como *overrides* negados (`!patrón`), porque un *override* sin negar significa «solo esto». El índice completo va en varios hilos (`build_parallel`) y mira el `CancelToken` en cada entrada; devuelve rutas relativas a la raíz ordenadas sin distinguir mayúsculas.
- El árbol del proyecto (`bracket-ui`, `tree.rs`) lista cada carpeta con `list_dir` en un hilo propio del árbol la primera vez que se abre; después vuelve a listar solo las carpetas ya listadas en las que la vigilancia del proyecto ha visto cambios (todas si cambia `.gitignore` o `.ignore`, o si se perdieron notificaciones). Si la carpeta no se puede vigilar, una carpeta se vuelve a listar cada vez que se reabre. Las filas visibles se reconstruyen solo cuando llega un listado o se abre o cierra una carpeta, y se pintan con `ScrollArea::show_rows`, así que el coste por frame no depende del tamaño del proyecto. Una carpeta dentro de otra cerrada sigue abierta para cuando se vuelva a abrir su padre. Si un patrón de `[project] exclude` no es válido, se avisa una vez en la barra de estado y el árbol sigue sin exclusiones. Cambiar de proyecto descarta el árbol y cancela los listados pendientes.
- Con las sondas de medición activas no se lee ni se escribe la sesión, para que las medidas no dependan de la sesión del usuario ni la cambien.

## Recursos de Windows (Fase 1)

- `crates/bracket-app/build.rs` genera un script de recursos y lo compila con `rc.exe` del Windows SDK (`embed-resource`); si no se puede, la compilación falla en vez de producir un exe sin manifiesto.
- **Manifiesto** (`res/bracket.manifest`): DPI por monitor (PerMonitorV2), rutas largas, Windows 10/11 como sistema compatible, controles comunes v6 (aspecto actual de los diálogos) y ejecución sin pedir permisos de administrador.
- **Versión:** `VERSIONINFO` con la versión de `Cargo.toml`, visible en las propiedades del fichero y en el Administrador de tareas.
- **Icono:** lo dibuja `build/icon.rs` por código (sin ficheros binarios en el repositorio): un cuadrado redondeado con `[|]`, muestreado 4×4 por píxel. El `.ico` lleva 9 tamaños (16 a 256; bitmaps hasta 48 y PNG desde 64, 32 KB en total). Una copia de 64×64 en RGBA es el icono de la ventana (`ViewportBuilder::with_icon`).

## Línea de comandos e instancia única (Fase 1)

- `bracket-app` (`cli.rs`) interpreta los argumentos; `OpenRequest::parse` (`bracket-session`) separa `fichero:línea:columna` (un `:número` final siempre es posición, porque los nombres de fichero de Windows no admiten `:` salvo tras la unidad).
- `--help` y `--version` salen antes de crear la ventana. Las versiones release son programas de interfaz gráfica sin consola propia: `console.rs` usa la salida redirigida si la hay, se engancha a la consola del proceso padre (`AttachConsole`) o, si no hay ninguna, muestra un mensaje. Es el segundo bloque `unsafe` del proyecto (`GetStdHandle` y `AttachConsole`); no hay alternativa segura en la biblioteca estándar.
- Instancia única sin dependencias nuevas (`instance.rs`): la primera instancia mantiene abierto `instance.lock` en la carpeta de datos sin compartirlo (`share_mode(0)`); Windows lo cierra al terminar el proceso, también si se cuelga, así que no puede quedar un bloqueo huérfano. Otra instancia escribe sus ficheros (rutas absolutas) en `requests/` y espera hasta 3 s a que desaparezca la petición; si nadie la recoge, la retira y abre su propia ventana.
- La instancia principal vigila `requests/` con el mismo `FileWatcher` que los ficheros abiertos, lee al arrancar las peticiones que hubiera y descarta las de más de 30 s.
- Todo lo que no pinta (sondeos de hilos, buzón, cambios en disco, sesión, petición de cierre) va en `App::logic`, que eframe llama antes de cada `ui` y también con la ventana minimizada u oculta. Así se atienden las peticiones con la ventana minimizada y se guarda la sesión al cerrarla desde la barra de tareas.
- eframe guarda una ventana diminuta en (0, 0) si guarda estando minimizada (al salir o en el autoguardado). `App::save` repone el último valor guardado con la ventana sin minimizar; el autoguardado es cada 5 s (eframe solo escribe si algo cambió).
- Abrir maximizada mostraba un fotograma negro: eframe pinta el primero con la ventana oculta y Windows lo descarta al maximizarla mientras aparece. El gancho `window_builder` de eframe crea la ventana sin maximizar pero con el tamaño maximizado que eframe guardó, y la app la maximiza en el segundo fotograma, ya en pantalla.
- Con las sondas de medición no hay instancia única: cada ejecución es independiente.

## Resaltado (Fase 1)

- Gramáticas: las de syntect (de sublimehq/Packages) para C#, CSS, JSON, XML y Rust, y dos propias en `crates/bracket-syntax/grammars/` para Razor y T-SQL (formato `.sublime-syntax` versión 1, la que entiende syntect; las de Sublime Text 4 con `extends` no sirven). Razor monta las construcciones de Razor sobre HTML con `with_prototype`, como la gramática «HTML (ASP)», e incrusta el contexto `code_block_in` de la gramática de C#.
- `build.rs` compila las gramáticas en un único paquete que se incrusta en el exe (344 KB comprimidos) y comprueba que ninguna referencia queda sin resolver. Se conserva el paquete completo de syntect (75 lenguajes): sus gramáticas se refieren unas a otras por posición y no se pueden sacar sueltas. El paquete se carga la primera vez que hace falta, en el hilo del resaltador (≈1 ms; syntect compila cada regex al usarla).
- Motor (`highlight.rs`, sin egui): guarda el estado del analizador cada 128 líneas. Tras una edición, los puntos de control por encima de la primera línea cambiada siguen valiendo. Si el número de líneas no cambió, los de debajo se conservan: cuando el análisis llega a uno con el mismo estado (y por debajo de la última línea cambiada), todo lo de debajo vuelve a valer.
- Cada pestaña resaltada tiene su hilo (`HighlightWorker`). La interfaz pide las líneas visibles más una página por arriba y otra por abajo. El hilo atiende solo la petición más reciente, abandona una larga si llega otra (conservando el trabajo hecho) y devuelve tramos de color por línea. Mientras llega el resultado nuevo, las líneas cuyo texto no cambió usan el anterior.
- La vista construye un `LayoutJob` de egui por línea con esos colores (egui guarda en caché las galeradas).

## Temas (Fase 1)

- Dos ajustes independientes: la apariencia de la interfaz (`Appearance`: según el sistema, clara u oscura, aplicada con `egui::Context::set_theme`, que también cambia la barra de título de Windows) y el tema del editor (`auto` o el nombre de uno de los siete temas de syntect).
- `follow_theme` (en `App::logic`) calcula el tema efectivo: el configurado o, en automático, el claro o el oscuro según `ctx.theme()`, así que sigue a Windows en directo. Cuando cambia, se descartan los hilos de resaltado, que se vuelven a crear con el tema nuevo.
- `bracket-syntax` da los colores del área del editor (`ThemeColors`: fondo, texto, cursor, selección, margen y números de línea) sin depender de egui; la interfaz usa los de la interfaz donde el tema no define uno (`EditorStyle`).

## Paquetes de lenguaje (Fase 1)

- `bracket-syntax` (`packages.rs`) lee `languages/<id>/language.toml` en las carpetas de `Paths::language_dirs` (prioridad: `%APPDATA%\Bracket\languages`, `languages` junto al exe; en portable solo esta). La primera carpeta con un id gana; un paquete con el id de un lenguaje integrado lo sustituye, y en las extensiones los paquetes van antes que los integrados.
- Los manifiestos se leen en `main` antes de crear la ventana (son pocos y pequeños), así que las pestañas tienen lenguaje desde el primer fotograma. De la gramática solo se lee su `scope:` de primer nivel.
- Las gramáticas externas se analizan en el hilo del resaltador la primera vez que se resalta algo, junto con las integradas (`into_builder` + `build`), así que pueden incluir o sustituir gramáticas integradas (para un scope gana la última añadida). Un `include` a una gramática que no existe se queda en texto sin color. Los errores se recogen y la interfaz los añade a sus avisos.
- Ningún problema de un paquete impide arrancar: el paquete se ignora con un aviso traducido (`PackageWarning`).

## Cambios en disco (Fase 1)

- `bracket-session` (`watch.rs`) envuelve `notify` (`ReadDirectoryChangesW`): vigila exactamente las rutas de las pestañas cargadas (se actualiza cuando cambian) y solo informa de qué ruta cambió.
- `ProjectWatcher` (también en `watch.rs`) vigila la carpeta del proyecto de forma recursiva. Solo cuenta creaciones, borrados y renombrados (en Windows, escribir en un fichero es `Modify(Any)` y se descarta) y agrupa los cambios en un hilo propio hasta que pasan 200 ms sin ninguno, o como mucho 1 s durante una compilación. Una notificación perdida pide volver a listar todo.
- `FileIndex::resync_dir` (`bracket-index`) actualiza el índice tras un cambio en una carpeta: compara los ficheros directos con un listado nuevo, recorre las subcarpetas nuevas y quita las desaparecidas. Las rutas con un mismo prefijo forman un bloque contiguo de la lista ordenada, así que solo se reescribe ese bloque. La app tendrá el índice con Ctrl+P (paso 9).
- `bracket-ui` (`disk.rs`) guarda por pestaña el sello del fichero (fecha de modificación y tamaño, `DiskStamp`) tomado al leerlo y después de cada guardado. Un aviso del vigilante programa una comprobación 200 ms después (los editores escriben, renombran y tocan el fichero seguidos); la comprobación corre en un hilo y solo lee el fichero si el sello cambió. Mientras Bracket guarda la pestaña, la comprobación espera, así que los guardados propios no se confunden con cambios externos.
- Los avisos se emparejan con las pestañas por nombre de fichero: el vigilante puede escribir la carpeta de otra forma (nombres largos en lugar de 8.3, otras mayúsculas) y una coincidencia de más solo cuesta comprobar un sello.
- Recargar usa `Document::reload`: sustituye el texto en un paso deshacible manteniendo línea y columna de cada cursor. Un fichero borrado deja el documento como modificado (`Document::mark_missing`), así que cerrar pregunta y la sesión guarda su texto.

## Fuentes (Fase 1)

- `bracket-app` (`system_fonts.rs`) encuentra fuentes instaladas por nombre de familia leyendo el registro de Windows (HKCU y luego HKLM) e implementa el trait `FontSource`.
- `bracket-ui` (`fonts.rs`) decide qué familias usar: la monoespaciada configurada (o Consolas, Cascadia Mono, Courier New) y Segoe UI para la interfaz. Se leen en un hilo que empieza al principio de `main` y termina mientras eframe inicializa OpenGL.
- La vista avisa de los caracteres visibles sin glifo (solo revisa los no ASCII, y deja de revisar un tipo cuando ya se pidió su respaldo) y la app carga en un hilo Segoe UI Symbol, una fuente CJK o Malgun Gothic.

## Prototipo de interfaz (`bracket-ui`, Fase 0)

- `EditorView` pinta solo las líneas visibles dentro de un `ScrollArea` de egui y traduce teclado, ratón, portapapeles e IME a órdenes del núcleo. La composición del IME se muestra en el cursor sin entrar en el buffer hasta que se confirma.
- Límite conocido: egui usa coordenadas `f32`. Con 1M de líneas (unos 17 millones de píxeles) la precisión es de unos 2 px al final del documento; por encima de unos pocos millones de líneas hará falta un scroll propio basado en líneas.
- Los textos visibles están en `locales/es.toml` y `locales/en.toml` y se piden con métodos de `i18n.rs` (ver [decisions/0002-i18n.md](decisions/0002-i18n.md)).

## Deuda técnica conocida

Pendiente de resolver; cada punto indica la fase prevista.

- **Memoria de las fuentes de respaldo:** egui ocupa unas dos veces el tamaño de cada fuente cargada; con texto japonés o chino en pantalla, Microsoft YaHei añade unos 42 MB. Alternativa si molesta: una fuente CJK más pequeña con menos cobertura (MS Gothic, 9 MB).
- **Sin fuentes del sistema:** si no se encontrara ninguna fuente (Consolas y Segoe UI vienen con Windows), la aplicación termina sin mostrar ningún mensaje.
- **Arranque (todas las fases):** 120–135 ms sin carga en la 0.1.0; unos 60 ms son la inicialización de OpenGL en Windows y ninguna opción de eframe lo reduce (ver [measurements.md](measurements.md)). Quedan unos 15–25 ms de margen: lo nuevo debe ir fuera del camino del primer frame, y `check.ps1 -Perf` vigila las regresiones.
- **Licencias en la distribución (Fase 5):** el aviso BSD-3-Clause de encoding_rs (y los de las demás dependencias) debe acompañar al instalador y al zip.
- **Guardado desde el diálogo de cambios sin guardar:** se hace en el hilo de interfaz y la bloquea mientras dura (normalmente milisegundos). Los hard links del fichero original no se conservan al guardar (es inherente al guardado atómico).
- **Unidades de texto (Fase 4):** los clústeres de grafemas no se tratan como unidad y los caracteres anchos (CJK) cuentan como una columna.
- **Ficheros grandes (Fase 4):** las líneas muy largas se maquetan enteras, y el `ScrollArea` de egui pierde precisión en `f32` por encima de ~1M de líneas (hará falta un scroll propio basado en líneas). Un fichero de 100 MB ocupa unos 200 MB de memoria.
- **Velocidad del resaltado:** unos 110 µs por línea con el motor de regex en Rust puro; se colorea primero lo visible, pero recorrer de golpe un fichero largo de arriba abajo deja colores pendientes unos instantes.
- **Paquete de gramáticas:** se incluye el paquete completo de syntect (344 KB comprimidos) porque sus gramáticas no se pueden separar; copiar al repositorio solo las fuentes necesarias ahorraría unos 300 KB.
- **Razor:** el HTML dentro de los bloques de código (`@{ <p>…</p> }`) no se reconoce y los bloques de `@if` se muestran como texto.

## Decisiones

Las decisiones de arquitectura se registran en [decisions/](decisions/).
