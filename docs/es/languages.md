[English](../en/languages.md) · **Español**

# Lenguajes

Qué lenguajes trae Bracket, qué hace con cada uno y cómo añadir otros con un paquete.

## Lenguajes integrados

| Lenguaje | Identificador | Extensiones | Insignia | Símbolos | Ruta bajo el cursor |
|---|---|---|---|---|---|
| C# | `csharp` | `.cs`, `.csx` | C# | tree-sitter | — |
| Razor | `razor` | `.razor`, `.cshtml` | @ | directivas por reglas; `@code` con tree-sitter | — |
| CSS | `css` | `.css` | # | tree-sitter | — |
| T-SQL | `tsql` | `.sql` | SQL | reglas regex | — |
| JSON | `json` | `.json`, `.jsonc` | {} | tree-sitter | ruta JSON |
| XML | `xml` | `.xml`, `.xsig`, `.xsd`, `.xsl`, `.xslt`, `.config`, `.csproj`, `.vbproj`, `.props`, `.targets`, `.resx`, `.nuspec`, `.xaml` | <> | tree-sitter | XPath |
| Rust | `rust` | `.rs` | RS | — | — |

Todos tienen resaltado de sintaxis (las gramáticas de syntect, compiladas dentro del ejecutable). El lenguaje se elige por la extensión y se puede cambiar en la barra de estado.

## Qué se hace con los símbolos

Los símbolos son las declaraciones de un fichero: clases, métodos, reglas de CSS, claves de JSON, procedimientos… Con ellos funcionan:

- **Esquema** (pestaña del panel lateral, **Ver → Esquema**): los símbolos del documento activo en árbol.
- **Ctrl+Mayús+O** (`@` en la paleta): ir a un símbolo del documento activo.
- **Ctrl+T** (`#` en la paleta): ir a un símbolo de cualquier fichero del proyecto.
- **F12 y Ctrl+clic**: ir a la definición del nombre del cursor.

**Mayús+F12** (referencias) no usa los símbolos: busca el nombre como palabra completa en el proyecto, así que funciona en cualquier lenguaje. Distingue mayúsculas salvo en T-SQL y en texto sin lenguaje.

Límites: el esquema y la ruta bajo el cursor, documentos de hasta 10 MB; el índice del proyecto, ficheros de hasta 1 MB; como mucho 50.000 símbolos por fichero.

## Símbolos de cada lenguaje

### C#

Espacios de nombres (también los de ámbito de fichero, `namespace Demo;`, que abarcan el resto del fichero), clases, structs, interfaces, records, enums y sus miembros, delegados, métodos, constructores, propiedades, campos (uno por variable: `int a, b;` son dos), eventos e indexadores (`this`). Cada símbolo cuelga del que lo contiene.

### Razor

Las directivas `@page`, `@using`, `@inject`, `@inherits`, `@implements`, `@layout`, `@attribute`, `@namespace`, `@model`, `@typeparam` y `@rendermode`, con su línea como nombre; y los miembros de los bloques `@code` y `@functions`, analizados con la gramática de C#. En Razor, ir a definición sobre `<MiComponente />` abre `MiComponente.razor`.

### CSS

Las reglas, con sus selectores como nombre (`.btn, .btn:hover`); `@media`, `@supports` y otras reglas `@`, con su primera línea; `@keyframes`, con su nombre; y las variables (`--color-principal`). Las reglas dentro de `@media` cuelgan de ella. Ir a definición toma `-` como parte del nombre, así que funciona con las variables.

### JSON

Cada clave, anidada como en el documento. Los objetos y arrays dentro de un array se llaman por su posición: `[0]`, `[1]`…

### XML y XSIG

Cada elemento, con su etiqueta como nombre (con prefijo: `ds:Signature`) y, si tiene atributo `id`, `Id`, `ID`, `name` o `Name`, también ese atributo: `Line id="3"`.

### T-SQL

`CREATE`, `ALTER` y `CREATE OR ALTER` de `PROCEDURE` (o `PROC`), `FUNCTION`, `VIEW` y `TRIGGER`; `CREATE TABLE` y `CREATE TYPE`. Sin distinguir mayúsculas. Los corchetes se quitan del nombre (`[dbo].[GetInvoice]` queda `dbo.GetInvoice`) y cada símbolo llega hasta el siguiente `GO` o el siguiente símbolo. Son las mismas reglas que puede traer un paquete (ver [symbols.toml](#symbolstoml)).

### Rust

Sin símbolos de momento: solo resaltado. Un paquete puede añadírselos con un `symbols.toml`.

## Ruta bajo el cursor

En XML y XSIG, la barra de estado muestra la XPath del elemento del cursor: `/Invoice/Lines/Line[3]/Amount`. Un elemento lleva su posición (desde 1) solo si tiene hermanos con la misma etiqueta, y dentro de un atributo se añade `/@nombre`. En JSON, la ruta del valor: `$.items[2].name`, con las posiciones de los arrays desde 0 y `['otra clave']` para las claves que no son identificadores. Un clic la copia.

## Paquetes de lenguaje

Un paquete añade un lenguaje, o sustituye uno integrado, con una carpeta por lenguaje dentro de `languages`:

- **Instalado:** `%APPDATA%\Bracket\languages\` y, con menos prioridad, `languages\` junto a `bracket.exe`.
- **Portable:** solo `languages\` junto a `bracket.exe`.

El nombre de la carpeta es el identificador del lenguaje; si coincide con uno integrado (`csharp`, `razor`, `css`, `tsql`, `json`, `xml`, `rust`), lo sustituye. Si un paquete tiene un problema (falta una clave, `format_version` de otra versión, una gramática con errores), se ignora y la barra de estado lo indica; si el error está en `symbols.toml`, el lenguaje se carga sin esas reglas. Los cambios en los paquetes se aplican al reiniciar Bracket.

```
languages\
  python\
    language.toml          obligatorio
    syntax.sublime-syntax  opcional: la gramática para el resaltado
    symbols.toml           opcional: reglas para los símbolos
```

### language.toml

```toml
format_version = 1               # obligatorio: versión del formato
name = "Python"                  # obligatorio: el nombre que se ve
extensions = ["py", "pyw"]       # obligatorio: sin punto
scope = "source.python"          # opcional: una gramática que ya incluye Bracket
tab_width = 4                    # opcional: tabulación para este lenguaje
line_comment = "#"               # opcional, para funciones futuras
block_comment = ['"""', '"""']   # opcional, para funciones futuras
pairs = [["(", ")"], ["[", "]"]] # opcional, para funciones futuras

[badge]                          # opcional: por defecto, las dos primeras letras en gris
text = "Py"                      # de 1 a 3 caracteres
color = "#3776AB"
```

### Gramática

Un `syntax.sublime-syntax` en la carpeta del paquete (formato de Sublime Text, versión 1; las gramáticas que usan `extends` de Sublime Text 4 y los `.tmLanguage` no son compatibles). Sin fichero, `scope` permite usar una de las gramáticas incluidas, las del paquete de syntect: Python, Go, Java, JavaScript, HTML, Markdown, YAML, Shell, Batch… (unas 75). Sin gramática, el lenguaje se reconoce y lleva insignia, pero sin colores.

### symbols.toml

Reglas de expresiones regulares que encuentran los símbolos del lenguaje. Con ellas funcionan el esquema, Ctrl+Mayús+O, Ctrl+T e ir a definición, igual que con los integrados. Las reglas de un paquete mandan sobre el extractor integrado del mismo identificador; un paquete que sustituye un lenguaje integrado sin traer `symbols.toml` conserva sus símbolos.

```toml
format_version = 1

[[symbol]]
kind = "class"
pattern = '^class (?<name>\w+)'

[[symbol]]
kind = "function"
pattern = '^[ \t]*def (?<name>\w+)'
end = '^\S'        # opcional: donde termina el símbolo
strip = "[]"       # opcional: caracteres que se quitan del nombre
```

- **`kind`:** el tipo de símbolo, uno de la tabla de abajo.
- **`pattern`:** sintaxis de la crate `regex` (sin referencias hacia atrás ni búsquedas alrededor), con `^` y `$` en cada línea (también con saltos CRLF); `(?i)` ignora mayúsculas. El nombre es el grupo `name`, o el primer grupo, o la coincidencia entera. Un patrón compilado de más de 1 MB se rechaza.
- **`end`** (opcional): el símbolo llega hasta donde coincide `end` o hasta el siguiente símbolo, lo que llegue antes. Sin `end`, hasta el siguiente símbolo.
- **`strip`** (opcional): los caracteres que se quitan del nombre.
- Los símbolos de reglas no se anidan: todos quedan en el primer nivel del esquema.

### Tipos de símbolo

| `kind` | Insignia | Lo usan |
|---|---|---|
| `namespace` | N | C# |
| `class`, `struct`, `record`, `enum`, `delegate` | C, S, R, E, D | C# |
| `interface` | I | C# |
| `enum_member` | e | C# |
| `method`, `constructor` | M | C# |
| `property`, `field`, `indexer` | P, F, [] | C# |
| `event` | Ev | C# |
| `selector`, `at_rule`, `keyframes`, `variable` | #, @, K, V | CSS |
| `key` | K | JSON |
| `element` | <> | XML |
| `procedure`, `function`, `view`, `table`, `trigger`, `type` | P, F, V, T, Tr, T | T-SQL |
| `directive` | @ | Razor |

Cualquier paquete puede usar cualquiera de ellos.
