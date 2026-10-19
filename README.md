[English](README.en.md) · **Español**

# Bracket

Editor de código nativo y ligero para Windows 10/11 (x64), escrito en Rust. Pensado para quien quiere algo más que Notepad++ (proyecto, símbolos, búsqueda en carpeta) sin el peso de VS Code, y sigue usando un IDE completo para el trabajo de fondo.

> **Estado: Fase 3 (símbolos y navegación, 0.3.0) completada; sin publicar.** Todavía no hay nada que instalar: se compila desde el código. Este README describe el objetivo; la sección [Hoja de ruta](#hoja-de-ruta) dice qué existe ya y la [guía de usuario](docs/es/user-guide.md) cómo se usa.

**No es Brackets.** Bracket (en singular) no tiene relación con Brackets, el editor de Adobe ya descontinuado, ni con sus sucesores Brackets Continued y Phoenix Code.

## Para quién

Desarrolladores en Windows que abren muchos ficheros sueltos y carpetas de proyecto (C#, Razor, CSS, T-SQL, JSON, XML) y quieren un editor que arranque al instante, navegue por símbolos y busque en el proyecto, sin extensiones ni configuración.

## Funcionalidades previstas

- Pestañas, sesión recuperable y recarga automática si el fichero cambia en disco.
- Guardado atómico que conserva codificación, BOM y saltos de línea. Los XML/XSIG se guardan byte a byte (no invalida ficheros firmados).
- Carpeta como proyecto: árbol lateral que respeta `.gitignore`, Ctrl+P (fichero), Ctrl+T (símbolo en el proyecto), Ctrl+Shift+O (símbolo en el fichero), paleta de comandos, buscar y reemplazar con regex en fichero y carpeta.
- Navegación por símbolos sin compilador: outline, ir a definición y referencias por nombre (aproximadas).
- Resaltado y símbolos para C#, Razor, CSS, T-SQL, JSON y XML/XSIG, y resaltado para Rust, integrados en el ejecutable. Otros lenguajes se añaden copiando una carpeta, sin recompilar.
- Instalador por usuario (sin administrador) o zip portable. "Abrir con" y asociaciones de ficheros.
- Actualizaciones solo cuando el usuario las acepta, con canal estable y canal preliminar opcional.
- Interfaz en español (predeterminado) e inglés.
- Sin telemetría. La única conexión de red es la comprobación de actualizaciones, desactivable.

## Objetivos de rendimiento

Resultados de la 0.3.0 en un portátil Intel Core i7-13620H con gráfica integrada y Windows 11 (método y detalle en [mediciones](docs/es/measurements.md)).

| Medida | Objetivo | 0.3.0 |
|---|---|---|
| Tamaño del ejecutable | < 15 MB | 14,8 MB (tree-sitter y sus gramáticas: +5,5 MB) |
| Arranque hasta poder escribir | < 150 ms | 124–137 ms, también abriendo un proyecto de 50.000 ficheros; igual que la 0.2.0 medida intercalada (en caliente; la primera ejecución tras compilar incluye el análisis de Windows Defender) |
| RAM en reposo con 10 pestañas | < 80 MB | 43 MB (38 MB con el proyecto de 50.000 ficheros; 42–45 MB con la navegación experimental activada) |
| Fichero de 100 MB | visible en < 1 s sin bloquear la interfaz | 0,33 s |
| Ctrl+P en un proyecto de 50.000 ficheros | < 50 ms por pulsación | 8 ms |
| Ctrl+T con 210.000 símbolos (experimental) | < 50 ms por pulsación | 13 ms |

Límites conocidos: sin sistema de extensiones y sin LSP de serie (LSP opcional previsto para la 1.1.0).

## Compilar desde el código

Requisitos: Windows 10/11 x64, [rustup](https://rustup.rs) y las Build Tools de Visual Studio con la carga de trabajo de C++ (MSVC y Windows SDK). La versión de Rust se instala sola a partir de `rust-toolchain.toml`.

```powershell
git clone https://github.com/JaimeZagala/bracket.git
cd bracket
cargo build --release -p bracket-app
# Ejecutable: target\release\bracket.exe
./scripts/check.ps1   # formato, clippy, tests y documentación
```

## Hoja de ruta

| Fase | Versión | Contenido | Estado |
|---|---|---|---|
| 0 | — | Cimientos y prototipo del editor | Completada |
| 1 | 0.1.0 | Editor base: pestañas, sesión, codificaciones, resaltado, temas, configuración, idiomas | Completada (sin publicar) |
| 2 | 0.2.0 | Proyectos: árbol de carpetas, Ctrl+P, paleta, buscar y reemplazar en carpeta | Completada (sin publicar) |
| 3 | 0.3.0 | Símbolos y navegación: esquema, Ctrl+Mayús+O, ir a definición, historial, ruta XPath/JSON; índice del proyecto experimental | Completada (sin publicar) |
| 4 | 0.4.0 | Edición avanzada: multicursor, plegado, diff, ficheros grandes | Pendiente |
| 5 | 0.5.0 → 1.0.0 | Instalador, zip portable, actualizador, firma y publicación | Pendiente |
| 6 | 1.1.0 | LSP opcional | Pendiente |

Detalle: [docs/es/plan.md](docs/es/plan.md). Arquitectura: [docs/es/architecture.md](docs/es/architecture.md). Lenguajes: [docs/es/languages.md](docs/es/languages.md). Cambios: [CHANGELOG.md](CHANGELOG.md).

## Licencia

Bracket se distribuye bajo doble licencia, a elección de quien lo use: [MIT](LICENSE-MIT) o [Apache-2.0](LICENSE-APACHE). Componentes de terceros: [THIRD_PARTY.md](THIRD_PARTY.md).
