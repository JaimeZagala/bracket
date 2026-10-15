[English](../en/architecture.md) · **Español**

# Arquitectura

Estado: Fase 0. Este documento describe la estructura acordada; cada sección indica qué está implementado.

## Crates del workspace

| Crate | Responsabilidad | Depende de (workspace) | Estado |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), selecciones múltiples, deshacer/rehacer por transacciones, movimiento de cursor, codificación, saltos de línea, modelo de documento. Sin UI ni APIs de Windows. | — | Vacía |
| `bracket-syntax` | Carga de lenguajes integrados y externos, resaltado incremental por líneas con caché, temas. | `bracket-core` (previsto) | Vacía |
| `bracket-index` | Recorrido de proyecto, extracción de símbolos (tree-sitter y regex), índice incremental, búsqueda difusa, referencias por nombre. | `bracket-core` (previsto) | Vacía |
| `bracket-search` | Buscar y reemplazar con regex en fichero y carpeta, cancelable, con resultados en streaming. | `bracket-core` (previsto) | Vacía |
| `bracket-session` | Configuración TOML, modo portable, sesión recuperable, recientes, vigilancia de ficheros, registro de comandos. | `bracket-core` (previsto) | Vacía |
| `bracket-update` | Comprobación de versiones por canal, descarga, verificación de suma y firma, aplicación y vuelta atrás. | — | Vacía |
| `bracket-ui` | Interfaz egui: vista del editor, pestañas, árbol de proyecto, paleta, barra de estado. | `bracket-core` | Vacía |
| `bracket-app` | Binario `bracket.exe`: arranque, instancia única, argumentos, manifiesto de Windows, registro en HKCU. | `bracket-ui`, `bracket-update` | Vacía |

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

## Decisiones

Las decisiones de arquitectura se registran en [decisions/](decisions/).
