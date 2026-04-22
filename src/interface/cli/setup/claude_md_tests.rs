//! Basic tests for `claude_md.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "claude_md_tests.rs"] mod tests;`.
//!
//! Edge-case / CRLF / self-healing tests live in the sibling
//! `claude_md_edge_tests.rs`.

use super::{
    build_updated_markdown, remove_marker_block, render_claude_local_md_section, MARKER_BEGIN,
    MARKER_END,
};

#[test]
fn render_block_contains_markers() {
    let section = render_claude_local_md_section("\n");
    assert!(section.starts_with(MARKER_BEGIN));
    assert!(section.trim_end().ends_with(MARKER_END));
    assert!(section.contains("rlm Workflow Instructions"));
}

#[test]
fn render_block_uses_requested_eol() {
    let lf = render_claude_local_md_section("\n");
    assert!(!lf.contains('\r'), "LF render must not contain \\r");

    let crlf = render_claude_local_md_section("\r\n");
    // Every LF in the body is paired with a preceding CR.
    let bytes = crlf.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            assert!(i > 0 && bytes[i - 1] == b'\r', "orphaned \\n at byte {i}");
        }
    }
}

#[test]
fn build_markdown_appends_to_empty() {
    let out = build_updated_markdown("");
    assert!(out.contains(MARKER_BEGIN));
    assert!(out.contains(MARKER_END));
}

#[test]
fn build_markdown_preserves_content_outside_markers() {
    let existing =
        "# Project\nSome notes.\n\n<!-- rlm:begin -->\nOLD BLOCK\n<!-- rlm:end -->\n\n## Footer\n";
    let out = build_updated_markdown(existing);
    assert!(out.starts_with("# Project"));
    assert!(out.contains("## Footer"));
    assert!(!out.contains("OLD BLOCK"));
    assert!(out.contains("rlm Workflow Instructions"));
}

#[test]
fn build_markdown_appends_when_no_markers_present() {
    let existing = "# Project\nLine 1\n";
    let out = build_updated_markdown(existing);
    assert!(out.starts_with("# Project"));
    assert!(out.contains(MARKER_BEGIN));
    assert!(out.contains(MARKER_END));
}

#[test]
fn remove_marker_block_strips_block() {
    let existing = "# A\n\n<!-- rlm:begin -->\nstuff\n<!-- rlm:end -->\n\n# B\n";
    let out = remove_marker_block(existing);
    assert!(!out.contains("stuff"));
    assert!(out.contains("# A"));
    assert!(out.contains("# B"));
}

#[test]
fn remove_marker_block_noop_when_absent() {
    let existing = "# A\nNo rlm block here.\n";
    let out = remove_marker_block(existing);
    assert_eq!(out, existing);
}

#[test]
fn template_includes_test_discipline_section() {
    let body = super::render_claude_local_md_section("\n");
    assert!(
        body.contains("### Test discipline"),
        "template should include the Test discipline heading"
    );
    assert!(
        body.contains("test_command") && body.contains("no_tests_warning"),
        "template should reference the key test_impact fields"
    );
    assert!(
        body.contains("build.errors"),
        "template should reference build.errors so agents know to fix compile failures"
    );
}

#[test]
fn template_includes_usage_best_practices() {
    let body = super::render_claude_local_md_section("\n");
    assert!(
        body.contains("Never run `rlm index` manually"),
        "template should warn against redundant index calls"
    );
    assert!(
        body.contains("--code-file"),
        "template should recommend --code-file to avoid heredoc escape issues"
    );
    assert!(
        body.contains("AmbiguousSymbol"),
        "template should explain ambiguous-symbol disambiguation"
    );
}

#[test]
fn template_mentions_similar_symbols_and_extract() {
    let body = super::render_claude_local_md_section("\n");
    assert!(
        body.contains("similar_symbols"),
        "template should explain what to do with similar_symbols"
    );
    assert!(
        body.contains("rlm extract"),
        "template should mention the extract command"
    );
}
