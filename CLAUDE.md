# CLAUDE.md — Bracket

Editor de código nativo para Windows 10/11 x64, escrito en Rust. Intermedio entre Notepad++ y VS Code: ligero, con navegación de proyecto y símbolos. No es un IDE. Plan completo: [docs/es/plan.md](docs/es/plan.md). Arquitectura: [docs/es/architecture.md](docs/es/architecture.md).

## Estado

Fases 0, 1 y 2 completadas (0.1.0 y 0.2.0, etiquetas locales `v0.1.0` y `v0.2.0`). La Fase 3 (símbolos y navegación, 0.3.0) necesita aprobación antes de empezar. Versión del workspace: 0.2.0. Detalle en `docs/es/plan.md`.

## Comandos

```powershell
./scripts/check.ps1                         # todo lo de abajo, en orden
./scripts/check.ps1 -Perf                   # además: build release y presupuesto de arranque (150 ms)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
./scripts/check-doc-pairs.ps1               # parejas de idioma de la documentación
cargo build --release -p bracket-app        # target/release/bracket.exe
./scripts/measure.ps1 -Exe target/release/bracket.exe -Runs 12   # tamaño, arranque, RAM
cargo bench --workspace                     # benchmarks (criterion)
./scripts/compare-startup.ps1 -A <exe anterior> -B target/release/bracket.exe   # regresiones de arranque
cargo run --release -p bracket-core --example generate_large_file   # target/large-1m.txt
cargo run --release -p bracket-index --example generate_project     # target/project-50k (50.000 ficheros)
```

Las mediciones se anotan en `docs/es/measurements.md` y `docs/en/measurements.md` con fecha, versión y equipo.

Antes de dar un paso por terminado: `./scripts/check.ps1` en verde. No se desactivan lints ni tests para que pase.

## Reglas del proyecto

- Rust estable fijado en `rust-toolchain.toml`. Target `x86_64-pc-windows-msvc`. Sin nightly.
- Versión única en `[workspace.package]` de `Cargo.toml`. Crates con `publish = false`.
- Dependencias entre crates: solo `bracket-app` depende de `bracket-ui` y de `bracket-update`. `bracket-core` y `bracket-update` no dependen de ninguna crate del workspace. Las flechas siempre van hacia abajo.
- El núcleo (todo menos `bracket-ui` y `bracket-app`) no usa egui ni APIs de Windows.
- Hilo de interfaz: solo pinta y gestiona eventos. Trabajo pesado en hilos con cancelación.
- Sin `unsafe` salvo necesidad demostrada, con comentario `SAFETY` (lint `unsafe_code = "deny"` en el workspace). Excepciones hoy: `ReplaceFileW` en `crates/bracket-app/src/file_replace.rs` y `AttachConsole`/`GetStdHandle` en `crates/bracket-app/src/console.rs`.
- Sin telemetría. Única conexión de red: comprobación de actualizaciones (Fase 5).
- Sin pérdida de datos: guardado atómico, se conservan codificación, BOM y saltos de línea. XML/XSIG se guardan byte a byte.
- Dependencia nueva: justificar (mantenimiento, licencia, impacto medido en tamaño y arranque), comprobar versión y API reales con `cargo info` y docs.rs, y anotarla en `THIRD_PARTY.md` y `THIRD_PARTY.en.md`.
- No avanzar de fase sin aprobación. Sin refactors fuera del alcance de la tarea.

## Idiomas

- Código, comentarios, commits, ramas, logs, errores, tests y nombres de ficheros: inglés.
- Documentación: español (referencia) e inglés, en el mismo commit. Raíz: `X.md` (es) + `X.en.md` (en). `docs/es/` y `docs/en/` con los mismos nombres. Primera línea de cada fichero: enlace a su pareja.
- Excepciones: `LICENSE-MIT` y `LICENSE-APACHE` (solo inglés) y este `CLAUDE.md` (solo español).
- Textos visibles de la interfaz: nunca sueltos en el código. Van en `crates/bracket-ui/locales/es.toml` y `en.toml` (mismas claves y marcadores), con un método en `i18n.rs` y su línea en el test `every_method_finds_its_key`.
- Conversación con el autor: español.

## Git y repositorio público

- Identidad configurada a nivel de repositorio (`git config --local`): `Jaime Zagalá <692421+JaimeZagala@users.noreply.github.com>`. La identidad global del equipo es de empresa: comprobar `git log --format='%an <%ae>'` antes de cada push.
- Ningún dato de empresa ni de clientes, ningún secreto, en código, tests, corpus, commits ni issues. Revisar el diff antes de cada push.
- Nada en GitHub (repositorio, releases, etiquetas, push) sin confirmación explícita del autor.
- Conventional Commits en inglés (`feat(core): ...`). Un commit por paso pequeño que compila y tiene tests.
- Sin trailer `Co-Authored-By` en los commits.

## Notas del entorno

- Windows PowerShell 5.1 escribe UTF-8 **con BOM** (`Set-Content -Encoding utf8`). Para escribir ficheros usar las herramientas de edición o Git Bash; los scripts `.ps1` del repo se mantienen en ASCII.
- `cargo` y `gh` se instalaron con winget; en shells abiertos antes de la instalación hay que reabrir la terminal para tener el PATH.
