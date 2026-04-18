
    ; Functions
    (function_declaration name: (identifier) @fn_name) @fn_def
    (generator_function_declaration name: (identifier) @gen_fn_name) @gen_fn_def

    ; Arrow functions assigned to variables
    (lexical_declaration
        (variable_declarator
            name: (identifier) @arrow_name
            value: (arrow_function))) @arrow_def
    (variable_declaration
        (variable_declarator
            name: (identifier) @arrow_name
            value: (arrow_function))) @arrow_def

    ; Classes
    (class_declaration name: (identifier) @class_name) @class_def

    ; Class methods
    (method_definition
        name: (property_identifier) @method_name) @method_def

    ; ES Module imports
    (import_statement) @import_decl

    ; CommonJS require (variable declarations with require)
    (lexical_declaration
        (variable_declarator
            value: (call_expression
                function: (identifier) @_require_fn
                (#eq? @_require_fn "require")))) @require_decl
    (variable_declaration
        (variable_declarator
            value: (call_expression
                function: (identifier) @_require_fn
                (#eq? @_require_fn "require")))) @require_decl
