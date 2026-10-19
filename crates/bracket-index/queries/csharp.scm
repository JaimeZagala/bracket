; Symbols of C#: each declaration is captured as @definition.<kind> and its name as @name.
; A file-scoped namespace ("namespace Demo;") does not wrap the declarations after it, so its
; range is extended to the end of the document (the _to_end suffix).

(namespace_declaration name: (_) @name) @definition.namespace
(file_scoped_namespace_declaration name: (_) @name) @definition.namespace_to_end

(class_declaration name: (identifier) @name) @definition.class
(struct_declaration name: (identifier) @name) @definition.struct
(interface_declaration name: (identifier) @name) @definition.interface
(record_declaration name: (identifier) @name) @definition.record
(enum_declaration name: (identifier) @name) @definition.enum
(enum_member_declaration name: (identifier) @name) @definition.enum_member
(delegate_declaration name: (identifier) @name) @definition.delegate

(method_declaration name: (identifier) @name) @definition.method
(constructor_declaration name: (identifier) @name) @definition.constructor
(property_declaration name: (identifier) @name) @definition.property
(indexer_declaration "this" @name) @definition.indexer
(event_declaration name: (identifier) @name) @definition.event

; One symbol per variable: "int a, b;" declares two fields.
(field_declaration
  (variable_declaration (variable_declarator name: (identifier) @name))) @definition.field
(event_field_declaration
  (variable_declaration (variable_declarator name: (identifier) @name))) @definition.event
