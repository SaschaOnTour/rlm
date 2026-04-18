
    (call function: (identifier) @call_name)
    (call function: (attribute attribute: (identifier) @method_call))
    (import_statement name: (dotted_name) @import_name)
    (import_from_statement module_name: (dotted_name) @import_from_module)
    (import_from_statement name: (dotted_name) @import_from_name)
    (aliased_import name: (dotted_name) @import_alias)
    (type) @type_ref
