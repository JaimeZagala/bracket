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
