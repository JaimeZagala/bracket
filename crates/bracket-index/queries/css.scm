; Symbols of CSS. Rules are named by their selectors; @media, @supports and other at-rules by
; their first line (no @name capture); custom properties (--x) are variables.

(rule_set (selectors) @name) @definition.selector
(media_statement) @definition.at_rule
(supports_statement) @definition.at_rule
(at_rule) @definition.at_rule
(keyframes_statement (keyframes_name) @name) @definition.keyframes
((declaration (property_name) @name) @definition.variable
  (#match? @name "^--"))
