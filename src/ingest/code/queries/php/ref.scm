
    (function_call_expression function: (name) @call_name)
    (member_call_expression name: (name) @method_call)
    (namespace_use_clause (qualified_name) @use_path)
    (namespace_use_clause (name) @use_simple)
    (named_type (name) @type_ref)
    (named_type (qualified_name) @type_ref_qualified)
