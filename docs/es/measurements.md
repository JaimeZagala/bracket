[English](../en/measurements.md) · **Español**

# Mediciones

Registro de las mediciones de rendimiento del proyecto, con el método para reproducirlas. Solo lo que aparece aquí puede citarse como ventaja en el README.

## Método

`scripts/measure.ps1 -Exe <ruta> -Runs 12` mide:

- **Tamaño:** bytes del ejecutable release (perfil `release` del workspace: LTO fat, `codegen-units = 1`, `panic = "abort"`, `strip`).
- **Arranque:** el script toma una marca de tiempo precisa (`GetSystemTimePreciseAsFileTime`) justo antes de lanzar el proceso. El ejecutable, con `BRACKET_MEASURE_STARTUP=<fichero>`, escribe su marca de entrada en `main()` y la del momento en que el primer frame ya está en pantalla (segunda llamada a `ui()`), y sale. Se informa de *lanzamiento → main* y *lanzamiento → primer frame*; el cierre del proceso no cuenta.
- **Primera ejecución:** se informa aparte. Tras compilar o reiniciar incluye el análisis de Windows Defender sobre el exe nuevo y cachés de disco frías (de 0,4 a 1,5 s en las pruebas). La mediana usa el resto de ejecuciones (en caliente).
- **Memoria en reposo:** tras 3 s con la ventana abierta. La cifra de referencia es el *espacio de trabajo privado* (columna "Memoria" del Administrador de tareas). El *working set* total incluye las DLL compartidas del driver gráfico.

Referencia: lanzar y cerrar un ejecutable Rust vacío cuesta unos 9 ms en este equipo.

## Equipo de prueba

Equipo A: Intel Core i7-13620H, 32 GB de RAM, GPU integrada Intel UHD Graphics, Windows 11 build 26200, alimentación conectada.

## Fase 0 — línea base de egui/eframe 0.36.2 (2026-10-15, equipo A)

Ventana con una etiqueta. Rust 1.99.0.

| Variante | Tamaño | Arranque en caliente (mediana) | Lanzamiento → main | Memoria privada |
|---|---|---|---|---|
| eframe `glow` + `default_fonts` | 4,95 MB | 143 ms | 17 ms | 28 MB |
| eframe `glow` + `default_fonts` + `accesskit` | 5,23 MB | 144 ms | 18 ms | — |
| eframe por defecto (`wgpu`) | 9,73 MB | 264 ms | 17 ms | 244 MB |
| **`bracket.exe` 0.0.0** (`glow` + `default_fonts` + `accesskit`, ventana 1000×700) | **5,23 MB** | **139 ms** | 16 ms | **40 MB** |

Conclusiones:

- glow frente a wgpu: la mitad de tamaño, unos 120 ms menos de arranque y una sexta parte de memoria en esta GPU. Se usa glow.
- `accesskit` (accesibilidad para lectores de pantalla) cuesta 0,3 MB y nada medible en arranque. Se incluye.
- El arranque de una ventana vacía ya ronda el presupuesto de 150 ms: unos 125 ms son la creación de la ventana, el contexto OpenGL y el primer frame dentro de eframe. Queda muy poco margen para el resto de la aplicación.

## Fase 0 — prototipo del editor (2026-10-15, equipo A)

`bracket.exe` 0.0.0 con el prototipo del editor (commit `feat(ui): add editor prototype`). Ficheros generados con `cargo run --release -p bracket-core --example generate_large_file -- <líneas> <ruta>` (CRLF, tabulaciones, texto no ASCII).

| Caso | Tamaño | Lanzamiento → contenido visible (mediana en caliente) | Memoria privada |
|---|---|---|---|
| Sin fichero | 5,53 MB | 143 ms | 28 MB |
| 1.000.000 de líneas (52,6 MB) | 5,53 MB | 263 ms | 88 MB |
| 2.000.000 de líneas (100,2 MB) | 5,53 MB | 363 ms | 146 MB |

La memoria en reposo varía unos ±10 MB entre ejecuciones (la ventana vacía midió 40 MB y luego 28 MB).

### Benchmarks (criterion, `cargo bench --workspace`)

| Benchmark | Tiempo (mediana) |
|---|---|
| `open/rope_from_str_1m_lines`: construir el rope de 1M líneas desde memoria | 23 ms |
| `open/load_file_1m_lines`: leer del disco, validar UTF-8 y crear el documento | 50 ms |
| `insert_char_middle_1m_lines`: insertar un carácter en medio | 1,6 µs |
| `frame_1000x700_middle_of_1m_lines`: frame completo sin cambios (galleys en caché) | 91 µs |
| `frame_1000x700_scrolling_page_down_1m_lines`: frame con una página nueva de líneas | 393 µs |

El tiempo de frame es solo CPU (maquetación, pintado y teselado de egui); no incluye la GPU ni la presentación.

Conclusiones:

- El fichero de 100 MB queda visible en 0,36 s, por debajo del presupuesto de 1 s, sin bloquear la interfaz (la carga va en un hilo de trabajo).
- Un frame con scroll cuesta 0,4 ms de CPU: hay margen de sobra para 60 Hz.

## Fase 0 — comparación de frameworks de interfaz (2026-10-15, equipo A)

Prototipo de iced 0.14.0 (variantes wgpu y tiny-skia) frente a `bracket.exe`; su código quedó en el historial (commit `61c1712`). Tabla completa y análisis en [decisions/0001-ui.md](decisions/0001-ui.md).

Scroll (`./scripts/measure.ps1 -Exe <exe> -Arguments target\large-1m.txt -Scroll [-Maximized]`, ejecuciones intercaladas): una página por frame durante 300 frames.

| | CPU por frame, 1000×700 | CPU por frame, maximizada | fps maximizada |
|---|---|---|---|
| egui + glow (`bracket.exe`) | 3,8 ms | 4,2 ms | ~100 (vsync) |
| iced + wgpu | 3,3 ms | 3,4 ms | ~100 (vsync) |
| iced + tiny-skia | 8,6 ms | 14,8 ms | 66 (sin vsync) |

Nota: durante estas pruebas el equipo tenía carga de fondo variable (12–43 % de CPU); los arranques medidos en rondas intercaladas salen más lentos que los de las tablas anteriores, tomadas en un momento tranquilo.

## Fase 1, paso 1 — análisis del arranque (2026-10-15, equipo A, carga de fondo < 10 %)

`measure.ps1` desglosa ahora el arranque con marcas de la propia aplicación (app creada y primera llamada a `ui()`):

| Fase | Mediana en caliente |
|---|---|
| Lanzamiento → `main` | 12–14 ms |
| `main` → app creada (bucle de eventos, ventana, WGL, contexto OpenGL, pintor de egui) | 74–79 ms |
| App creada → primera llamada a `ui()` | 1,5 ms |
| Primera llamada a `ui()` → primer frame presentado | 35–38 ms |
| **Total** | **123–133 ms** |

Detalle de "`main` → app creada", según los mensajes de log de eframe con marca de tiempo:

| Tramo | Tiempo |
|---|---|
| Bucle de eventos de winit | ~5,5 ms |
| Ventana + inicialización de WGL (carga de `opengl32` y del driver, elección del formato de píxel) | ~60 ms |
| Contexto OpenGL y hacerlo actual | ~8 ms |
| Pintor de egui (versión de OpenGL, shaders, VAO) | ~5 ms |

Opciones probadas en rondas intercaladas (12 rondas; ruido entre variantes idénticas: ±8 ms): `vsync = false`, `dithering = false`, aceleración por hardware obligatoria, sin arrastrar y soltar, sin fuentes de emoji y solo la fuente Hack. Ninguna cambia el arranque más allá del ruido.

Conclusiones:

- La mayor parte del arranque es la inicialización de OpenGL en Windows (driver incluido), fuera del alcance de la aplicación. Con la máquina tranquila el arranque cumple el presupuesto (123–133 ms < 150 ms) con un margen de unos 20 ms.
- `./scripts/check.ps1 -Perf` compila en release y falla si la mediana en caliente supera 150 ms. No corre en la integración continua (los runners no tienen GPU) y conviene ejecutarlo con poca carga de fondo.
- Todo lo que se añada en las fases siguientes debe quedar fuera del camino del primer frame (en hilos o diferido).

## Fase 1 — evolución del tamaño del ejecutable (equipo A)

| Paso | Tamaño | Cambio | Motivo |
|---|---|---|---|
| Fin de la Fase 0 | 5.802.496 B | — | |
| Paso 2: configuración TOML | 5.971.456 B | +165 KB | `toml_edit` y el código de configuración |
| Paso 3: traducciones y selector de idioma | 6.099.456 B | +128 KB | Catálogos y menú de idioma. El mecanismo de catálogos mide +7,5 KB aislado (ver [decisions/0002-i18n.md](decisions/0002-i18n.md)); el resto debe de ser el código de menús de egui, enlazado por primera vez (no se midió por separado) |
| Paso 4: fuentes del sistema | 4.736.000 B | −1.363 KB | Sin las fuentes integradas de egui (`default_fonts`); entra `winreg` |
| Paso 5: codificaciones | 5.165.568 B | +430 KB | `encoding_rs` (tablas de todas las codificaciones WHATWG) y `chardetng` |
| Paso 6: diálogos de ficheros | 5.216.256 B | +50 KB | `rfd` y el menú Archivo. Arranque intercalado frente al paso 5: 135,6 frente a 134,3 ms |
| Paso 7: guardado con `ReplaceFileW` | 5.228.032 B | +12 KB | `windows-sys` (ya en el árbol). Arranque intercalado frente al paso 6 con el equipo muy cargado: 289 frente a 312 ms (sin regresión) |
| Paso 8: pestañas y registro de lenguajes | 5.266.944 B | +39 KB | Barra de pestañas, registro de lenguajes y varios ficheros por argumento. Arranque intercalado frente al paso 7 (15 rondas, equipo cargado): 236,2 frente a 238,6 ms (sin regresión) |
| Paso 9: sesión recuperable | 5.323.776 B | +57 KB | Fichero de sesión y copias, restauración de pestañas. Arranque intercalado frente al paso 8 (15 rondas, equipo tranquilo): 120,2 frente a 127,0 ms. Leer una sesión de 10 pestañas cuesta 30 µs (`cargo bench -p bracket-session`); las sondas no la leen |
| Paso 9: posición de la ventana | 5.870.592 B | +547 KB | Feature `persistence` de eframe (serde y ron). Arranque intercalado frente al paso 9 sin ella (15 rondas): 126,8 frente a 137,4 ms. Con las sondas no se lee ni escribe `window.ron` |
| Paso 10: recarga por cambios en disco | 5.992.960 B | +120 KB | `notify` (y `walkdir`; `windows-sys` 0.60 ya estaba en el árbol) y el seguimiento de ficheros. Arranque intercalado frente al paso 9 (15 rondas): 119,6 frente a 133,6 ms |
| Paso 11: instancia única y línea de comandos | 6.055.424 B | +61 KB | Argumentos, ayuda en catálogos, mensaje de `rfd`, bloqueo y buzón. Arranque intercalado frente al paso 10 (15 rondas): 133,1 frente a 131,0 ms (ruido). Entregar ficheros a la instancia abierta: la segunda termina en 110–230 ms |
| Paso 11: carga diferida de la sesión | — | — | Sesión con una pestaña pequeña activa y dos inactivas de 100 y 52 MB, memoria privada a los 3 s: 226 MB antes, 47 MB con carga diferida (221 MB al activar la de 100 MB) |
| Paso 12: resaltado con syntect | 8.043.520 B | +1.988 KB | Paquete de gramáticas (344 KB comprimidos) y código: motor de regex en Rust puro (`fancy-regex`, `regex-automata` con tablas Unicode), analizador de syntect, descompresión y deserialización. Arranque intercalado frente al paso 11 (15 rondas): 122,1 frente a 125,1 ms (las gramáticas se cargan en el hilo del resaltador) |
| Paso 13: paquetes de lenguaje | 8.300.544 B | +257 KB | Lector YAML de syntect (`yaml-load`) para las gramáticas de los paquetes, y el cargador. Arranque intercalado frente al paso 12 (15 rondas, equipo cargado): 165,5 frente a 164,3 ms |
| Paso 14: temas | 8.369.664 B | +69 KB | Menú Ver, apariencia y colores del editor (los temas ya iban en el exe). Arranque intercalado frente al paso 13 (15 rondas): 139,5 frente a 142,2 ms |
| Paso 15: barra de estado completa | 8.400.896 B | +31 KB | Lenguaje del documento, selecciones y nueva disposición. Arranque intercalado frente al paso 14 (15 rondas): 135,4 frente a 141,9 ms |
| Paso 16: manifiesto, icono y versión | 8.454.144 B | +53 KB | Icono (32 KB), icono de la ventana en RGBA (16 KB), manifiesto y versión. Arranque intercalado frente al paso 15 (15 rondas): 120,3 frente a 131,7 ms; «primer `ui()` → fotograma» baja de 33,5 a 25,8 ms (probablemente por declarar el DPI en el manifiesto) |

Paso 5, arranque intercalado frente al paso 4 (15 rondas): 144,9 ms frente a 148,6 ms, dentro del ruido. Ese día las medianas absolutas rondaron 145–149 ms con el equipo cargado: el margen frente a los 150 ms es mínimo.

El arranque en caliente se mantiene en 123–134 ms en estos pasos (dentro del ruido de ±8 ms).

### Paso 4 — memoria según el texto abierto

| Texto | Memoria privada |
|---|---|
| Solo ASCII | 30,7 MB |
| Japonés (carga Microsoft YaHei, 19,7 MB) | 72,7 MB |
| Japonés, coreano y símbolos (YaHei + Malgun Gothic + Segoe UI Symbol) | 103,3 MB |

egui ocupa unas dos veces el tamaño de cada fuente cargada. Las fuentes de respaldo solo se cargan cuando ese texto aparece en pantalla.

Arranque, comparado de forma intercalada con el exe del paso 3 (`scripts/compare-startup.ps1`, 20 rondas, carga de fondo entre el 3 y el 29 %): paso 3 145,2 ms, paso 4 139,7 ms. Sin regresión; las mediciones sueltas de este día oscilaron entre 130 y 156 ms según la carga del equipo, por eso las regresiones se comprueban con ejecuciones intercaladas.

Benchmarks de pintado tras el cambio de fuente (Consolas en lugar de Hack): frame estable 83 µs, frame con scroll 420 µs. Una primera versión de la detección de glifos ausentes subió el frame con scroll a 2 ms al preguntar en cada frame por caracteres sin fuente; ahora solo se revisan caracteres no ASCII y se deja de revisar un tipo cuando ya se pidió su respaldo.

### Paso 12: benchmarks de resaltado (`cargo bench -p bracket-syntax`)

| Benchmark | Tiempo (mediana) |
|---|---|
| Primera pasada sobre 10.000 líneas de C# | 1,12 s (≈110 µs por línea) |
| Página en la línea 5.000 tras escribir en un comentario de la línea 12 (se reutilizan los puntos de control) | 27 ms (547 ms sin reutilizarlos) |
| Página en la línea 5.000 ya resaltada (desde el punto de control más cercano) | 6,8 ms |

El coste por línea es el del motor de regex en Rust puro; como se resalta primero lo visible, la primera pantalla de un fichero se colorea en unos 15 ms.

## Fase 1 — cierre, 0.1.0 (2026-10-18, equipo A)

`./scripts/measure.ps1` (desde una copia portable temporal, sin la sesión del usuario). Presupuestos del plan:

| Medida | Presupuesto | 0.1.0 | Cómo |
|---|---|---|---|
| Tamaño del ejecutable | < 15 MB | 8.454.144 B (8,06 MB) | Release con LTO |
| Arranque hasta el primer fotograma | < 150 ms | 126,1 ms (mediana en caliente de 11; mínimo 119,7) | `-Runs 12`, sin fichero |
| Memoria en reposo, sin ficheros | — | 30,4 MB | Conjunto de trabajo privado a los 3 s |
| Memoria con 10 pestañas | < 80 MB | 33,6 MB (activa: JSON resaltado); 49,2 MB si la activa carga la fuente de símbolos | 10 ficheros (93 KB): C#, Razor, T-SQL, JSON, XML, CSS, Markdown, TOML y Rust; a los 5 s |
| Fichero de 100 MB visible | < 1 s | 334 ms (mediana en caliente de 5) | `target\large-2m.txt`; 200 MB de memoria |
| Ctrl+P con 50.000 ficheros | < 50 ms | — | Fase 2 |

La primera ejecución de cada medición (1,2–1,5 s) es la de un exe recién copiado: incluye el análisis de Windows Defender y no representa el uso normal.

## Fase 2, paso 4 — recorrido del proyecto (2026-10-19, equipo A)

`cargo bench -p bracket-index` sobre `target/project-50k` (`cargo run --release -p bracket-index --example generate_project`): 50.000 ficheros en 50 × 20 carpetas, más 5.000 en `node_modules/`, que excluye `.gitignore`.

| Benchmark | Tiempo (mediana) |
|---|---|
| `walk/index_50k_files`: índice completo, varios hilos | 110,5 ms |
| `walk/list_root`: listar la raíz para el árbol | 1,53 ms |

En caliente (la caché de ficheros de Windows ya tiene las carpetas). Generar el proyecto tarda unos 26 s, casi todo por Windows Defender.

## Fase 2, paso 5 — panel del proyecto (2026-10-19, equipo A)

| Medida | Paso 4 | Paso 5 | Cambio |
|---|---|---|---|
| Tamaño del ejecutable | 8.521.216 B | 8.795.648 B | +268 KB: `ignore` (con globset y crossbeam) y el árbol |
| Arranque (`compare-startup.ps1`, 15 rondas intercaladas, mediana) | 132,2 ms | 131,4 ms | Ruido |

El árbol solo pinta las filas visibles (`ScrollArea::show_rows`) y reconstruye la lista de filas cuando cambia, así que el coste por frame no depende del tamaño del proyecto. Listar una carpeta cuesta lo mismo que `walk/list_root` del paso 4 (1,5 ms con 50 carpetas).

## Fase 2, paso 7 — vigilancia del proyecto (2026-10-19, equipo A)

| Medida | Resultado |
|---|---|
| `walk/resync_root`: actualizar el índice tras un cambio en la raíz (50.000 ficheros) | 9,9 ms (recorrer el proyecto entero: 109 ms) |
| `walk/resync_deep_folder`: tras un cambio en `src07\module03` | 0,66 ms |
| Árbol al día tras crear una carpeta y dos ficheros, renombrar uno y borrar otro desde fuera | < 1,5 s (antirrebote de 200 ms) |
| Tamaño del ejecutable | 8.837.632 B (+37,5 KB frente al paso 6, con Rust incluido: Rust no añade nada) |

## Fase 2, paso 8 — acciones del árbol (2026-10-19, equipo A)

| Medida | Paso 7 | Paso 8 | Cambio |
|---|---|---|---|
| Tamaño del ejecutable | 8.837.632 B | 9.058.816 B | +216 KB: `trash` (con las interfaces COM de la crate `windows`) y las acciones |
| Arranque (`compare-startup.ps1`, 15 rondas intercaladas, mediana) | 117,1 ms | 121,8 ms | Ruido (±8 ms entre variantes idénticas) |

## Fase 2, paso 9 — Ctrl+P (2026-10-19, equipo A)

`cargo bench -p bracket-index --bench fuzzy`: una consulta sobre los 50.000 ficheros de `target/project-50k`, 200 resultados con posiciones. Presupuesto: 50 ms por pulsación.

| Consulta | Un hilo | Hasta 8 hilos |
|---|---|---|
| `f` (casi todo coincide) | 20,8 ms | 6,2 ms |
| `file` | 24,6 ms | 6,8 ms |
| `fl12cs` | 2,1 ms | 0,90 ms |
| `src07/module03/file12` | 1,95 ms | 0,73 ms |
| `zzz` (nada coincide) | 1,88 ms | 0,72 ms |

| Medida | Paso 8 | Paso 9 | Cambio |
|---|---|---|---|
| Tamaño del ejecutable | 9.058.816 B | 9.251.328 B | +188 KB: el recorrido en paralelo de `ignore` (ya no solo el listado de una carpeta), el emparejador y la paleta |
| Arranque (15 rondas intercaladas, mediana) | 118,9 ms | 122,6 ms | Ruido |
| Bytes privados a los 6 s con `target/project-50k` abierto | 47,1 MB | 54,3 MB | +7,2 MB: el índice de 50.000 rutas |

## Fase 2, paso 10 — paleta de comandos (2026-10-19, equipo A)

Tamaño del ejecutable: 9.291.264 B (+39 KB frente al paso 9: el registro, el modo comandos de la paleta y el menú Ir).

## Fase 2, paso 11 — buscar en el fichero (2026-10-19, equipo A)

`cargo bench -p bracket-search`: un documento de 50 MB con una línea de cada 100 que contiene «Customer».

| Benchmark | Tiempo (mediana) |
|---|---|
| `find/rope_to_string_50mb`: copiar la cuerda a un texto, en el hilo | 25,5 ms |
| `find/plain_50mb`: «customer» sin distinguir mayúsculas | 8,8 ms |
| `find/whole_word_50mb`: lo mismo, palabra completa | 8,1 ms |
| `find/regex_50mb`: `item\d+5\.Price` | 90,5 ms |

| Medida | Paso 10 | Paso 11 | Cambio |
|---|---|---|---|
| Tamaño del ejecutable | 9.291.264 B | 9.373.184 B | +80 KB: `regex` y la barra de búsqueda |
| Arranque (15 rondas intercaladas, mediana) | 140,3 ms | 141,9 ms | Ruido (el equipo iba cargado en las dos) |

## Fase 2, paso 12 — reemplazar en el fichero (2026-10-19, equipo A)

Tamaño del ejecutable: 9.398.784 B (+25 KB frente al paso 11).

`find/replace_all_regex_50mb` (cada `new Customer(…)` con su reemplazo `Customer.Create($1)`, unas 10.000 coincidencias en 50 MB): 79,8 ms. En esta sesión el equipo iba unas 7 veces más lento que en el paso 11 (en la misma ejecución, `find/plain_50mb` dio 76 ms frente a 8,8 ms y una compilación release tardó 6,5 min en lugar de 1,5), así que la cifra es pesimista y se repetirá en el cierre de la fase (paso 15). El arranque no se midió por lo mismo.

## Fase 2, paso 13 — buscar en la carpeta (2026-10-19, equipo A)

| Medida | Resultado |
|---|---|
| `folder/search_50k_files`: «x» en los 50.000 ficheros de `target/project-50k` (todos coinciden), con la caché de ficheros caliente | 765 ms (de ellos, unos 120 ms el recorrido; referencia en la misma sesión: `walk/index_50k_files` 118 ms) |
| Tamaño del ejecutable | 9.461.760 B (+61 KB frente al paso 12) |
| Arranque (15 rondas intercaladas, mediana, frente al paso 11) | 146,6 → 148,0 ms: ruido |

Los resultados llegan fichero a fichero, así que los primeros se ven mucho antes de que termine.

## Fase 2, paso 14 — reemplazar en la carpeta (2026-10-19, equipo A)

Tamaño del ejecutable: 9.523.200 B (+60 KB frente al paso 13: el reemplazo en ficheros, la vista previa, las casillas y la confirmación).

## Fase 2 — cierre, 0.2.0 (2026-10-19, equipo A)

`./scripts/measure.ps1` (copia portable temporal, sin la sesión del usuario) y `cargo bench`. Presupuestos del plan:

| Medida | Presupuesto | 0.1.0 | 0.2.0 | Cómo |
|---|---|---|---|---|
| Tamaño del ejecutable | < 15 MB | 8.454.144 B | 9.523.712 B (9,08 MB) | Release con LTO |
| Arranque hasta el primer fotograma | < 150 ms | 126,1 ms | 123,5 ms (mínimo 115,2) | `-Runs 12`, sin fichero |
| Arranque abriendo un proyecto | < 150 ms | — | 129,6 ms (mínimo 120,4) | `-Runs 15 -Arguments target\project-50k` |
| Memoria en reposo, sin ficheros | — | 30,4 MB | 30,2 MB | Conjunto de trabajo privado a los 3 s |
| Memoria con 10 pestañas | < 80 MB | 33,6 MB | 34,0 MB | 10 ficheros (48 KB): C#, Razor, T-SQL, XML, CSS, Markdown, TOML, Rust, texto y JSON (activa); a los 5 s |
| Memoria con el proyecto de 50.000 ficheros | — | — | 37,1 MB (bytes privados 53,7 MB) | Árbol e índice construidos; a los 5 s |
| Fichero de 100 MB visible | < 1 s | 334 ms | No medido de nuevo | La Fase 2 no cambió la carga |
| Ctrl+P con 50.000 ficheros | < 50 ms | — | 7,0 ms (peor consulta, `file`) | `fuzzy/word` |

Benchmarks del proyecto (`target/project-50k`, caché de ficheros caliente):

| Benchmark | Tiempo (mediana) |
|---|---|
| `walk/index_50k_files`: índice completo | 121,1 ms |
| `walk/resync_root` / `walk/resync_deep_folder`: actualizar el índice tras un cambio | 10,5 ms / 0,76 ms |
| `fuzzy/one_letter` … `fuzzy/no_match`: Ctrl+P | 0,74–7,0 ms |
| `folder/search_50k_files`: buscar en los 50.000 ficheros (todos coinciden) | 940 ms |
| `find/plain_50mb` / `find/regex_50mb`: Ctrl+F en 50 MB | 10,3 ms / 100,3 ms |
| `find/replace_all_regex_50mb`: «Reemplazar todo» con grupos en 50 MB | 14,2 ms (repite la medida pesimista del paso 12) |

Benchmarks de la Fase 1, sin regresión: `open/rope_from_str_1m_lines` 24,2 ms (antes 23), `open/load_file_1m_lines` 45,2 ms (antes 50), `insert_char_middle_1m_lines` 1,2 µs (antes 1,6). El pintado del viewport, comparado el mismo día con la 0.1.0 compilada aparte: frame sin cambios 110,6 µs frente a 102,5 µs y frame con scroll 542,6 µs frente a 505,4 µs (+7–8 %, del orden del ruido entre compilaciones). Una primera tanda de benchmarks, lanzada seguida de otras, dio cifras 3–4 veces peores; repetida a solas, no se reprodujo.

Durante el cierre, abrir el proyecto de 50.000 ficheros al arrancar subía el arranque a 151,6 ms: el recorrido del índice en todos los núcleos competía con el primer fotograma. Ahora el índice empieza después del primer fotograma (commit `perf(ui): start the file index after the first frame`) y el arranque con proyecto queda en 129,6 ms.

La primera ejecución de cada medición (1,2–2,0 s) es la de un exe recién copiado: incluye el análisis de Windows Defender.

## Fase 3, paso 1 — tree-sitter (2026-10-19, equipo A)

Programa mínimo con el perfil release del workspace (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`), que analiza un texto con cada gramática:

| Variante | Tamaño | Añadido |
|---|---|---|
| Sin tree-sitter | 118.784 B | — |
| tree-sitter 0.27.1 (sin gramáticas en uso) | 130.048 B | +11 KB |
| + C# 0.23.5 | 5.545.984 B | +5,42 MB |
| + CSS 0.25.0, JSON 0.24.8 y XML 0.7.0 | 5.711.360 B | +0,17 MB (total: +5,59 MB) |
| Todas, con las gramáticas en `opt-level = "s"` o `"z"` | 5.707.264 B | −4 KB |

El tamaño de la gramática de C# es casi todo datos: `ts_parse_table` es una tabla densa de 4.096 estados × 530 símbolos × 2 bytes (4,3 MB) en `parser.c` (29,7 MB de código fuente generado). Ninguna opción del compilador la reduce; habría que regenerar la gramática. Se mantiene la optimización por defecto.

Velocidad de análisis de C# (un fichero sintético de 1,5 MB, 4.000 clases): 287 ms, 5,3 MB/s, 508.008 nodos; igual con `opt-level = "s"` o `"z"`.

## Fase 3, paso 2 — símbolos de C# (2026-10-19, equipo A)

`cargo bench -p bracket-index --bench symbols`: un fichero C# sintético de 1,47 MB (4.000 clases con campo, propiedad y métodos; 20.001 símbolos).

| Benchmark | Tiempo (mediana) | Velocidad |
|---|---|---|
| `symbols/csharp_1_5mb`: analizar y extraer los símbolos | 414 ms | 3,4 MiB/s |

Desglose medido en release: análisis de tree-sitter 297 ms, consulta 96 ms, construcción de los símbolos (nombres, posiciones en caracteres y anidamiento) 4 ms. Una primera ejecución dio 1,38 s con el equipo cargado (la referencia `walk/index_50k_files` también iba más lenta: 139 ms frente a 121); repetida, 414 ms. El índice del proyecto (paso 7) analizará los ficheros en paralelo y el esquema (paso 5) de forma incremental.

## Fase 3, paso 5 — esquema, tree-sitter en el exe (2026-10-19, equipo A)

| Medida | 0.2.0 | Paso 5 | Cambio |
|---|---|---|---|
| Tamaño del ejecutable | 9.523.712 B (9,08 MB) | 15.282.176 B (14,57 MB) | +5,49 MB: tree-sitter con las gramáticas de C#, CSS, JSON y XML, las consultas, las reglas y el esquema |
| Arranque (`compare-startup.ps1`, 15 rondas intercaladas, mediana) | 168,1 ms | 169,9 ms | Ruido; las dos cifras son altas porque el equipo iba cargado |

El presupuesto es de 15 MB (como en el resto de mediciones, 1 MB = 2²⁰ bytes: 15.728.640 B): quedan 446.464 B (436 KB) de margen para las fases siguientes.
