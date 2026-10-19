[English](../../en/decisions/0003-symbols.md) · **Español**

# 0003 — Extracción de símbolos con tree-sitter

- **Estado:** aceptada el 2026-10-19 al aprobar la Fase 3. El plan ya preveía tree-sitter; antes de aprobarlo se midió su coste y se planteó la alternativa de extractores propios.
- **Contexto de medición:** Rust 1.99.0, programa mínimo con el perfil release del workspace (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`), 2026-10-19, equipo A.

## Requisitos

- Símbolos de C#, CSS, JSON, XML/XSIG, T-SQL y Razor para el esquema, Ctrl+Mayús+O, Ctrl+T, ir a definición y referencias por nombre (aproximadas, sin compilador).
- Ruta XPath y JSON bajo el cursor.
- Sin bloquear la interfaz: análisis en hilos de trabajo, incremental al editar.

## Opciones medidas

| Opción | Tamaño añadido | Notas |
|---|---|---|
| tree-sitter 0.27.1 + `tree-sitter-c-sharp` 0.23.5 | +5,4 MB | La gramática de C# es casi todo el coste (tablas del analizador) |
| … + `tree-sitter-css` 0.25.0, `tree-sitter-json` 0.24.8 y `tree-sitter-xml` 0.7.0 | +0,16 MB más | Total +5,55 MB |
| Extractores propios (escáner por lenguaje) | Decenas de KB (estimado) | Solo reconocen declaraciones; el de C# es el trabajo grande |

Con el ejecutable en 9,1 MB (0.2.0), tree-sitter lo deja en unos 14,6 MB frente al presupuesto de 15 MB.

## Decisión

**tree-sitter** para C#, CSS, JSON y XML; reglas regex para T-SQL y para los paquetes de lenguaje externos (`symbols.toml`); Razor combina sus directivas con la gramática de C# para los bloques `@code`.

- Árbol sintáctico completo y tolerante a errores: un fichero a medio escribir se sigue analizando bien.
- Análisis incremental al editar.
- Gramáticas mantenidas por la comunidad (MIT); los símbolos salen de consultas propias sobre el árbol.
- El mismo árbol sirve para la ruta XPath y JSON y, más adelante, para el plegado (Fase 4).

## Consecuencias

- El ejecutable queda cerca del presupuesto de tamaño. El paso 1 de la Fase 3 mide si se puede reducir sin perder velocidad, y la Fase 5 tendrá que caber en lo que quede; si no cabe, se revisará el presupuesto con datos.
- Las gramáticas son código C compilado con MSVC (ya necesario para compilar Bracket): las compilaciones y el CI tardan algo más y ese código queda fuera de las reglas sobre `unsafe` del workspace.
- Las gramáticas pueden ir por detrás de las versiones nuevas de C#; los símbolos que dependan de sintaxis nueva se comprueban en el corpus de pruebas.
- Las gramáticas no se pueden cargar sin recompilar: los paquetes externos siguen usando reglas regex.
- Los extractores van detrás de una interfaz común (fichero → símbolos), así que un lenguaje podría cambiar de método sin tocar el índice, la paleta ni la navegación.
