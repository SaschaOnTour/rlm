
    (function_item name: (identifier) @fn_name) @fn_def
    (struct_item name: (type_identifier) @struct_name) @struct_def
    (enum_item name: (type_identifier) @enum_name) @enum_def
    (trait_item name: (type_identifier) @trait_name) @trait_def
    (impl_item type: (type_identifier) @impl_name) @impl_def
    (const_item name: (identifier) @const_name) @const_def
    (static_item name: (identifier) @static_name) @static_def
    (mod_item name: (identifier) @mod_name) @mod_def
    (use_declaration) @use_decl
    (macro_definition name: (identifier) @macro_name) @macro_def
    (type_item name: (type_identifier) @type_alias_name) @type_alias_def
