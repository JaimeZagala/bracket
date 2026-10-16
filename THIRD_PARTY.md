[English](THIRD_PARTY.en.md) · **Español**

# Componentes de terceros

Dependencias y recursos de terceros incluidos en el ejecutable de Bracket, con su origen y licencia. Se actualiza en el mismo commit que añade, cambia o quita una dependencia.

Cada dependencia nueva se justifica antes de entrar: mantenimiento activo, licencia compatible con MIT/Apache-2.0 e impacto medido en tamaño del ejecutable y arranque.

## Crates de Rust

Solo las dependencias directas; las transitivas se revisarán con una herramienta de licencias antes de la 1.0.0.

| Crate | Versión | Licencia | Uso | Impacto medido |
|---|---|---|---|---|
| [ropey](https://crates.io/crates/ropey) | 1.6.1 | MIT | Buffer de texto (rope) en `bracket-core` | +0,3 MB junto con el núcleo y la vista del editor ([mediciones](docs/es/measurements.md)) |
| [eframe](https://crates.io/crates/eframe) (incluye egui) | 0.36.2 | MIT OR Apache-2.0 | Interfaz gráfica (backend glow, sin fuentes integradas) en `bracket-ui` | Exe de 0,10 a 5,23 MB; arranque de 9 a 139 ms ([mediciones](docs/es/measurements.md)); sin `default_fonts` el exe baja 1,36 MB |
| Feature `persistence` de eframe: [serde](https://crates.io/crates/serde) 1.0.229 y [ron](https://crates.io/crates/ron) 0.12.2 | — | MIT OR Apache-2.0 | Guardar la posición y el tamaño de la ventana (`window.ron`) y devolverla a un monitor conectado | +547 KB; sin efecto medible en el arranque |
| [toml_edit](https://crates.io/crates/toml_edit) | 0.25.17 | MIT OR Apache-2.0 | Leer y guardar `config.toml` en `bracket-session` conservando comentarios | +165 KB junto con el código de configuración; sin efecto medible en el arranque |
| [winreg](https://crates.io/crates/winreg) | 0.56.0 | MIT | Leer las fuentes instaladas del registro de Windows en `bracket-app` (y, en la Fase 5, las asociaciones de ficheros) | Incluido en el cambio del paso 4 (−1,36 MB en total al quitar las fuentes integradas) |
| [encoding_rs](https://crates.io/crates/encoding_rs) | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause | Decodificar y codificar texto (UTF-8, UTF-16 y codificaciones heredadas) en `bracket-core` | +430 KB junto con chardetng |
| [chardetng](https://crates.io/crates/chardetng) | 1.0.0 | Apache-2.0 OR MIT | Adivinar la codificación de ficheros que no son UTF-8 | (incluido arriba) |
| [rfd](https://crates.io/crates/rfd) | 0.17.2 | MIT | Diálogos nativos de Abrir y Guardar como (`IFileDialog` de Windows) en `bracket-ui` | +50 KB junto con el menú Archivo |
| [windows-sys](https://crates.io/crates/windows-sys) | 0.61.2 | MIT OR Apache-2.0 | Llamadas a `ReplaceFileW` (guardado que conserva atributos y permisos) y a `AttachConsole`/`GetStdHandle` (salida de `--help`) en `bracket-app`; la misma versión ya estaba en el árbol por winit y winreg | +12 KB |
| [notify](https://crates.io/crates/notify) | 8.2.0 | CC0-1.0 | Avisos de cambios en los ficheros abiertos (`ReadDirectoryChangesW`) en `bracket-session`; sin features por defecto (solo eligen backends de macOS). La 9.0 aún es candidata | +120 KB junto con el seguimiento de ficheros; sin efecto medible en el arranque |
| [syntect](https://crates.io/crates/syntect) | 5.3.0 | MIT | Resaltado con gramáticas de Sublime Text en `bracket-syntax`, con el motor de regex en Rust puro (`regex-fancy`, sin C). En tiempo de ejecución solo carga las gramáticas compiladas; el `build.rs` las compila | +1,99 MB junto con el paquete de gramáticas (344 KB); sin efecto medible en el arranque (se carga en un hilo al primer uso) |

La parte BSD-3-Clause de encoding_rs (tablas de datos de WHATWG) exige reproducir su aviso de copyright en la distribución binaria: se incluirá en el fichero de licencias de terceros que acompañe al instalador y al zip (Fase 5).

## Solo para desarrollo y pruebas

No se incluyen en el ejecutable.

| Crate | Versión | Licencia | Uso |
|---|---|---|---|
| [proptest](https://crates.io/crates/proptest) | 1.12.0 | MIT OR Apache-2.0 | Tests de propiedades |
| [criterion](https://crates.io/crates/criterion) | 0.8.2 | Apache-2.0 OR MIT | Benchmarks |

## Gramáticas y temas

| Recurso | Origen | Licencia |
|---|---|---|
| Gramáticas de C#, CSS, JSON, XML, HTML, JavaScript y el resto del paquete de syntect | [sublimehq/Packages](https://github.com/sublimehq/Packages), a través del paquete incluido en syntect 5.3.0 | Licencia propia de Sublime HQ: permite copiar, usar, modificar y distribuir, sin garantía y sin exigir atribución (los ficheros con licencia propia se rigen por ella) |
| Gramáticas de Razor y T-SQL (`crates/bracket-syntax/grammars/`) | Propias de Bracket | MIT OR Apache-2.0 |
| Tema de colores InspiredGitHub | Incluido en syntect 5.3.0 ([InspiredGitHub.tmtheme](https://github.com/sethlopezme/InspiredGitHub.tmtheme)) | MIT |
