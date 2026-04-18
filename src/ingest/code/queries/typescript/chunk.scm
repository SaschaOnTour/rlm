
    ; Functions
    (function_declaration name: (identifier) @fn_name) @fn_def
    (generator_function_declaration name: (identifier) @gen_fn_name) @gen_fn_def

    ; Arrow functions assigned to variables
    (lexical_declaration
        (variable_declarator
            name: (identifier) @arrow_name
            value: (arrow_function))) @arrow_def

    ; Classes (including abstract classes)
    (class_declaration name: (type_identifier) @class_name) @class_def
    (abstract_class_declaration name: (type_identifier) @abs_class_name) @abs_class_def

    ; Class methods
    (method_definition
        name: (property_identifier) @method_name) @method_def

    ; Interfaces
    (interface_declaration name: (type_identifier) @iface_name) @iface_def

    ; Type aliases
    (type_alias_declaration name: (type_identifier) @type_alias_name) @type_alias_def

    ; Enums
    (enum_declaration name: (identifier) @enum_name) @enum_def

    ; ES Module imports
    (import_statement) @import_decl

    ; Namespaces/Modules
    (module name: (identifier) @namespace_name) @namespace_def
    (internal_module name: (identifier) @internal_namespace_name) @internal_namespace_def
