[English](../en/architecture.md) · **Español**

# Arquitectura

Estado: Fase 0. Este documento describe la estructura acordada; cada sección indica qué está implementado.

## Crates del workspace

| Crate | Responsabilidad | Depende de (workspace) | Estado |
|---|---|---|---|
| `bracket-core` | Buffer (ropey), selecciones múltiples, deshacer/rehacer por transacciones, movimiento de cursor, codificación, saltos de línea, modelo de documento. Sin UI ni APIs de Windows. | — | Fase 0: buffer, selecciones, transacciones, historial, movimiento y documento UTF-8 |
| `bracket-syntax` | Carga de lenguajes integrados y externos, resaltado incremental por líneas con caché, temas. | `bracket-core` (previsto) | Vacía |
| `bracket-index` | Recorrido de proyecto, extracción de símbolos (tree-sitter y regex), índice incremental, búsqueda difusa, referencias por nombre. | `bracket-core` (previsto) | Vacía |
| `bracket-search` | Buscar y reemplazar con regex en fichero y carpeta, cancelable, con resultados en streaming. | `bracket-core` (previsto) | Vacía |
| `bracket-session` | Configuración TOML, modo portable, sesión recuperable, recientes, vigilancia de ficheros, registro de comandos. | `bracket-core` (previsto) | Vacía |
| `bracket-update` | Comprobación de versiones por canal, descarga, verificación de suma y firma, aplicación y vuelta atrás. | — | Vacía |
| `bracket-ui` | Interfaz egui: vista del editor, pestañas, árbol de proyecto, paleta, barra de estado. | `bracket-core` | Fase 0: prototipo (vista del editor, ventana, barra de estado mínima, catálogo de textos) |
| `bracket-app` | Binario `bracket.exe`: arranque, instancia única, argumentos, manifiesto de Windows, registro en HKCU. | `bracket-ui`, `bracket-update` | Fase 0: arranque y fichero por argumento |

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
- **Documento:** buffer + ruta + BOM + salto de línea + versión guardada. Guardado atómico: fichero temporal en la misma carpeta, `sync_all` y renombrado sobre el destino. En la Fase 0 solo UTF-8; un fichero que no es UTF-8 se abre en solo lectura y no se puede guardar.

## Prototipo de interfaz (`bracket-ui`, Fase 0)

- `EditorView` pinta solo las líneas visibles dentro de un `ScrollArea` de egui y traduce teclado, ratón, portapapeles e IME a órdenes del núcleo. La composición del IME se muestra en el cursor sin entrar en el buffer hasta que se confirma.
- Límite conocido: egui usa coordenadas `f32`. Con 1M de líneas (unos 17 millones de píxeles) la precisión es de unos 2 px al final del documento; por encima de unos pocos millones de líneas hará falta un scroll propio basado en líneas.
- Los textos visibles están en `i18n.rs` (español e inglés). La solución definitiva de traducción se decide en la Fase 1.

## Deuda técnica conocida

Pendiente de resolver; cada punto indica la fase prevista.

- **Fuentes (Fase 1):** egui solo trae Hack, Ubuntu y emoji; los caracteres CJK (chino, japonés, coreano) se ven como cuadros. Hay que cargar fuentes del sistema desde `C:\Windows\Fonts` (una monoespaciada como Consolas o Cascadia Mono y una CJK de respaldo) midiendo su coste en arranque y memoria.
- **Arranque (Fase 1):** ~140 ms sin carga, al límite del presupuesto de 150 ms. Perfilar eframe (contexto OpenGL, fuentes, primer frame) y vigilar regresiones (ver [decisions/0001-ui.md](decisions/0001-ui.md)).
- **Codificaciones (Fase 1):** solo UTF-8; el resto se abre en solo lectura.
- **Guardado (Fase 1):** el renombrado no conserva atributos, ACL ni hard links del fichero original (usar `ReplaceFileW` desde la capa de Windows); el guardado desde el diálogo de cambios sin guardar bloquea la interfaz.
- **Estado modificado (Fase 1):** deshacer hasta el estado guardado sigue marcando el documento como modificado.
- **Unidades de texto (Fase 4):** los clústeres de grafemas no se tratan como unidad y los caracteres anchos (CJK) cuentan como una columna.
- **Ficheros grandes (Fase 4):** las líneas muy largas se maquetan enteras, y el `ScrollArea` de egui pierde precisión en `f32` por encima de ~1M de líneas (hará falta un scroll propio basado en líneas).

## Decisiones

Las decisiones de arquitectura se registran en [decisions/](decisions/).
