[English](THIRD_PARTY.en.md) · **Español**

# Componentes de terceros

Dependencias y recursos de terceros incluidos en el ejecutable de Bracket, con su origen y licencia. Se actualiza en el mismo commit que añade, cambia o quita una dependencia.

Cada dependencia nueva se justifica antes de entrar: mantenimiento activo, licencia compatible con MIT/Apache-2.0 e impacto medido en tamaño del ejecutable y arranque.

## Crates de Rust

Solo las dependencias directas; las transitivas se revisarán con una herramienta de licencias antes de la 1.0.0.

| Crate | Versión | Licencia | Uso | Impacto medido |
|---|---|---|---|---|
| [ropey](https://crates.io/crates/ropey) | 1.6.1 | MIT | Buffer de texto (rope) en `bracket-core` | Se mide al enlazarlo en `bracket.exe` (Fase 0, paso 11) |
| [eframe](https://crates.io/crates/eframe) (incluye egui) | 0.36.2 | MIT OR Apache-2.0 | Interfaz gráfica (backend glow) en `bracket-ui` | Exe de 0,10 a 5,23 MB; arranque de 9 a 139 ms ([mediciones](docs/es/measurements.md)) |

## Solo para desarrollo y pruebas

No se incluyen en el ejecutable.

| Crate | Versión | Licencia | Uso |
|---|---|---|---|
| [proptest](https://crates.io/crates/proptest) | 1.12.0 | MIT OR Apache-2.0 | Tests de propiedades |

## Gramáticas y temas

Todavía no hay gramáticas ni temas incluidos.

| Recurso | Origen | Licencia |
|---|---|---|
