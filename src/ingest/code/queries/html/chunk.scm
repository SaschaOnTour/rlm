
    ; Elements with id attribute
    (element
        (start_tag
            (tag_name) @tag_name
            (attribute
                (attribute_name) @attr_name
                (quoted_attribute_value) @id_value
                (#eq? @attr_name "id")))
        ) @element_with_id

    ; Script elements
    (script_element) @script_el

    ; Style elements
    (style_element) @style_el

    ; Doctype
    (doctype) @doctype_el
