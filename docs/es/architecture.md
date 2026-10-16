[English](../en/architecture.md) · **Español**

# Arquitectura

Estado: Fase 0. Este documento describe la estructura acordada; cada sección indica qué está implementado.

## Crates del workspace

| Crate | Responsabilidad | Depende de (workspace) | Estado |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), selecciones múltiples, deshacer/rehacer por transacciones, movimiento de cursor, codificación, saltos de línea, modelo de documento. Sin UI ni APIs de Windows. | — | Fase 0: buffer, selecciones, transacciones, historial, movimiento y documento UTF-8 |
| `bracket-syntax` | Carga de lenguajes integrados y externos, resaltado incremental por líneas con caché, temas. | `bracket-core` (previsto) | Fase 1: registro de lenguajes (los seis integrados, extensiones e insignia) |
| `bracket-index` | Recorrido de proyecto, extracción de símbolos (tree-sitter y regex), índice incremental, búsqueda difusa, referencias por nombre. | `bracket-core` (previsto) | Vacía |
| `bracket-search` | Buscar y reemplazar con regex en fichero y carpeta, cancelable, con resultados en streaming. | `bracket-core` (previsto) | Vacía |
| `bracket-session` | Configuración TOML, modo portable, sesión recuperable, recientes, vigilancia de ficheros, registro de comandos. | `bracket-core` | Fase 1: rutas (modo portable o `%APPDATA%`), configuración TOML y sesión recuperable |
| `bracket-update` | Comprobación de versiones por canal, descarga, verificación de suma y firma, aplicación y vuelta atrás. | — | Vacía |
| `bracket-ui` | Interfaz egui: vista del editor, pestañas, árbol de proyecto, paleta, barra de estado. | `bracket-core`, `bracket-session`, `bracket-syntax` | Fase 1: pestañas, menú Archivo, barra de estado, fuentes del sistema, catálogos de textos |
| `bracket-app` | Binario `bracket.exe`: arranque, instancia única, argumentos, manifiesto de Windows, registro en HKCU. | `bracket-ui`, `bracket-session`, `bracket-update` | Fase 0: arranque y fichero por argumento. Fase 1: resuelve rutas y carga la configuración |

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

En la Fase 0, la carga y el guardado de ficheros usan `std::thread` y `std::sync::mpsc`; los tokens de cancelación y las generaciones llegan con las primeras tareas cancelables (Fase 2).

## Modelo del núcleo (`bracket-core`)

- **Posiciones:** índices de carácter (valores escalares Unicode), la unidad nativa de ropey 1.x. No puede haber posiciones a mitad de un carácter UTF-8. tree-sitter y regex trabajan en bytes; la conversión (O(log n)) se hará en el borde.
- **Saltos de línea:** solo LF, CRLF y CR parten líneas, como en Notepad++ y VS Code (U+2028 no). El texto nunca se normaliza; el salto de línea del documento (el primero que aparece, CRLF para ficheros nuevos) solo se usa para el texto nuevo: Intro y pegado.
- **CRLF como unidad:** el cursor nunca se queda entre `\r` y `\n`, y Retroceso/Supr borran el par completo. Los clústeres de grafemas (p. ej. `e` + acento combinante) todavía no se tratan como unidad.
- **Selecciones:** conjunto no vacío, ordenado y sin solapes, con una selección principal. Cada selección tiene ancla, cabeza y columna objetivo para moverse en vertical.
- **Edición:** toda edición se planifica por selección contra el texto anterior y se aplica como una sola transacción (lista de cambios con el texto borrado, invertible).
- **Deshacer:** escribir caracteres seguidos o borrar carácter a carácter forma un solo paso; pegar, Intro, borrar una selección y las ediciones multicursor son pasos propios. Mover el cursor corta el grupo.
- **Estado modificado:** cada paso del historial tiene un identificador único y la versión del buffer es la del último paso aplicado; deshacer y rehacer recuperan identificadores anteriores, así que volver al texto guardado lo deja como no modificado.
- **Reemplazo de ficheros:** el núcleo define el trait `FileReplacer` (por defecto, renombrar). `bracket-app` usa `ReplaceFileW` (`file_replace.rs`), que conserva atributos, ACL y fecha de creación. Es el único bloque `unsafe` del proyecto, con su comentario `SAFETY`.
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
- `bracket-app` lee la sesión al arrancar (30 µs con 10 pestañas).
- La ventana (posición, tamaño, maximizada) la guarda eframe con su feature `persistence` en `window.ron`, en la carpeta de datos (también en modo portable). eframe la devuelve a un monitor conectado si el suyo ya no está y tiene en cuenta el escalado de cada pantalla. Solo se guarda la ventana, no el estado de los widgets de egui. eframe aplica `centered` después de la posición guardada, así que solo se activa cuando aún no existe `window.ron`. Coste: 547 KB (serde y ron).
- Con las sondas de medición activas no se lee ni se escribe la sesión, para que las medidas no dependan de la sesión del usuario ni la cambien.

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
- **Arranque (todas las fases):** 123–133 ms sin carga; unos 60 ms son la inicialización de OpenGL en Windows y ninguna opción de eframe lo reduce (ver [measurements.md](measurements.md)). Quedan unos 20 ms de margen: lo nuevo debe ir fuera del camino del primer frame, y `check.ps1 -Perf` vigila las regresiones.
- **Licencias en la distribución (Fase 5):** el aviso BSD-3-Clause de encoding_rs (y los de las demás dependencias) debe acompañar al instalador y al zip.
- **Guardado desde el diálogo de cambios sin guardar:** se hace en el hilo de interfaz y la bloquea mientras dura (normalmente milisegundos). Los hard links del fichero original no se conservan al guardar (es inherente al guardado atómico).
- **Unidades de texto (Fase 4):** los clústeres de grafemas no se tratan como unidad y los caracteres anchos (CJK) cuentan como una columna.
- **Ficheros grandes (Fase 4):** las líneas muy largas se maquetan enteras, y el `ScrollArea` de egui pierde precisión en `f32` por encima de ~1M de líneas (hará falta un scroll propio basado en líneas).

## Decisiones

Las decisiones de arquitectura se registran en [decisions/](decisions/).
