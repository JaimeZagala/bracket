; Symbols of XML (XSIG, .config, .csproj...): every element, named by its tag, plus its id or
; name attribute when it has one (`Line id="3"`).

(element (STag (Name) @name)) @definition.element
(element (EmptyElemTag (Name) @name)) @definition.element
(element
  (STag (Name) @name
    (Attribute (Name) @_attribute) @detail)
  (#any-of? @_attribute "id" "Id" "ID" "name" "Name")) @definition.element
(element
  (EmptyElemTag (Name) @name
    (Attribute (Name) @_attribute) @detail)
  (#any-of? @_attribute "id" "Id" "ID" "name" "Name")) @definition.element
