[English](../en/user-guide.md) · **Español**

# Guía de usuario

Bracket está en desarrollo (Fase 1). Esta guía describe lo que ya funciona y crece con cada fase.

## Abrir y guardar

- Menú **Archivo**: Nuevo (Ctrl+N), Abrir (Ctrl+O), Guardar (Ctrl+S), Guardar como (Ctrl+Mayús+S) y Salir. Abrir y Guardar como usan los diálogos de Windows.
- También se abre un fichero pasándolo como argumento (`bracket.exe fichero.txt`) o arrastrándolo a la ventana.
- Guardar un documento sin título abre Guardar como. El guardado es atómico y conserva el BOM y los saltos de línea del fichero. Al guardar sobre un fichero existente se mantienen sus atributos (por ejemplo, oculto), sus permisos y su fecha de creación. El título de la ventana marca con ● los cambios sin guardar; si deshaces hasta el texto guardado, la marca desaparece.
- Si hay cambios sin guardar, Bracket pregunta antes de abrir otro fichero, crear uno nuevo o salir.
- Bracket detecta la codificación al abrir (UTF-8, UTF-16 y codificaciones heredadas como windows-1252) y guarda en la misma, byte a byte. Si un fichero no se puede volver a escribir exactamente igual, se abre en solo lectura y la barra de estado lo indica.

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
