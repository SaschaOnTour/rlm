//! Discovery / language-mapping tests for `scanner.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "scanner_tests.rs"] mod tests;`.
//!
//! Filter / skip-reason tests (.gitignore, size cap, `.rlm/`) live in
//! the sibling `scanner_filter_tests.rs`.

use super::{detect_ui_context, ext_to_lang, is_supported_extension, Scanner, SkipReason};
use std::fs;
use tempfile::TempDir;

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

#[test]
fn scanner_finds_files() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
    fs::write(tmp.path().join("ignore.exe"), "binary").unwrap();
    let scanner = Scanner::new(tmp.path());
    let files = scanner.scan().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].extension, "rs");
}

#[test]
fn scanner_skips_target_dir() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("lib.rs"), "// compiled").unwrap();
    fs::write(tmp.path().join("src.rs"), "fn main() {}").unwrap();
    let scanner = Scanner::new(tmp.path());
    let files = scanner.scan().unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].relative_path.contains("src.rs"));
}

#[test]
fn scan_all_includes_unsupported() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("main.rs"), "fn main(){}").unwrap();
    fs::write(tmp.path().join("view.cshtml"), "@model X").unwrap();

    let scanner = Scanner::new(tmp.path());
    let files = scanner.scan_all().unwrap();

    assert_eq!(files.len(), 2);

    let rs = files.iter().find(|f| f.extension == "rs").unwrap();
    assert!(rs.supported);
    assert!(rs.reason.is_none());

    let cshtml = files.iter().find(|f| f.extension == "cshtml").unwrap();
    assert!(!cshtml.supported);
    assert_eq!(cshtml.reason, Some(SkipReason::UnsupportedExtension));
}
