//! Tests for `lang_map.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "lang_map_tests.rs"] mod tests;`.

use super::{detect_ui_context, ext_to_lang, is_supported_extension};
#[test]
fn is_supported_extension_works() {
    assert!(is_supported_extension("rs"));
    assert!(is_supported_extension("py"));
    assert!(is_supported_extension("md"));
    assert!(!is_supported_extension("exe"));
    assert!(!is_supported_extension("png"));
}

#[test]
fn ext_to_lang_maps_correctly() {
    assert_eq!(ext_to_lang("rs"), "rust");
    assert_eq!(ext_to_lang("py"), "python");
    assert_eq!(ext_to_lang("cs"), "csharp");
    assert_eq!(ext_to_lang("ts"), "typescript");
    assert_eq!(ext_to_lang("md"), "markdown");
    assert_eq!(ext_to_lang("xyz"), "unknown");
}

#[test]
fn detect_ui_context_works() {
    assert_eq!(detect_ui_context("src/pages/Home.tsx"), Some("page".into()));
    assert_eq!(
        detect_ui_context("src/components/Button.tsx"),
        Some("component".into())
    );
    assert_eq!(
        detect_ui_context("src/screens/Login.tsx"),
        Some("screen".into())
    );
    assert_eq!(detect_ui_context("src/utils/helper.ts"), None);
    assert_eq!(detect_ui_context("src/App.tsx"), Some("ui".into()));
}
