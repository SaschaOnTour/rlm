
    ; Function calls
    (call_expression
        function: (identifier) @call_name)
    (call_expression
        function: (member_expression
            property: (property_identifier) @method_call))

    ; Import paths
    (import_statement
        source: (string) @import_path)

    ; Type references
    (type_identifier) @type_ref

    ; Generic type arguments
    (type_arguments (type_identifier) @generic_type_ref)

    ; Decorators
    (decorator (call_expression function: (identifier) @decorator_name))
    (decorator (identifier) @decorator_name)
