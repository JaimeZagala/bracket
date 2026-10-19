[English](../en/user-guide.md) · **Español**

# Guía de usuario

Bracket 0.1.0 (Fase 1, editor base). Esta guía describe lo que ya funciona y crece con cada fase.

## Abrir y guardar

- Menú **Archivo**: Nuevo (Ctrl+N), Abrir (Ctrl+O), Guardar (Ctrl+S), Guardar como (Ctrl+Mayús+S) y Salir. Abrir y Guardar como usan los diálogos de Windows.
- También se abren ficheros pasándolos como argumentos (`bracket.exe a.cs b.json`, ver [Línea de comandos](#línea-de-comandos)) o arrastrándolos a la ventana.
- Guardar un documento sin título abre Guardar como. El guardado es atómico y conserva el BOM y los saltos de línea del fichero. Al guardar sobre un fichero existente se mantienen sus atributos (por ejemplo, oculto), sus permisos y su fecha de creación. El título de la ventana marca con ● los cambios sin guardar; si deshaces hasta el texto guardado, la marca desaparece.
- Si hay cambios sin guardar, Bracket pregunta antes de cerrar la pestaña. Al salir no pregunta: los cambios se quedan en la sesión (ver abajo).
- Bracket detecta la codificación al abrir (UTF-8, UTF-16 y codificaciones heredadas como windows-1252) y guarda en la misma, byte a byte. Si un fichero no se puede volver a escribir exactamente igual, se abre en solo lectura y la barra de estado lo indica.

## Proyectos

Una carpeta se abre como proyecto con **Archivo → Abrir carpeta**, con `bracket.exe C:\ruta\carpeta`, arrastrándola a la ventana o pasándosela a un Bracket que ya esté abierto. El título de la ventana muestra el nombre del proyecto, el diálogo Abrir empieza en él y la sesión lo recuerda al cerrar. **Archivo → Cerrar carpeta** lo cierra; las pestañas abiertas se quedan.

**Archivo → Recientes** muestra los últimos ficheros (hasta 15) y carpetas (hasta 10) que se abrieron, los más recientes primero; los que ya no existen no aparecen. Una carpeta reciente se abre como proyecto. «Borrar la lista» la vacía. La lista se guarda en `recent.toml`, junto a `config.toml`.

Con un proyecto abierto, el **panel del proyecto** muestra a la izquierda el árbol de carpetas: primero las carpetas y después los ficheros, por nombre, con la insignia de lenguaje en los ficheros que la tienen. Un clic en una carpeta la abre o la cierra. Un clic en un fichero lo muestra en la **pestaña de vista previa**, con el título en cursiva: el siguiente fichero que se mire así ocupa su lugar, para no llenar la barra de pestañas. La pestaña pasa a ser normal al editar el documento, al hacer doble clic en ella o al hacer doble clic en el fichero del árbol. El borde derecho se arrastra para cambiar el ancho.

- **Ctrl+B** oculta o muestra el panel (también en **Ver → Panel del proyecto**).
- **Ctrl+Mayús+E** lleva el teclado al árbol: ↑ y ↓ mueven la selección (también Inicio, Fin, RePág y AvPág), → abre la carpeta o entra en ella, ← la cierra o sube a la carpeta que la contiene, Intro abre el fichero en una pestaña normal (o abre y cierra la carpeta), Espacio lo muestra en la vista previa sin salir del árbol, y Esc vuelve al editor.
- La sesión recuerda el ancho, si el panel está visible y qué carpetas estaban abiertas.
- El árbol se actualiza solo cuando otros programas crean, borran o renombran ficheros y carpetas del proyecto (un `git checkout`, una compilación, el Explorador), también si cambia `.gitignore`.
- El **menú contextual** (clic derecho en una entrada, o en el nombre del proyecto arriba) tiene Nuevo fichero, Nueva carpeta, Cambiar nombre (F2), Eliminar (Supr), Copiar ruta, Copiar ruta relativa y Mostrar en el Explorador. Sobre un fichero, lo nuevo se crea en su carpeta.
- El nombre se escribe en el propio árbol: Intro o un clic fuera lo confirma y Esc lo cancela. Al cambiar el nombre queda seleccionado sin la extensión, como en el Explorador. Un nombre no válido para Windows (con `<>:"/\|?*`, terminado en punto o espacio, o reservado como `CON` o `NUL`) se avisa en la barra de estado. Si ya existe algo con ese nombre, no se sustituye.
- **Eliminar** mueve la entrada a la Papelera de reciclaje, después de confirmar, así que se puede recuperar. Un fichero nuevo se abre en una pestaña; las pestañas de un fichero o una carpeta renombrados pasan al nuevo nombre.

### Ir a fichero (Ctrl+P)

**Ctrl+P** (o **Ir → Ir a fichero…**) abre un cuadro arriba para ir a un fichero escribiendo parte de su ruta:

- Basta con escribir algunas letras en orden, sin que vayan juntas: `prgcs` encuentra `src\Program.cs`. Pesan más las letras al principio de una palabra, las seguidas y las del nombre del fichero. Se pueden incluir carpetas con `\` o `/` (`models/user`); los espacios no cuentan.
- Las letras que coinciden se resaltan. ↑ y ↓ eligen (también RePág y AvPág), Intro abre y Esc cierra.
- Sin texto muestra los ficheros abiertos recientemente.
- `Program.cs:42` abre el fichero en la línea 42 y `Program.cs:42:7`, también en la columna 7. `:120` va a la línea 120 del documento activo.
- Busca en los ficheros del proyecto, con las mismas exclusiones que el árbol. El índice se prepara al abrir la carpeta (con 50.000 ficheros, una décima de segundo) y se actualiza solo. Sin proyecto, busca entre los recientes.

### Paleta de comandos e Ir a línea

- **Ctrl+Mayús+P** o **F1** (o **Ver → Paleta de comandos…**) abre el mismo cuadro con `>` delante: lista los comandos disponibles, como «Archivo: Guardar», con su atajo a la derecha, y se filtran escribiendo como en Ctrl+P. Al pasar el ratón por un comando se ve su identificador (por ejemplo `file.save`).
- **Ctrl+G** (o **Ir → Ir a línea…**) abre el cuadro con `:` delante: escribe la línea, o `línea:columna`, e Intro. También funciona escribir `:` en Ctrl+P.
- Los menús muestran los mismos comandos y atajos; los que no se pueden usar en ese momento (por ejemplo, Cerrar carpeta sin proyecto) salen desactivados.

La búsqueda de texto en el proyecto llega en los próximos pasos de la Fase 2.

## Pestañas

- Cada fichero se abre en su propia pestaña. Si la pestaña activa es un documento nuevo vacío, el fichero ocupa su lugar; si el fichero ya está abierto, se va a su pestaña.
- La insignia de color indica el lenguaje según la extensión: `C#` (C#), `@` (Razor), `#` (CSS), `SQL` (T-SQL), `{}` (JSON), `<>` (XML, también `.xsig`, `.config`, `.csproj`, `.resx`...) y `RS` (Rust). Al pasar el ratón por una pestaña se ve la ruta completa.
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

Los ficheros de C#, Razor (`.razor`, `.cshtml`), CSS, T-SQL (`.sql`), JSON, XML (también `.xsig`, `.config`, `.csproj`...) y Rust (`.rs`) se muestran con colores, según la insignia de su pestaña. El resaltado se calcula en segundo plano: al abrir un fichero largo primero se colorea lo que está a la vista.

- Los ficheros de más de 16 MB y las líneas de más de 20.000 caracteres (por ejemplo, JSON minificado) se muestran sin colores, para que el editor siga siendo rápido.
- En Razor se colorean las directivas (`@page`, `@using`...), los comentarios `@* *@`, las expresiones (`@Model.Nombre`, `@(a + b)`) y el C# de `@{ }` y `@code { }`. El HTML dentro de los bloques de código todavía no se reconoce.
- Los colores dependen del tema del editor (ver [Apariencia y temas](#apariencia-y-temas)).

## Apariencia y temas

En el menú **Ver**:

- **Apariencia:** interfaz clara, oscura o según Windows (por defecto). Con «Según Windows», Bracket cambia solo cuando cambia el modo de Windows.
- **Tema del editor:** los colores del texto y del fondo del editor. «Automático» (por defecto) usa InspiredGitHub con la apariencia clara y base16 Ocean oscuro con la oscura; también se puede elegir uno fijo: InspiredGitHub, Solarized (claro u oscuro), base16-ocean (claro u oscuro), base16-eighties o base16-mocha.

Los cambios se aplican al momento y se guardan en `config.toml`.

## Lenguajes externos

Se pueden añadir lenguajes, o sustituir uno de los integrados, con una carpeta por lenguaje dentro de `languages`:

- **Instalado:** `%APPDATA%\Bracket\languages\` y, con menos prioridad, `languages\` junto a `bracket.exe`.
- **Portable:** solo `languages\` junto a `bracket.exe`.

El nombre de la carpeta es el identificador del lenguaje; si coincide con uno integrado (`csharp`, `razor`, `css`, `tsql`, `json`, `xml`), lo sustituye. Dentro va `language.toml`:

```toml
format_version = 1             # obligatorio: versión del formato
name = "Python"                # obligatorio
extensions = ["py", "pyw"]     # obligatorio
scope = "source.python"        # gramática que ya incluye Bracket (opcional)
tab_width = 4                  # tabulación para este lenguaje (opcional)
line_comment = "#"             # opcional, para funciones futuras
block_comment = ['"""', '"""'] # opcional, para funciones futuras
pairs = [["(", ")"], ["[", "]"]] # opcional, para funciones futuras

[badge]                        # opcional: por defecto, las dos primeras letras en gris
text = "Py"                    # de 1 a 3 caracteres
color = "#3776AB"
```

- **Gramática:** un `syntax.sublime-syntax` en la misma carpeta (formato de Sublime Text, versión 1; las gramáticas que usan `extends` de Sublime Text 4 y los `.tmLanguage` no son compatibles). Si no hay fichero, `scope` permite usar cualquiera de las gramáticas incluidas, que son las del paquete de syntect: Python, Rust, Go, Java, JavaScript, HTML, Markdown, YAML, Shell, Batch... (unas 75). Sin gramática, el lenguaje se reconoce y lleva insignia, pero sin colores.
- Si un paquete tiene un problema (falta una clave, `format_version` de otra versión, gramática con errores), se ignora y la barra de estado lo indica. Los cambios en los paquetes se aplican al reiniciar Bracket.

## Cambios en disco

Bracket vigila los ficheros abiertos. Si otro programa los cambia mientras están abiertos:

- **Sin cambios en Bracket:** el fichero se recarga solo y la barra de estado lo indica. El cursor se queda en la misma línea y columna, y Deshacer vuelve al texto anterior.
- **Con cambios sin guardar:** aparece un aviso encima del texto. «Recargar del disco» sustituye tu texto por el del disco (Deshacer lo recupera); «Mantener mis cambios» lo deja como está y el siguiente guardado sobrescribe el fichero.
- **Borrado o movido:** un aviso lo indica y la pestaña queda como modificada, así que el texto no se pierde: al guardar se vuelve a crear el fichero. Si el fichero vuelve con el mismo contenido, la pestaña deja de estar modificada.

## Barra de estado

De izquierda a derecha:

- **Mensajes:** el resultado de la última acción (guardado, recarga...), avisos de configuración o de paquetes de lenguaje, o una pista para abrir ficheros.
- **Posición:** `Lín 12, Col 5` (la columna cuenta las tabulaciones hasta su tope). Con texto seleccionado añade cuántos caracteres (`(25 seleccionados)`), y con varias selecciones muestra cuántas hay (`3 selecciones`).
- **Salto de línea** y **codificación:** ver abajo. «Solo lectura» aparece junto a la codificación cuando el fichero no se puede guardar sin alterar sus bytes.
- **Lenguaje del documento:** el detectado por la extensión. Pulsando se elige otro para esta pestaña, o «Texto sin formato»; la elección se recuerda en la sesión. Guardar con otro nombre vuelve a detectarlo por la extensión.
- **Idioma de la interfaz.**

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
appearance = "system" # apariencia: "system" (según Windows), "light" o "dark"

[editor]
font_family = "Consolas" # fuente del editor: cualquier fuente instalada
theme = "auto"       # tema del editor: "auto" o un nombre, p. ej. "Solarized (dark)"
font_size = 14.0     # tamaño de letra, de 8 a 48
tab_width = 4        # ancho de tabulación, de 1 a 16

[files]
line_ending = "crlf" # salto de línea de los ficheros nuevos: "crlf", "lf" o "cr"
encoding = "utf-8"   # codificación de los ficheros nuevos: "utf-8", "utf-8-bom" u otra (p. ej. "windows-1252")

[project]
exclude = ["bin/", "obj/"] # se dejan fuera de todos los proyectos, como líneas de .gitignore
```

En los proyectos se dejan fuera también los ficheros que excluyen `.gitignore` e `.ignore` (aunque la carpeta no sea un repositorio Git) y las carpetas `.git`, `.svn` y `.hg`; los ficheros ocultos sí aparecen.

El idioma también se puede cambiar desde el botón de la derecha de la barra de estado: el cambio es inmediato y se guarda en `config.toml`. El resto de cambios hechos a mano se aplican al reiniciar Bracket. Si un valor no es válido, se usa su valor por defecto y la barra de estado lo avisa; el resto del fichero se respeta. Cuando Bracket guarda la configuración, escribe todas sus claves con el valor actual, mantiene tus comentarios, el orden y las claves que no conoce, y nunca sobrescribe un fichero con errores de sintaxis.

## Fuentes

Bracket no incluye fuentes: usa las instaladas en Windows. El editor usa la fuente de `font_family` (Consolas por defecto; también vale cualquier fuente instalada, incluidas las instaladas solo para tu usuario). Si no está instalada, se usa Consolas, Cascadia Mono o Courier New y la barra de estado lo avisa. La interfaz usa Segoe UI.

Si el texto contiene caracteres que la fuente no tiene, Bracket carga automáticamente una fuente del sistema que los tenga: Segoe UI Symbol para símbolos, Microsoft YaHei (o Yu Gothic, MS Gothic) para chino y japonés, y Malgun Gothic para coreano. Estas fuentes solo se cargan cuando aparece ese texto en pantalla, porque ocupan bastante memoria (unos 40 MB en el caso de Microsoft YaHei).

## Modo portable

Si junto a `bracket.exe` hay un fichero llamado `portable.toml` (puede estar vacío), Bracket funciona en modo portable: la configuración y el resto de datos se guardan en esa misma carpeta, y no se escribe nada en `%APPDATA%` ni en el registro de Windows. Así se puede llevar en una memoria USB o en una carpeta sincronizada.
