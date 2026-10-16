[English](../en/user-guide.md) · **Español**

# Guía de usuario

Bracket está en desarrollo (Fase 1). Esta guía describe lo que ya funciona y crece con cada fase.

## Abrir y guardar

- Menú **Archivo**: Nuevo (Ctrl+N), Abrir (Ctrl+O), Guardar (Ctrl+S), Guardar como (Ctrl+Mayús+S) y Salir. Abrir y Guardar como usan los diálogos de Windows.
- También se abren ficheros pasándolos como argumentos (`bracket.exe a.cs b.json`, ver [Línea de comandos](#línea-de-comandos)) o arrastrándolos a la ventana.
- Guardar un documento sin título abre Guardar como. El guardado es atómico y conserva el BOM y los saltos de línea del fichero. Al guardar sobre un fichero existente se mantienen sus atributos (por ejemplo, oculto), sus permisos y su fecha de creación. El título de la ventana marca con ● los cambios sin guardar; si deshaces hasta el texto guardado, la marca desaparece.
- Si hay cambios sin guardar, Bracket pregunta antes de cerrar la pestaña. Al salir no pregunta: los cambios se quedan en la sesión (ver abajo).
- Bracket detecta la codificación al abrir (UTF-8, UTF-16 y codificaciones heredadas como windows-1252) y guarda en la misma, byte a byte. Si un fichero no se puede volver a escribir exactamente igual, se abre en solo lectura y la barra de estado lo indica.

## Pestañas

- Cada fichero se abre en su propia pestaña. Si la pestaña activa es un documento nuevo vacío, el fichero ocupa su lugar; si el fichero ya está abierto, se va a su pestaña.
- La insignia de color indica el lenguaje según la extensión: `C#` (C#), `@` (Razor), `#` (CSS), `SQL` (T-SQL), `{}` (JSON) y `<>` (XML, también `.xsig`, `.config`, `.csproj`, `.resx`...). Al pasar el ratón por una pestaña se ve la ruta completa.
- Un punto en lugar de la × indica cambios sin guardar.
- **Cerrar:** Ctrl+W, la × o clic con el botón central. Al cerrar la última queda un documento nuevo.
- **Cambiar de pestaña:** Ctrl+Tab y Ctrl+Mayús+Tab, o Ctrl+AvPág y Ctrl+RePág.
- Los documentos nuevos se numeran: «Sin título 1», «Sin título 2»...

## Línea de comandos

```text
bracket [opciones] [fichero[:línea[:columna]]]...
```

- `fichero:línea` abre el fichero con el cursor en esa línea; `fichero:línea:columna`, también en esa columna (cuenta caracteres: una tabulación es uno). Sirve para abrir directamente los errores de un compilador, como `bracket Program.cs:42:13`.
- `-h`, `--help` muestra la ayuda en el idioma de la interfaz; `-v`, `--version`, la versión. Se escriben en la consola desde la que se lanzó Bracket, o en un mensaje si se lanzó desde el Explorador o un acceso directo.
- `--` indica que lo que sigue son ficheros aunque empiecen por `-`.

**Una sola ventana.** Si Bracket ya está abierto, lanzarlo de nuevo abre los ficheros en esa ventana y la trae al frente, aunque esté minimizada. Bracket en modo portable y Bracket instalado son instancias distintas, porque cada uno tiene su carpeta de datos.

## Sesión

Al cerrar Bracket se guarda la sesión: las pestañas abiertas, cuál estaba activa, la posición del cursor y del scroll de cada una, y el texto de los documentos con cambios sin guardar, incluidos los que no tienen título. Al volver a abrirlo todo aparece como estaba y esos documentos siguen marcados como modificados: no se escribe nada en tus ficheros hasta que guardas.

- Al abrir, solo se lee la pestaña activa (y las que tienen cambios sin guardar); las demás se leen la primera vez que las activas, así Bracket arranca rápido aunque la sesión tenga ficheros grandes.
- La sesión también se guarda unos segundos después de cada cambio, así que si Bracket o Windows se cierran de golpe se pierden como mucho esos segundos.
- Si un fichero ya no existe y no tenía cambios, su pestaña no se abre. Si tenía cambios sin guardar, se abre con ellos y al guardar se vuelve a crear.
- Se guarda en `session.toml`, y el texto sin guardar en la carpeta `backups`, junto a `config.toml` (en `%APPDATA%\Bracket` o junto al exe en modo portable). Si `session.toml` estuviera dañado, Bracket lo aparta como `session.damaged.toml`, empieza sin sesión y no borra ninguna copia de `backups`.
- Si la sesión no se pudiera escribir al salir, Bracket pregunta qué hacer con los cambios sin guardar, como antes.
- La ventana vuelve a la posición y el tamaño que tenía, y maximizada si lo estaba (se guarda en `window.ron`, junto a `session.toml`). Si su monitor ya no está conectado, se abre en uno visible. La primera vez se abre centrada.

## Resaltado de sintaxis

Los ficheros de C#, Razor (`.razor`, `.cshtml`), CSS, T-SQL (`.sql`), JSON y XML (también `.xsig`, `.config`, `.csproj`...) se muestran con colores, según la insignia de su pestaña. El resaltado se calcula en segundo plano: al abrir un fichero largo primero se colorea lo que está a la vista.

- Los ficheros de más de 16 MB y las líneas de más de 20.000 caracteres (por ejemplo, JSON minificado) se muestran sin colores, para que el editor siga siendo rápido.
- En Razor se colorean las directivas (`@page`, `@using`...), los comentarios `@* *@`, las expresiones (`@Model.Nombre`, `@(a + b)`) y el C# de `@{ }` y `@code { }`. El HTML dentro de los bloques de código todavía no se reconoce.
- Por ahora el tema de colores es fijo (claro); los temas llegan en un paso posterior.

## Cambios en disco

Bracket vigila los ficheros abiertos. Si otro programa los cambia mientras están abiertos:

- **Sin cambios en Bracket:** el fichero se recarga solo y la barra de estado lo indica. El cursor se queda en la misma línea y columna, y Deshacer vuelve al texto anterior.
- **Con cambios sin guardar:** aparece un aviso encima del texto. «Recargar del disco» sustituye tu texto por el del disco (Deshacer lo recupera); «Mantener mis cambios» lo deja como está y el siguiente guardado sobrescribe el fichero.
- **Borrado o movido:** un aviso lo indica y la pestaña queda como modificada, así que el texto no se pierde: al guardar se vuelve a crear el fichero. Si el fichero vuelve con el mismo contenido, la pestaña deja de estar modificada.

## Codificación y saltos de línea

La barra de estado muestra la codificación (por ejemplo `UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM` o `windows-1252`) y el salto de línea (`CRLF`, `LF` o `CR`). Pulsando sobre ellos:

- **Reabrir con codificación:** vuelve a leer el fichero del disco con la codificación elegida (por si la detección se equivocó). Antes hay que guardar o descartar los cambios.
- **Guardar con codificación:** el próximo guardado usará esa codificación. Si el texto tiene caracteres que no existen en ella, no se guarda nada y la barra de estado dice cuál.
- **Convertir a CRLF / LF / CR:** cambia todos los saltos de línea del documento en un solo paso que se puede deshacer, y usa ese salto para las líneas nuevas.

## Idioma

La interfaz está en español (predeterminado) e inglés. Para cambiarla, pulsa el nombre del idioma a la derecha de la barra de estado y elige otro: el cambio es inmediato y se recuerda en la configuración.

## Configuración

La configuración está en `config.toml`:

- **Instalado:** `%APPDATA%\Bracket\config.toml`.
- **Portable:** `config.toml` junto a `bracket.exe` (ver más abajo).

El fichero es opcional: si no existe se usan los valores por defecto, y Bracket no lo crea hasta que se cambia un ajuste desde la interfaz. Se puede editar a mano:

```toml
[interface]
language = "es"      # idioma de la interfaz: "es" (español) o "en" (inglés)

[editor]
font_family = "Consolas" # fuente del editor: cualquier fuente instalada
font_size = 14.0     # tamaño de letra, de 8 a 48
tab_width = 4        # ancho de tabulación, de 1 a 16

[files]
line_ending = "crlf" # salto de línea de los ficheros nuevos: "crlf", "lf" o "cr"
encoding = "utf-8"   # codificación de los ficheros nuevos: "utf-8", "utf-8-bom" u otra (p. ej. "windows-1252")
```

El idioma también se puede cambiar desde el botón de la derecha de la barra de estado: el cambio es inmediato y se guarda en `config.toml`. El resto de cambios hechos a mano se aplican al reiniciar Bracket. Si un valor no es válido, se usa su valor por defecto y la barra de estado lo avisa; el resto del fichero se respeta. Cuando Bracket guarda la configuración, escribe todas sus claves con el valor actual, mantiene tus comentarios, el orden y las claves que no conoce, y nunca sobrescribe un fichero con errores de sintaxis.

## Fuentes

Bracket no incluye fuentes: usa las instaladas en Windows. El editor usa la fuente de `font_family` (Consolas por defecto; también vale cualquier fuente instalada, incluidas las instaladas solo para tu usuario). Si no está instalada, se usa Consolas, Cascadia Mono o Courier New y la barra de estado lo avisa. La interfaz usa Segoe UI.

Si el texto contiene caracteres que la fuente no tiene, Bracket carga automáticamente una fuente del sistema que los tenga: Segoe UI Symbol para símbolos, Microsoft YaHei (o Yu Gothic, MS Gothic) para chino y japonés, y Malgun Gothic para coreano. Estas fuentes solo se cargan cuando aparece ese texto en pantalla, porque ocupan bastante memoria (unos 40 MB en el caso de Microsoft YaHei).

## Modo portable

Si junto a `bracket.exe` hay un fichero llamado `portable.toml` (puede estar vacío), Bracket funciona en modo portable: la configuración y el resto de datos se guardan en esa misma carpeta, y no se escribe nada en `%APPDATA%` ni en el registro de Windows. Así se puede llevar en una memoria USB o en una carpeta sincronizada.
