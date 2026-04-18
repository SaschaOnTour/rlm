
    (invocation_expression function: (identifier) @call_name)
    (invocation_expression function: (member_access_expression name: (identifier) @method_call))
    (using_directive (qualified_name) @using_path)
    (using_directive (identifier) @using_simple)
    (generic_name (identifier) @type_ref)
    (predefined_type) @type_ref
