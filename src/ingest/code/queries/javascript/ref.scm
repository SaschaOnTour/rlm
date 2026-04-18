
    ; Function calls
    (call_expression
        function: (identifier) @call_name)
    (call_expression
        function: (member_expression
            property: (property_identifier) @method_call))

    ; Import paths
    (import_statement
        source: (string) @import_path)

    ; Require paths
    (call_expression
        function: (identifier) @_require
        arguments: (arguments (string) @require_path)
        (#eq? @_require "require"))

    ; JSX elements
    (jsx_element
        open_tag: (jsx_opening_element
            name: (identifier) @jsx_component))
    (jsx_self_closing_element
        name: (identifier) @jsx_component)
