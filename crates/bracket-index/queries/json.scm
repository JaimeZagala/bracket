; Symbols of JSON: every key, and the objects and arrays inside arrays, named by their index
; ([0], [1]...) so the outline follows the document's structure.

(pair key: (string (string_content) @name)) @definition.key
(array (object) @definition.key_index)
(array (array) @definition.key_index)
