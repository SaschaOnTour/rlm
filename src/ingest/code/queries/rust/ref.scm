
    (call_expression function: (identifier) @call_name)
    (call_expression function: (scoped_identifier name: (identifier) @scoped_call))
    (call_expression function: (field_expression field: (field_identifier) @method_call))
    (use_declaration argument: (scoped_identifier name: (identifier) @use_name))
    (use_declaration argument: (scoped_identifier) @use_path)
    (use_declaration argument: (use_as_clause path: (scoped_identifier) @use_as_path))
    (use_declaration argument: (use_list (identifier) @use_list_item))
    (use_declaration argument: (use_list (scoped_identifier name: (identifier) @use_list_scoped)))
    (use_declaration argument: (scoped_use_list path: (scoped_identifier) @use_group_path))
    (use_declaration argument: (identifier) @use_simple)
    (type_identifier) @type_ref
