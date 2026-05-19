//! Tests for `path_match::line_carries_path_call`.

use super::line_carries_path_call;

/// `(line, parent, symbol, expected)` — col is auto-located from
/// `line.find(symbol)`, so each test reads as a one-liner that
/// directly mirrors the source form it's checking.
fn assert_path_call(line: &str, parent: &str, symbol: &str, expected: bool) {
    let col = line.find(symbol).expect("symbol must appear on the line") as u32;
    let actual = line_carries_path_call(line, col as usize, parent, symbol);
    assert_eq!(
        actual, expected,
        "line={line:?} parent={parent} symbol={symbol} expected={expected}"
    );
}

#[test]
fn bare_form_matches() {
    assert_path_call("Foo::new()", "Foo", "new", true);
}

#[test]
fn scoped_bare_form_matches() {
    assert_path_call("module::Foo::new()", "Foo", "new", true);
}

#[test]
fn turbofish_form_matches() {
    assert_path_call("Foo::<T>::new()", "Foo", "new", true);
}

#[test]
fn qualified_form_matches() {
    assert_path_call("<Foo>::new()", "Foo", "new", true);
}

#[test]
fn trait_disambiguated_form_matches() {
    assert_path_call("<Foo as Make>::new()", "Foo", "new", true);
}

#[test]
fn trait_disambiguated_with_generics_matches() {
    assert_path_call("<Foo<T> as Make>::new()", "Foo", "new", true);
}

#[test]
fn trait_disambiguated_nested_generics_matches() {
    assert_path_call("<Foo<HashMap<K, V>> as Make>::new()", "Foo", "new", true);
}

#[test]
fn wrong_parent_rejected() {
    assert_path_call("Bar::new()", "Foo", "new", false);
}

#[test]
fn qualified_wrong_parent_rejected() {
    assert_path_call("<Bar as Make>::new()", "Foo", "new", false);
}

#[test]
fn longer_ident_at_col_rejected() {
    // `new_with_x` would match the symbol prefix; the ident-boundary
    // check rejects it so `--symbol new` doesn't fold `new_with_x` in.
    let line = "Foo::new_with_x()";
    let col = line.find("new_with_x").unwrap() as u32;
    assert!(!line_carries_path_call(line, col as usize, "Foo", "new"));
}

#[test]
fn parent_ident_boundary_check_rejects_substring_prefix() {
    // `notFoo::new` ends with `Foo` literally but the boundary check
    // looks one char back; "tFoo" isn't a Foo reference.
    assert_path_call("notFoo::new()", "Foo", "new", false);
}

#[test]
fn method_call_form_rejected() {
    // `var.new()` is a method call, not a path call; no `::` prefix.
    assert_path_call("var.new()", "Foo", "new", false);
}

#[test]
fn qualified_scoped_path_matches() {
    // `<outer::Foo as Make>::new()` — Foo is the last segment of a
    // scoped path inside the angle brackets.
    assert_path_call("<outer::Foo as Make>::new()", "Foo", "new", true);
}

#[test]
fn qualified_deeply_scoped_path_with_generics_matches() {
    // Two-level scoping plus generics: still matches on bare type name.
    assert_path_call("<outer::nested::Foo<T> as Make>::new()", "Foo", "new", true);
}

#[test]
fn qualified_scoped_wrong_last_segment_rejected() {
    // Last segment is `Bar`, not `Foo` — must reject even though
    // `Foo` appears elsewhere in the line.
    assert_path_call("<outer::Bar as Make>::new()", "Foo", "new", false);
}

#[test]
fn qualified_scoped_path_without_trait_matches() {
    // `<outer::Foo>::new()` — scoped path inside brackets, no trait.
    assert_path_call("<outer::Foo>::new()", "Foo", "new", true);
}

// ─── Unicode-safe identifier scanning ────────────────────────────────

#[test]
fn unicode_parent_in_bare_form_does_not_panic_and_matches() {
    // `Föö` is two-byte-per-glyph; the previous byte-at-a-time
    // ident scanner classified `0xC3` (ö's lead byte) as alphabetic
    // standalone, then sliced inside the codepoint and panicked.
    assert_path_call("Föö::new()", "Föö", "new", true);
}

#[test]
fn unicode_parent_in_qualified_form_does_not_panic_and_matches() {
    assert_path_call("<Föö as Make>::new()", "Föö", "new", true);
}

#[test]
fn unicode_parent_in_scoped_qualified_form_matches() {
    assert_path_call("<outer::Föö as Make>::new()", "Föö", "new", true);
}

#[test]
fn unicode_parent_mismatch_rejected() {
    assert_path_call("Föö::new()", "Bär", "new", false);
}

// ─── Whitespace around `::` separator ────────────────────────────────

#[test]
fn whitespace_around_separator_in_bare_form_matches() {
    // Rust accepts `Foo :: new()` as the same path as `Foo::new()`.
    assert_path_call("Foo :: new()", "Foo", "new", true);
}

#[test]
fn whitespace_only_after_separator_in_bare_form_matches() {
    assert_path_call("Foo::  new()", "Foo", "new", true);
}

#[test]
fn whitespace_inside_qualified_brackets_matches() {
    assert_path_call("< Foo as Make >::new()", "Foo", "new", true);
}

#[test]
fn whitespace_around_separator_in_scoped_path_matches() {
    assert_path_call("outer :: Foo :: new()", "Foo", "new", true);
}
