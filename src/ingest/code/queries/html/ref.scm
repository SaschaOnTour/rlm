
    ; Class references
    (attribute
        (attribute_name) @_class_attr
        (quoted_attribute_value) @class_value
        (#eq? @_class_attr "class"))

    ; href links
    (attribute
        (attribute_name) @_href_attr
        (quoted_attribute_value) @href_value
        (#eq? @_href_attr "href"))

    ; src references
    (attribute
        (attribute_name) @_src_attr
        (quoted_attribute_value) @src_value
        (#eq? @_src_attr "src"))
