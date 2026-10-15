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
